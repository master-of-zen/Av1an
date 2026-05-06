use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    path::Path,
};

use crate::EncodeSummaryStats;

pub struct LogMerger {
    encoder:              String,
    total_encode_seconds: f64,
    total_media_seconds:  f64,
    // Frame type stats: (count, total_qp, total_size_or_rate)
    // For x264, 3rd element is Bytes. For x265, strict parsing is harder for rate,
    // but x265 reports 'kb/s' per type. We can try to sum kb/s * time?
    // Or just parse whatever the 3rd metric is.
    // x264 'size' is Bytes. x265 'kb/s' is Rate.
    frame_i_stats:        (usize, f64, f64),
    frame_p_stats:        (usize, f64, f64),
    frame_b_stats:        (usize, f64, f64),

    // Header lines to preserve (from first chunk)
    headers: Vec<String>,

    // Captured log prefix (e.g. "x264 [info]: ")
    log_prefix: String,

    // Flag to stop collecting headers once encoding starts
    header_done: bool,
}

impl LogMerger {
    pub fn new(encoder: &str) -> Self {
        Self {
            encoder:              encoder.to_string(),
            total_encode_seconds: 0.0,
            total_media_seconds:  0.0,
            frame_i_stats:        (0, 0.0, 0.0),
            frame_p_stats:        (0, 0.0, 0.0),
            frame_b_stats:        (0, 0.0, 0.0),
            headers:              Vec::new(),
            log_prefix:           String::new(),
            header_done:          false,
        }
    }

    pub fn process_chunk(&mut self, log_path: &Path, is_first: bool) -> io::Result<()> {
        let file = File::open(log_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let trim = line.trim();

            // Skip garbage / progress lines strictly
            if trim.contains("Frames") || trim.contains("time=") || trim.contains("FPS") {
                continue;
            }

            let is_stat = if self.encoder.contains("x265") {
                self.parse_x265_line(trim)
            } else {
                self.parse_x264_line(trim)
            };

            if is_stat {
                self.header_done = true;
                continue;
            }

            // Capture headers
            if is_first && !self.header_done {
                // Heuristic: Keep lines that look like config/headers.
                // Exclude known stats/progress patterns if they slipped through
                if !trim.contains("frame I:")
                    && !trim.contains("frame P:")
                    && !trim.contains("frame B:")
                    && !trim.contains("encoded")
                    && !trim.contains("Weighted")
                    && !trim.contains("consecutive B-frames")
                    && !trim.contains("kb/s:")
                    && !trim.is_empty()
                {
                    self.headers.push(line.clone());
                }
            }
        }
        Ok(())
    }

    fn parse_x265_line(&mut self, line: &str) -> bool {
        // Robust x265 parser using anchors
        // source: "frame %c: %6u, Avg QP:%2.2lf  kb/s: %s"
        // Example: "x265 [info]: frame I:    473, Avg QP: 8.92  kb/s: 56843.55"

        let matched = self.parse_stats_generic(line, true);
        if matched {
            // Debug print if needed
        }
        matched
    }

    fn parse_x264_line(&mut self, line: &str) -> bool {
        // Robust x264 parser
        // source: "frame %c:%-5d Avg QP:%5.2f  size:%6.0f"
        // Example: "x264 [info]: frame I:17    Avg QP:15.35  size: 38890"

        // Similar generic parsing but expect "size" instead of "kb/s" and handle
        // accumulated bytes
        self.parse_stats_generic(line, false)
    }

    fn parse_stats_generic(&mut self, line: &str, is_x265: bool) -> bool {
        let mut matched = false;

        for &type_char in &['I', 'P', 'B'] {
            let tag = format!("frame {}:", type_char);
            if let Some(mut idx) = line.find(&tag) {
                // Found frame tag.
                idx += tag.len();
                let remainder = line.get(idx..).unwrap_or("");

                // Strategy: Extract number up to next alpha character or comma or colon
                // x265: "    473, Avg QP..."
                // x264: "17    Avg QP..."

                let count_str: String = remainder
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || c.is_whitespace())
                    .collect();

                // If comma exists in count_str, strip it? no, take_while digit/space won't take
                // comma. Wait. x265 has comma. "473, "
                // If I take digits/space, I stop at comma. Correct.

                let trimmed_count = count_str.trim();
                let Ok(count) = trimmed_count.parse::<usize>() else {
                    continue;
                };

                // QP Anchor
                // x265: "Avg QP:" . x264: "Avg QP:" or "QP:"
                let qp_anchor = if remainder.contains("Avg QP:") {
                    "Avg QP:"
                } else {
                    "QP:"
                };
                let qp = remainder.find(qp_anchor).map_or(0.0, |qp_idx| {
                    let s = remainder.get(qp_idx + qp_anchor.len()..).unwrap_or("");
                    // Parse float
                    let qp_val_str: String = s
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    qp_val_str.parse().unwrap_or(0.0)
                });

                // Rate/Size Anchor
                let (metric_val, is_rate) = remainder.find("kb/s:").map_or_else(
                    || {
                        remainder.find("size:").map_or((0.0, is_x265), |sz_idx| {
                            let s = remainder.get(sz_idx + 5..).unwrap_or("");
                            let val_str: String = s
                                .trim_start()
                                .chars()
                                .take_while(|c| c.is_ascii_digit() || *c == '.')
                                .collect();
                            (val_str.parse().unwrap_or(0.0), false)
                        })
                    },
                    |kb_idx| {
                        let s = remainder.get(kb_idx + 5..).unwrap_or("");
                        let val_str: String = s
                            .trim_start()
                            .chars()
                            .take_while(|c| c.is_ascii_digit() || *c == '.')
                            .collect();
                        (val_str.parse().unwrap_or(0.0), true)
                    },
                );

                if matches!(type_char, 'I') {
                    self.capture_prefix(line, &tag);
                }

                // Store
                let stats = match type_char {
                    'I' => &mut self.frame_i_stats,
                    'P' => &mut self.frame_p_stats,
                    'B' => &mut self.frame_b_stats,
                    _ => unreachable!(),
                };

                stats.0 += count;
                stats.1 += qp * count as f64;

                if is_rate {
                    // x265 rate case
                    stats.2 += metric_val * count as f64;
                } else {
                    // x264 size case stores average bytes per frame for each frame type.
                    stats.2 += metric_val * count as f64;
                }

                matched = true;
                break; // Found the type for this line
            }
        }

        if !matched && line.contains("encoded") && line.contains("frames") {
            self.parse_summary(line);
            matched = true;
        }

        matched
    }

    fn capture_prefix(&mut self, line: &str, tag: &str) {
        if self.log_prefix.is_empty() {
            if let Some(idx) = line.find(tag) {
                self.log_prefix = line.get(..idx).unwrap_or("").trim().to_string();
            }
        }
    }

    fn parse_summary(&mut self, line: &str) {
        let mut frames = 0;
        if let Some(idx) = line.find("encoded") {
            let s = line.get(idx + 7..).unwrap_or(""); // after "encoded"
            let frames_str: String = s.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(f) = frames_str.parse::<usize>() {
                frames = f;
            }
        }

        let mut parsed_encode_seconds = false;
        if let Some(in_idx) = line.find("in ") {
            let s = line.get(in_idx + 3..).unwrap_or("");
            let time_str: String =
                s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if let Ok(time) = time_str.parse::<f64>() {
                self.total_encode_seconds += time;
                parsed_encode_seconds = true;
            }
        }

        if let Some(media_seconds) = parse_x264_duration_seconds(line) {
            self.total_media_seconds += media_seconds;
        }

        if frames > 0 && !parsed_encode_seconds {
            if let Some(fps) = parse_summary_fps(line) {
                if fps > 0.0 {
                    self.total_encode_seconds += frames as f64 / fps;
                }
            }
        }
    }

    #[cfg(test)]
    pub fn write_merged<W: Write>(&self, mut writer: W) -> io::Result<()> {
        let inferred_summary = self.inferred_summary();
        self.write_merged_inner(&mut writer, inferred_summary.as_ref())
    }

    pub fn write_merged_with_summary<W: Write>(
        &self,
        mut writer: W,
        summary: &EncodeSummaryStats,
    ) -> io::Result<()> {
        self.write_merged_inner(&mut writer, Some(summary))
    }

    #[cfg(test)]
    fn inferred_summary(&self) -> Option<EncodeSummaryStats> {
        let total_frames = self.frame_i_stats.0 + self.frame_p_stats.0 + self.frame_b_stats.0;
        if total_frames == 0 {
            return None;
        }

        let total_bytes =
            (self.frame_i_stats.2 + self.frame_p_stats.2 + self.frame_b_stats.2).round() as u64;

        Some(EncodeSummaryStats {
            total_frames,
            media_seconds: self.total_media_seconds,
            encode_seconds: self.total_encode_seconds,
            total_bytes,
        })
    }

    fn write_merged_inner<W: Write>(
        &self,
        mut writer: W,
        summary: Option<&EncodeSummaryStats>,
    ) -> io::Result<()> {
        for line in &self.headers {
            writeln!(writer, "{}", line)?;
        }
        writeln!(writer)?; // Spacing after headers

        let prefix = if self.log_prefix.is_empty() {
            if self.encoder.contains("x265") {
                "x265 [info]:"
            } else {
                "x264 [info]:"
            }
        } else {
            &self.log_prefix
        };

        // Write Frame Stats
        // x265: "frame I:    473, Avg QP: 8.92  kb/s: 56843.55"
        // x264: "frame I:17    Avg QP:15.35  size: 38890"

        let write_stat = |w: &mut W, type_char: char, s: (usize, f64, f64)| -> io::Result<()> {
            if s.0 == 0 {
                return Ok(());
            }
            let avg_qp = s.1 / s.0 as f64;

            if self.encoder.contains("x265") {
                let avg_rate = s.2 / s.0 as f64;
                writeln!(
                    w,
                    "{} frame {}: {:>8}, Avg QP: {:>5.2}  kb/s: {:>8.2}",
                    prefix, type_char, s.0, avg_qp, avg_rate
                )?;
            } else {
                let avg_size = (s.2 / s.0 as f64).round() as u64;
                writeln!(
                    w,
                    "{} frame {}:{:>5}    Avg QP:{:>5.2}  size: {:>8}",
                    prefix, type_char, s.0, avg_qp, avg_size
                )?;
            }
            Ok(())
        };

        write_stat(&mut writer, 'I', self.frame_i_stats)?;
        write_stat(&mut writer, 'P', self.frame_p_stats)?;
        write_stat(&mut writer, 'B', self.frame_b_stats)?;

        writeln!(writer)?;

        let total_frames = summary.map_or_else(
            || self.frame_i_stats.0 + self.frame_p_stats.0 + self.frame_b_stats.0,
            |summary| summary.total_frames,
        );
        let encode_seconds =
            summary.map_or(self.total_encode_seconds, |summary| summary.encode_seconds);
        let media_seconds = summary.map_or_else(
            || {
                if self.total_media_seconds > 0.0 {
                    self.total_media_seconds
                } else {
                    self.total_encode_seconds
                }
            },
            |summary| summary.media_seconds,
        );

        let fps = if encode_seconds > 0.0 {
            total_frames as f64 / encode_seconds
        } else {
            0.0
        };

        let kbps = if self.encoder.contains("x265") {
            let sum_rate_count = self.frame_i_stats.2 + self.frame_p_stats.2 + self.frame_b_stats.2;
            if total_frames > 0 {
                sum_rate_count / total_frames as f64
            } else {
                0.0
            }
        } else {
            let total_bytes = summary.map_or_else(
                || {
                    (self.frame_i_stats.2 + self.frame_p_stats.2 + self.frame_b_stats.2).round()
                        as u64
                },
                |summary| summary.total_bytes,
            );
            if media_seconds > 0.0 {
                total_bytes as f64 * 8.0 / 1000.0 / media_seconds
            } else {
                0.0
            }
        };

        let avg_qp = if total_frames > 0 {
            (self.frame_i_stats.1 + self.frame_p_stats.1 + self.frame_b_stats.1)
                / total_frames as f64
        } else {
            0.0
        };

        if self.encoder.contains("x265") {
            // encoded 33138 frames in 9989.67s (3.32 fps), 20845.84 kb/s, Avg QP:11.15
            writeln!(
                writer,
                "encoded {} frames in {:.2}s ({:.2} fps), {:.2} kb/s, Avg QP:{:.2}",
                total_frames, encode_seconds, fps, kbps, avg_qp
            )?;
        } else {
            let duration_suffix = if media_seconds > 0.0 {
                format!(", duration {}", format_duration(media_seconds))
            } else {
                String::new()
            };
            writeln!(
                writer,
                "{} encoded {} frames, {:.2} fps, {:.2} kb/s{}",
                prefix, total_frames, fps, kbps, duration_suffix
            )?;
        }

        Ok(())
    }
}

fn parse_summary_fps(line: &str) -> Option<f64> {
    let fps_idx = line.find("fps")?;
    let before_fps = line.get(..fps_idx)?;
    let fps_str: String = before_fps
        .trim()
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let fps_str: String = fps_str.chars().rev().collect();
    fps_str.parse::<f64>().ok()
}

fn parse_x264_duration_seconds(line: &str) -> Option<f64> {
    let duration_idx = line.find("duration ")?;
    let duration = line.get(duration_idx + "duration ".len()..)?.trim();
    let mut parts = duration.split(':');
    let hours = parts.next()?.parse::<u64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

fn format_duration(total_seconds: f64) -> String {
    let hours = (total_seconds / 3600.0).floor() as u64;
    let minutes = ((total_seconds % 3600.0) / 60.0).floor() as u64;
    let seconds = total_seconds % 60.0;
    format!("{hours}:{minutes:02}:{seconds:05.2}")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    use super::LogMerger;
    use crate::EncodeSummaryStats;

    fn write_temp_log(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("should write temp log");
        path
    }

    #[test]
    fn x264_frame_size_is_weighted_by_frame_count() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk1 = write_temp_log(
            &dir,
            "chunk1.log",
            "x264 [info]: frame I:2     Avg QP:10.00  size:      100\nx264 [info]: encoded 2 \
             frames, 1.00 fps, 1.60 kb/s, duration 0:00:02.00\n",
        );
        let chunk2 = write_temp_log(
            &dir,
            "chunk2.log",
            "x264 [info]: frame I:1     Avg QP:20.00  size:      200\nx264 [info]: encoded 1 \
             frames, 1.00 fps, 1.60 kb/s, duration 0:00:01.00\n",
        );

        let mut merger = LogMerger::new("x264");
        merger.process_chunk(&chunk1, true).expect("chunk1 should parse");
        merger.process_chunk(&chunk2, false).expect("chunk2 should parse");

        let mut output = Vec::new();
        merger.write_merged(&mut output).expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        assert!(
            output.contains("frame I:    3    Avg QP:13.33  size:      133"),
            "unexpected merged output:\n{output}"
        );
    }

    #[test]
    fn x264_summary_uses_external_encode_stats() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk1 = write_temp_log(
            &dir,
            "chunk1.log",
            "x264 [info]: frame I:2     Avg QP:10.00  size:      100\nx264 [info]: encoded 2 \
             frames, 2.00 fps, 9999.99 kb/s, duration 0:00:01.00\n",
        );
        let chunk2 = write_temp_log(
            &dir,
            "chunk2.log",
            "x264 [info]: frame P:2     Avg QP:20.00  size:       50\nx264 [info]: encoded 2 \
             frames, 1.00 fps, 8888.88 kb/s, duration 0:00:02.00\n",
        );

        let mut merger = LogMerger::new("x264");
        merger.process_chunk(&chunk1, true).expect("chunk1 should parse");
        merger.process_chunk(&chunk2, false).expect("chunk2 should parse");

        let mut output = Vec::new();
        merger
            .write_merged_with_summary(&mut output, &EncodeSummaryStats {
                total_frames:   4,
                media_seconds:  2.0,
                encode_seconds: 0.5,
                total_bytes:    300,
            })
            .expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        assert!(
            output.contains(
                "x264 [info]: encoded 4 frames, 8.00 fps, 1.20 kb/s, duration 0:00:02.00"
            ),
            "unexpected merged output:\n{output}"
        );
    }
}
