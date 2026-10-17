//! Merging of the per-chunk x264/x265 text logs into one encoder-style log.
//!
//! av1an encodes every chunk separately, so every chunk produces its own copy
//! of the encoder's log. This module re-assembles those copies into a single
//! file that is laid out exactly the way the encoder itself would have printed
//! a full-length run:
//!
//! ```text
//! x264 [info]: using cpu capabilities: ARMv8 NEON DotProd
//! x264 [info]: profile High, level 1.3, 4:2:0, 8-bit
//!
//! x264 [info]: frame I:128   Avg QP:15.41  size:133675
//! x264 [info]: frame P:1506  Avg QP:19.61  size: 34845
//! x264 [info]: frame B:1632  Avg QP:23.43  size:  8615
//! x264 [info]: kb/s:2945.52
//!
//! encoded 3266 frames, 114.91 fps, 2945.52 kb/s
//! ```
//!
//! Frame counts are summed across chunks while QP, size and bitrate are
//! weighted by those counts, which is exactly how x264 and x265 compute their
//! own totals. Progress-bar redraws and per-chunk detail lines (`mb I`, `ref P
//! L0`, ...) are dropped: they either carry no information at all or describe a
//! single chunk only, so re-printing them would be misleading.

use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    path::Path,
};

use crate::{parse::starts_with_ignore_ascii_case, EncodeSummaryStats};

/// Order in which the frame types are reported by both encoders.
const FRAME_TYPES: [char; 3] = ['I', 'P', 'B'];

/// The encoder log flavour that the merged log reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    X264,
    X265,
}

impl LogFormat {
    fn for_encoder(encoder: &str) -> Self {
        if encoder.contains("x265") {
            Self::X265
        } else {
            Self::X264
        }
    }

    /// Prefix used when the encoder's own prefix could not be recovered from a
    /// statistics line.
    fn default_prefix(self) -> &'static str {
        match self {
            Self::X264 => "x264 [info]:",
            Self::X265 => "x265 [info]:",
        }
    }
}

/// How the encoder reports its encoding time.
///
/// The official encoders print plain seconds while the builds of
/// [Patman](https://github.com/Patman86) print a clock and mark their log level
/// in upper case:
///
/// ```text
/// official x264:  encoded 844 frames, 115.70 fps, 2930.10 kb/s
/// Patman   x264:  encoded 844 frames, 115.70 fps, 2930.10 kb/s, duration 0:00:07.29
/// official x265:  encoded 844 frames in 7.29s (115.70 fps), 2930.10 kb/s, Avg QP:20.00
/// Patman   x265:  encoded 844 frames in 00:00:07.290 (115.70 fps), 2930.10 kb/s, Avg QP:20.00
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncodeTimeStyle {
    Official,
    Patman,
}

/// Accumulated statistics of a single frame type.
///
/// `metric_sum` holds the third column of the encoder's statistics line (the
/// average size in bytes for x264, the kb/s for x265) multiplied by the number
/// of frames, so that the average over all chunks stays correct.
#[derive(Debug, Default, Clone, Copy)]
struct FrameTotals {
    count:      usize,
    qp_sum:     f64,
    metric_sum: f64,
}

impl FrameTotals {
    fn add(&mut self, count: usize, avg_qp: f64, metric: f64) {
        self.count += count;
        self.qp_sum = avg_qp.mul_add(count as f64, self.qp_sum);
        self.metric_sum = metric.mul_add(count as f64, self.metric_sum);
    }

    fn avg_qp(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.qp_sum / self.count as f64
        }
    }

    fn avg_metric(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.metric_sum / self.count as f64
        }
    }
}

pub struct LogMerger {
    format:              LogFormat,
    /// The encoder's own log prefix, e.g. `x264 [info]:`.
    prefix:              Option<String>,
    /// Configuration/version lines, kept verbatim from the first chunk log.
    headers:             Vec<String>,
    /// Set once the first chunk log with usable content has been read: the
    /// configuration headers are only taken from that one.
    headers_done:        bool,
    /// Set once a chunk log reached its statistics block.
    statistics_started:  bool,
    frames:              [FrameTotals; 3],
    /// Bytes of the chunk log that is currently being processed, used to
    /// recover the media duration from x264's `kb/s:` line.
    current_chunk_bytes: f64,
    /// Summary values recovered from the per-chunk summary lines, used when no
    /// externally computed summary is supplied.
    summary_frames:      usize,
    summary_media_secs:  f64,
    summary_encode_secs: f64,
    /// How the encoder reports its encoding time, detected from the chunk logs.
    encode_time_style:   Option<EncodeTimeStyle>,
}

impl LogMerger {
    pub fn new(encoder: &str) -> Self {
        Self {
            format:              LogFormat::for_encoder(encoder),
            prefix:              None,
            headers:             Vec::new(),
            headers_done:        false,
            statistics_started:  false,
            frames:              [FrameTotals::default(); 3],
            current_chunk_bytes: 0.0,
            summary_frames:      0,
            summary_media_secs:  0.0,
            summary_encode_secs: 0.0,
            encode_time_style:   None,
        }
    }

    /// Reads one per-chunk encoder log and folds it into the merged statistics.
    pub fn process_chunk(&mut self, log_path: &Path) -> io::Result<()> {
        self.current_chunk_bytes = 0.0;

        let reader = BufReader::new(File::open(log_path)?);

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();

            // Progress bars are redrawn constantly and carry no information.
            if is_encoder_progress_line(trimmed) {
                continue;
            }

            if self.parse_frame_statistics(trimmed) || self.parse_summary(trimmed) {
                // The statistics block of this chunk has started; everything
                // before it was part of the configuration header.
                self.statistics_started = true;
                continue;
            }

            if self.parse_bitrate(trimmed) {
                continue;
            }

            // Configuration/version headers are identical in every chunk log,
            // so only the copy of the first chunk is kept. Anything
            // that belongs to a chunk's statistics block is never
            // repeated either.
            if self.capturing_headers() && !is_chunk_statistics_line(trimmed) {
                self.headers.push(trimmed.to_string());
            }
        }

        // The configuration header is only taken from the first chunk log that
        // contains anything at all; an empty log must not consume that slot.
        if !self.headers.is_empty() || self.statistics_started {
            self.headers_done = true;
        }

        Ok(())
    }

    /// Headers are only collected until the statistics block of the first chunk
    /// log that actually contains something.
    fn capturing_headers(&self) -> bool {
        !self.headers_done && !self.statistics_started
    }

    /// Parses one `frame I:`, `frame P:` or `frame B:` statistics line.
    fn parse_frame_statistics(&mut self, line: &str) -> bool {
        for (slot, frame_type) in FRAME_TYPES.iter().enumerate() {
            let tag = format!("frame {frame_type}:");
            let Some(tag_index) = line.find(&tag) else {
                continue;
            };

            let rest = line.get(tag_index + tag.len()..).unwrap_or_default();

            // Remember how the encoder prefixes its log lines, so the merged
            // log can look exactly like the encoder's own output.
            if self.prefix.is_none() {
                let prefix = line.get(..tag_index).unwrap_or_default().trim_end();
                if !prefix.is_empty() {
                    self.prefix = Some(prefix.to_string());
                }
            }

            let Some(count) = parse_leading_usize(rest) else {
                // Unreadable counter: still part of the statistics block, so it
                // must never end up in the header section.
                return true;
            };

            // x264: "17    Avg QP:15.35  size: 38890"
            // x265: "      1, Avg QP:29.06  kb/s: 856.56"
            let avg_qp = value_after(rest, "Avg QP:").and_then(parse_leading_f64).unwrap_or(0.0);
            let metric = value_after(rest, "kb/s:")
                .or_else(|| value_after(rest, "size:"))
                .and_then(parse_leading_f64)
                .unwrap_or(0.0);

            self.frames[slot].add(count, avg_qp, metric);

            if self.format == LogFormat::X264 {
                self.current_chunk_bytes = metric.mul_add(count as f64, self.current_chunk_bytes);
            }

            return true;
        }

        false
    }

    /// Parses one `encoded N frames ...` summary line.
    fn parse_summary(&mut self, line: &str) -> bool {
        if !line.contains("encoded") || !line.contains("frames") {
            return false;
        }

        let frames = value_after(line, "encoded").and_then(parse_leading_usize);
        if let Some(frames) = frames {
            self.summary_frames += frames;
        }

        // x265 reports the encoding time, x264 only the achieved frame rate,
        // from which the encoding time follows.
        if let Some((encode_secs, style)) = value_after(line, "in ").and_then(parse_encode_time) {
            self.summary_encode_secs += encode_secs;
            self.encode_time_style = Some(style);
        } else if let Some((encode_secs, style)) =
            value_after(line, "duration ").and_then(parse_encode_time)
        {
            // Patman's x264 appends the encoding time as ", duration
            // 0:00:00.41".
            self.summary_encode_secs += encode_secs;
            self.encode_time_style = Some(style);
        } else if let Some(fps) = parse_fps(line)
            && let Some(frames) = frames
            && fps > 0.0
        {
            self.summary_encode_secs += frames as f64 / fps;
        }

        true
    }

    /// The style of summary line that the merged log has to reproduce.
    ///
    /// It is taken from the encoder's own summary lines. If a chunk log
    /// contains no summary at all, the captured log prefix tells the two
    /// build variants apart, as Patman's mods print their log level in
    /// upper case (`x264 [INFO]:`) and always append the encoding time.
    fn encode_time_style(&self) -> EncodeTimeStyle {
        self.encode_time_style.unwrap_or_else(|| {
            let is_patman = self.prefix.as_deref().is_some_and(|prefix| prefix.contains("[INFO]"));
            if is_patman {
                EncodeTimeStyle::Patman
            } else {
                EncodeTimeStyle::Official
            }
        })
    }

    /// Parses x264's overall bitrate line, e.g. `x264 [info]: kb/s:297.76`.
    ///
    /// x265 prints a kb/s value per frame type instead, which is handled as a
    /// frame statistic.
    fn parse_bitrate(&mut self, line: &str) -> bool {
        let Some(kbps) = value_after(line, "kb/s:").and_then(parse_leading_f64) else {
            return false;
        };

        // The bitrate line closes a chunk's statistics block, so the bytes of
        // that chunk are complete and its media duration follows from them.
        if self.format == LogFormat::X264 && kbps > 0.0 && self.current_chunk_bytes > 0.0 {
            self.summary_media_secs += self.current_chunk_bytes * 8.0 / 1000.0 / kbps;
        }

        true
    }

    /// Writes the merged log, deriving the summary from the parsed chunk logs.
    pub fn write_merged<W: Write>(&self, writer: W) -> io::Result<()> {
        let inferred_summary = self.inferred_summary();
        self.write_merged_inner(writer, inferred_summary.as_ref())
    }

    /// Writes the merged log using the authoritative statistics that av1an
    /// collected while encoding.
    pub fn write_merged_with_summary<W: Write>(
        &self,
        writer: W,
        summary: &EncodeSummaryStats,
    ) -> io::Result<()> {
        self.write_merged_inner(writer, Some(summary))
    }

    /// Summary reconstructed from the chunk logs alone, used when av1an's own
    /// statistics are unavailable.
    fn inferred_summary(&self) -> Option<EncodeSummaryStats> {
        let parsed_frames = self.parsed_frames();
        if parsed_frames == 0 && self.summary_frames == 0 {
            return None;
        }

        Some(EncodeSummaryStats {
            total_frames:   if parsed_frames > 0 {
                parsed_frames
            } else {
                self.summary_frames
            },
            media_seconds:  self.summary_media_secs,
            encode_seconds: self.summary_encode_secs,
            total_bytes:    self.parsed_bytes(),
        })
    }

    fn parsed_frames(&self) -> usize {
        self.frames.iter().map(|frame| frame.count).sum()
    }

    fn parsed_bytes(&self) -> u64 {
        self.frames.iter().map(|frame| frame.metric_sum).sum::<f64>().round() as u64
    }

    fn write_merged_inner<W: Write>(
        &self,
        mut writer: W,
        summary: Option<&EncodeSummaryStats>,
    ) -> io::Result<()> {
        let prefix = self.prefix.as_deref().unwrap_or_else(|| self.format.default_prefix());

        for header in &self.headers {
            writeln!(writer, "{header}")?;
        }
        if !self.headers.is_empty() {
            writeln!(writer)?;
        }

        let parsed_frames = self.parsed_frames();
        let total_frames = summary.map_or_else(
            || {
                if parsed_frames > 0 {
                    parsed_frames
                } else {
                    self.summary_frames
                }
            },
            |summary| summary.total_frames,
        );
        let encode_secs = summary
            .map(|summary| summary.encode_seconds)
            .filter(|secs| *secs > 0.0)
            .unwrap_or(self.summary_encode_secs);

        // The media duration is what the encoders base their bitrate on. It is
        // not part of every encoder summary, so fall back to whatever the chunk
        // logs allowed us to recover.
        let media_secs = summary
            .map(|summary| summary.media_seconds)
            .filter(|secs| *secs > 0.0)
            .or_else(|| (self.summary_media_secs > 0.0).then_some(self.summary_media_secs))
            .or_else(|| (self.summary_encode_secs > 0.0).then_some(self.summary_encode_secs))
            .unwrap_or(0.0);

        let rate_sum = self.frames.iter().map(|frame| frame.metric_sum).sum::<f64>();
        let kbps = if self.format == LogFormat::X265 && parsed_frames > 0 && rate_sum > 0.0 {
            // x265's per-frame-type kb/s values average exactly when weighted
            // by the number of frames of that type.
            rate_sum / parsed_frames as f64
        } else {
            let total_bytes = summary
                .map(|summary| summary.total_bytes)
                .filter(|bytes| *bytes > 0)
                .unwrap_or_else(|| self.parsed_bytes());
            if media_secs > 0.0 {
                total_bytes as f64 * 8.0 / 1000.0 / media_secs
            } else {
                0.0
            }
        };

        for (slot, frame_type) in FRAME_TYPES.iter().enumerate() {
            self.write_frame_statistics(&mut writer, prefix, *frame_type, self.frames[slot])?;
        }

        // x264 closes its statistics block with the overall bitrate.
        if self.format == LogFormat::X264 && kbps > 0.0 {
            writeln!(writer, "{prefix} kb/s:{kbps:.2}")?;
        }

        writeln!(writer)?;

        let fps = if encode_secs > 0.0 {
            total_frames as f64 / encode_secs
        } else {
            0.0
        };

        match self.format {
            // x264: "encoded %d frames, %.2f fps, %.2f kb/s"
            // Patman's x264 appends ", duration %d:%02d:%02d.%02d".
            LogFormat::X264 => {
                let duration =
                    if self.encode_time_style() == EncodeTimeStyle::Patman && encode_secs > 0.0 {
                        format!(", duration {}", format_duration(encode_secs))
                    } else {
                        String::new()
                    };
                writeln!(
                    writer,
                    "encoded {total_frames} frames, {fps:.2} fps, {kbps:.2} kb/s{duration}"
                )?;
            },
            // x265: "encoded %d frames in %.2fs (%.2f fps), %.2f kb/s, Avg QP:%2.2lf"
            // Patman's x265 reports the time as "%02d:%02d:%02d.%03d" instead.
            LogFormat::X265 => {
                let avg_qp = if parsed_frames > 0 {
                    self.frames.iter().map(|frame| frame.qp_sum).sum::<f64>() / parsed_frames as f64
                } else {
                    0.0
                };
                let elapsed = match self.encode_time_style() {
                    EncodeTimeStyle::Official => format!("{encode_secs:.2}s"),
                    EncodeTimeStyle::Patman => format_clock_millis(encode_secs),
                };
                writeln!(
                    writer,
                    "encoded {total_frames} frames in {elapsed} ({fps:.2} fps), {kbps:.2} kb/s, \
                     Avg QP:{avg_qp:.2}"
                )?;
            },
        }

        Ok(())
    }

    /// Writes one frame-type statistics line using the encoder's own layout.
    fn write_frame_statistics<W: Write>(
        &self,
        writer: &mut W,
        prefix: &str,
        frame_type: char,
        totals: FrameTotals,
    ) -> io::Result<()> {
        if totals.count == 0 {
            return Ok(());
        }

        let count = totals.count;
        let avg_qp = totals.avg_qp();

        match self.format {
            // x264: "frame %c:%-5d Avg QP:%5.2f  size:%6.0f"
            LogFormat::X264 => {
                let avg_size = totals.avg_metric().round() as u64;
                writeln!(
                    writer,
                    "{prefix} frame {frame_type}:{count:<5} Avg QP:{avg_qp:>5.2}  \
                     size:{avg_size:>6}"
                )?;
            },
            // x265: "frame %c: %6u, Avg QP:%2.2lf  kb/s: %-8.2lf"
            LogFormat::X265 => {
                let avg_rate = totals.avg_metric();
                writeln!(
                    writer,
                    "{prefix} frame {frame_type}: {count:>6}, Avg QP:{avg_qp:.2}  kb/s: \
                     {avg_rate:<8.2}"
                )?;
            },
        }

        Ok(())
    }
}

/// Returns true when a line of encoder stderr should be written to the
/// per-chunk log file.
///
/// Everything except the redrawn progress bar is kept: the configuration header
/// lines are needed to reproduce the encoder's own log, and the statistics
/// lines are needed to merge it.
pub(crate) fn should_capture_chunk_log_line(line: &str) -> bool {
    !is_encoder_progress_line(line)
}

/// Writes the part of an encoder stderr buffer that belongs in the per-chunk
/// log file.
///
/// av1an reads encoder stderr with `read_until(b'\r')`, so one buffer may hold
/// several `\n` separated lines, the redraw of a progress line, or the padding
/// that clears it. Lines are therefore filtered individually.
pub(crate) fn capture_log_lines<W: Write>(buffer: &str, writer: &mut W) -> io::Result<()> {
    for part in buffer.split_inclusive('\n') {
        if !should_capture_chunk_log_line(part) {
            continue;
        }

        writer.write_all(part.as_bytes())?;
        // Ensure a newline if the buffer ended mid-line, so that the merged log
        // never sees two lines glued together.
        if !part.ends_with('\n') {
            writer.write_all(b"\n")?;
        }
    }

    Ok(())
}

/// Returns true for progress-bar lines, which are redrawn continuously and
/// therefore carry no information.
///
/// The official encoders print their progress as
/// `[ 42.5%] 121/285 frames, 2.47 fps, 1445.28 kb/s, eta 0:01:06`, or, when the
/// total frame count is unknown, as `121 frames: 2.47 fps, 1445.28 kb/s`.
/// Patman's builds keep the percentage marker but add columns, and their x265
/// build capitalises `Frames`/`FPS`:
///
/// ```text
/// x264 [38.2%] 844/2208 frames @ 115.7 fps | 2930.1 kb/s | 0:00:07 [-0:00:11] | 12.89 MB [33.73 MB]
/// [38.2%] 844/2208 Frames @ 115.7 FPS | 2930.1 kb/s | 0:00:07 [-0:00:11] | 12.89 MB [33.73 MB]
/// x264 844 frames @ 115.7 fps | 2930.1 kb/s | 12.89 MB
/// 844 Frames @ 115.7 FPS | 2930.1 kb/s | 12.89 MB
/// ```
pub(crate) fn is_encoder_progress_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }

    // ffmpeg/vspipe style progress, e.g. "frame= 100 fps=25 time=00:00:04.00".
    if trimmed.contains("time=") {
        return true;
    }

    // The "encoded N frames ..." summary is not a progress line.
    if trimmed.contains("encoded") {
        return false;
    }

    // Every variant redraws a percentage marker, except when the encoder does
    // not know the total frame count.
    if trimmed.contains("%]") {
        return true;
    }

    // "121/285 frames", "121 frames:" or "121 Frames @ ..." without a marker.
    let mut previous: Option<&str> = None;
    for token in trimmed.split_ascii_whitespace() {
        if starts_with_ignore_ascii_case(token, "frames")
            && let Some(candidate) = previous
        {
            let count = candidate.split_once('/').map_or(candidate, |(count, _)| count);
            if count.parse::<u64>().is_ok() {
                return true;
            }
        }
        previous = Some(token);
    }

    false
}

/// Returns true for lines that belong to a chunk's statistics block rather than
/// to its configuration header.
///
/// The statistics block always follows the header, so this is only a safety net
/// for lines whose `frame X:` counter could not be parsed.
fn is_chunk_statistics_line(line: &str) -> bool {
    const DETAIL_ANCHORS: [&str; 15] = [
        "consecutive B-frames:",
        "mb I  ",
        "mb P  ",
        "mb B  ",
        "8x8 transform",
        "coded y,uvDC,uvAC",
        "i16 v,h,dc,p:",
        "i8 v,h,dc,ddl",
        "i4 v,h,dc,ddl",
        "i8c dc,h,v,p:",
        "Weighted P-Frames:",
        "Weighted B-Frames:",
        "ref P L0:",
        "ref B L0:",
        "ref B L1:",
    ];

    line.contains("frame I:")
        || line.contains("frame P:")
        || line.contains("frame B:")
        || (line.contains("encoded") && line.contains("frames"))
        || line.contains("kb/s:")
        || DETAIL_ANCHORS.iter().any(|anchor| line.contains(anchor))
}

/// Parses the decimal integer at the start of `value`, ignoring leading spaces.
fn parse_leading_usize(value: &str) -> Option<usize> {
    let digits: String = value.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Parses the decimal number at the start of `value`, ignoring leading spaces.
fn parse_leading_f64(value: &str) -> Option<f64> {
    let number: String = value
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok()
}

/// Returns what follows `anchor` on the same line.
fn value_after<'a>(line: &'a str, anchor: &str) -> Option<&'a str> {
    let index = line.find(anchor)?;
    line.get(index + anchor.len()..)
}

/// Returns the number directly preceding the first `fps` on the line.
fn parse_fps(line: &str) -> Option<f64> {
    let index = line.find("fps")?;
    let before: String = line
        .get(..index)?
        .trim()
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    before.chars().rev().collect::<String>().parse().ok()
}

/// Cuts the `H:MM:SS.ss` (or `MM:SS.ss`) clock at the start of `value`.
fn leading_clock(value: &str) -> String {
    value
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ':' || *c == '.')
        .collect()
}

/// Parses an encoding time, which is either a plain number of seconds (`1.39s`)
/// or a clock (`00:00:01.389`), together with the build style it implies.
fn parse_encode_time(value: &str) -> Option<(f64, EncodeTimeStyle)> {
    let clock = leading_clock(value);
    if clock.contains(':') {
        return parse_clock_secs(&clock).map(|secs| (secs, EncodeTimeStyle::Patman));
    }

    parse_leading_f64(value).map(|secs| (secs, EncodeTimeStyle::Official))
}

/// Formats seconds the way Patman's x265 prints its encoding time, as
/// `HH:MM:SS.mmm`.
fn format_clock_millis(total_seconds: f64) -> String {
    let total_millis = if total_seconds > 0.0 {
        (total_seconds * 1000.0) as u64
    } else {
        0
    };
    let millis = total_millis % 1000;
    let total_secs = total_millis / 1000;
    let seconds = total_secs % 60;
    let minutes = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Converts an `H:MM:SS.ss` or `MM:SS.ss` clock into seconds.
fn parse_clock_secs(clock: &str) -> Option<f64> {
    let mut seconds = 0.0f64;
    let mut fields = 0;
    for field in clock.split(':') {
        seconds = seconds.mul_add(60.0, field.parse::<f64>().ok()?);
        fields += 1;
    }

    (fields >= 2).then_some(seconds)
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

    use super::{is_encoder_progress_line, should_capture_chunk_log_line, LogMerger};
    use crate::EncodeSummaryStats;

    /// Real x264 0.165 log of a 120 frame encode, including the progress lines
    /// that x264 writes to stderr.
    const X264_CHUNK_0: &str = "\
yuv [info]: 320x240p 0:0 @ 30/1 fps (cfr)
x264 [info]: using cpu capabilities: ARMv8 NEON DotProd
x264 [info]: profile High, level 1.3, 4:2:0, 8-bit
[0.8%] 1/120 frames, 27.54 fps, 1199.52 kb/s, eta 0:00:04  
x264 [38.2%] 46/120 frames @ 115.7 fps | 2930.1 kb/s | 0:00:07 [-0:00:11] | 12.89 MB [33.73 MB]
46 frames: 115.70 fps, 2930.10 kb/s
x264 [info]: frame I:1     Avg QP:22.65  size:  4998
x264 [info]: frame P:38    Avg QP:27.05  size:  1779
x264 [info]: frame B:81    Avg QP:32.03  size:   942
x264 [info]: consecutive B-frames:  5.8%  8.3% 12.5% 73.3%
x264 [info]: mb I  I16..4: 31.3% 44.7% 24.0%
x264 [info]: mb P  I16..4:  2.4%  4.4%  1.0%  P16..4: 13.7%  9.8%  7.3%  0.0%  0.0%    skip:61.4%
x264 [info]: mb B  I16..4:  0.2%  0.3%  0.1%  B16..8: 19.4%  6.3%  1.5%  direct: 2.3%  skip:70.0%  \
                                L0:46.9% L1:47.8% BI: 5.4%
x264 [info]: 8x8 transform intra:53.3% inter:31.0%
x264 [info]: coded y,uvDC,uvAC intra: 9.7% 27.5% 23.2% inter: 4.9% 13.5% 11.6%
x264 [info]: i16 v,h,dc,p: 74% 20%  6%  0%
x264 [info]: i8 v,h,dc,ddl,ddr,vr,hd,vl,hu:  9% 10% 79%  1%  0%  0%  0%  0%  0%
x264 [info]: i4 v,h,dc,ddl,ddr,vr,hd,vl,hu: 42% 22% 31%  1%  1%  1%  1%  1%  1%
x264 [info]: i8c dc,h,v,p: 51% 16% 33%  1%
x264 [info]: Weighted P-Frames: Y:0.0% UV:0.0%
x264 [info]: ref P L0: 62.6%  7.6% 18.5% 11.4%
x264 [info]: ref B L0: 81.3% 14.6%  4.1%
x264 [info]: ref B L1: 94.7%  5.3%
x264 [info]: kb/s:297.76

encoded 120 frames, 752.64 fps, 297.76 kb/s
";

    /// Real x265 4.3 log of a 120 frame encode.
    const X265_CHUNK_0: &str = "\
yuv  [info]: 320x240 fps 30000/1000 i420p8 frames 0 - 119 of 120
raw  [info]: output file: out265.hevc
x265 [info]: HEVC encoder version 4.3+1-e9b8812
x265 [info]: build info [Mac OS X][clang 17.0.0][64 bit] 8bit+10bit+12bit
x265 [info]: using cpu capabilities: NEON Neon_DotProd
x265 [info]: Main profile, Level-2 (Main tier)
x265 [info]: Thread pool created using 8 threads
x265 [warning]: Source height < 720p; disabling lookahead-slices
x265 [info]: frame threads / pool features       : 3 / wpp(4 rows)
x265 [info]: Lookahead / bframes / badapt        : 20 / 4 / 2
[0.8%] 1/120 frames, 5.49 fps, 1435.20 kb/s, eta 0:00:21  
[20.0%] 24/120 frames, 54.24 fps, 259.79 kb/s, eta 0:00:01  
x265 [info]: frame I:      1, Avg QP:29.06  kb/s: 856.56  
x265 [info]: frame P:     33, Avg QP:30.06  kb/s: 403.17  
x265 [info]: frame B:     86, Avg QP:35.59  kb/s: 120.73  
x265 [info]: Weighted P-Frames: Y:0.0% UV:0.0%

encoded 120 frames in 1.39s (86.41 fps), 204.53 kb/s, Avg QP:34.01
";

    /// Real x264 log of a 120 frame encode as produced by
    /// [Patman's mod](https://github.com/Patman86/x264-Mod-by-Patman).
    const PATMAN_X264_CHUNK_0: &str = "\
yuv  [INFO]: 320x240 0:0 @ 30/1 fps (cfr)
yuv  [INFO]: color matrix: undef
x264 [INFO]: AVC Encoder x264 core 165 [Mod by Patman]
x264 [INFO]: built with Clang 21.0.0
x264 [INFO]: using cpu capabilities: none!
x264 [INFO]: profile: High, level: 1.3, subsampling: 4:2:0, bit-depth: 8-bit
x264 [0.8%] 1/120 frames @ 15.64 fps | 1200.0 kb/s | 0:00:00 [-0:00:00] | 5.00 KB [600.0 KB]
x264 112 frames @ 184.3 fps | 298.17 kb/s | 139.1 KB
x264 [100.0%] 120/120 frames @ 292.0 fps | 297.76 kb/s | 0:00:00 [-0:00:00] | 148.9 KB [148.9 KB]
x264 [INFO]: frame I:1     Avg QP:22.65  size:  5000
x264 [INFO]: frame P:38    Avg QP:27.05  size:  1779
x264 [INFO]: frame B:81    Avg QP:32.03  size:   942
x264 [INFO]: Weighted P-Frames: Y:0.0% UV:0.0%
x264 [INFO]: ref P L0: 62.6%  7.6% 18.5% 11.4%
x264 [INFO]: kb/s:297.76

encoded 120 frames, 291.97 fps, 297.76 kb/s, duration 0:00:00.41
";

    /// Log of a 120 frame encode as produced by
    /// [Patman's x265 mod](https://github.com/Patman86/x265-Mod-by-Patman),
    /// which capitalises `Frames`/`FPS` and prints its encoding time as a
    /// clock.
    const PATMAN_X265_CHUNK_0: &str = "\
yuv  [INFO]: 320x240 fps 30000/1000 i420p8 frames 0 - 119 of 120
x265 [INFO]: HEVC encoder version 4.3+1-e9b8812 [Mod by Patman]
x265 [INFO]: frame threads / pool features       : 3 / wpp(4 rows)
[0.8%] 1/120 Frames @ 5.49 FPS | 1435.20 kb/s | 0:00:00 [-0:00:21] | 5.00 KB [600.0 KB]
1 Frames @ 5.49 FPS | 1435.20 kb/s | 5.00 KB
x265 [INFO]: frame I:      1, Avg QP:29.06  kb/s: 856.56  
x265 [INFO]: frame P:     33, Avg QP:30.06  kb/s: 403.17  
x265 [INFO]: frame B:     86, Avg QP:35.59  kb/s: 120.73  
x265 [INFO]: Weighted P-Frames: Y:0.0% UV:0.0%

encoded 120 frames in 00:00:01.389 (86.41 fps), 204.53 kb/s, Avg QP:34.01
";

    fn write_temp_log(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("should write temp log");
        path
    }

    fn merge(dir: &TempDir, encoder: &str, logs: &[(&str, &str)]) -> String {
        let mut merger = LogMerger::new(encoder);
        for (name, contents) in logs {
            let path = write_temp_log(dir, name, contents);
            merger.process_chunk(&path).expect("chunk should parse");
        }

        let mut output = Vec::new();
        merger.write_merged(&mut output).expect("merged log should write");
        String::from_utf8(output).expect("merged log should be utf8")
    }

    #[test]
    fn x264_statistics_are_aligned_like_x264() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk_0 = write_temp_log(&dir, "log_0.log", X264_CHUNK_0);
        let chunk_1 = write_temp_log(
            &dir,
            "log_1.log",
            "yuv [info]: 320x240p 0:0 @ 30/1 fps (cfr)\nx264 [info]: using cpu capabilities: \
             ARMv8 NEON DotProd\nx264 [info]: profile High, level 1.3, 4:2:0, 8-bit\nx264 [info]: \
             frame I:2     Avg QP:24.00  size: 10000\nx264 [info]: frame P:40    Avg QP:26.00  \
             size:  2000\nx264 [info]: frame B:80    Avg QP:30.00  size:  1000\nx264 [info]: \
             kb/s:250.00\nencoded 122 frames, 500.00 fps, 250.00 kb/s\n",
        );

        let mut merger = LogMerger::new("x264");
        merger.process_chunk(&chunk_0).expect("chunk 0 should parse");
        merger.process_chunk(&chunk_1).expect("chunk 1 should parse");

        let mut output = Vec::new();
        merger
            .write_merged_with_summary(&mut output, &EncodeSummaryStats {
                total_frames:   242,
                media_seconds:  8.0,
                encode_seconds: 2.0,
                total_bytes:    300_000,
            })
            .expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        let expected = "\
yuv [info]: 320x240p 0:0 @ 30/1 fps (cfr)
x264 [info]: using cpu capabilities: ARMv8 NEON DotProd
x264 [info]: profile High, level 1.3, 4:2:0, 8-bit

x264 [info]: frame I:3     Avg QP:23.55  size:  8333
x264 [info]: frame P:78    Avg QP:26.51  size:  1892
x264 [info]: frame B:161   Avg QP:31.02  size:   971
x264 [info]: kb/s:300.00

encoded 242 frames, 121.00 fps, 300.00 kb/s
";
        assert_eq!(output, expected);
    }

    #[test]
    fn patman_x264_statistics_are_aligned_like_patman_x264() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk_0 = write_temp_log(&dir, "log_0.log", PATMAN_X264_CHUNK_0);

        let mut merger = LogMerger::new("x264");
        merger.process_chunk(&chunk_0).expect("chunk 0 should parse");

        let mut output = Vec::new();
        merger
            .write_merged_with_summary(&mut output, &EncodeSummaryStats {
                total_frames:   120,
                media_seconds:  4.0,
                encode_seconds: 120.0 / 291.97,
                total_bytes:    148_880,
            })
            .expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        let expected = "\
yuv  [INFO]: 320x240 0:0 @ 30/1 fps (cfr)
yuv  [INFO]: color matrix: undef
x264 [INFO]: AVC Encoder x264 core 165 [Mod by Patman]
x264 [INFO]: built with Clang 21.0.0
x264 [INFO]: using cpu capabilities: none!
x264 [INFO]: profile: High, level: 1.3, subsampling: 4:2:0, bit-depth: 8-bit

x264 [INFO]: frame I:1     Avg QP:22.65  size:  5000
x264 [INFO]: frame P:38    Avg QP:27.05  size:  1779
x264 [INFO]: frame B:81    Avg QP:32.03  size:   942
x264 [INFO]: kb/s:297.76

encoded 120 frames, 291.97 fps, 297.76 kb/s, duration 0:00:00.41
";
        assert_eq!(output, expected);
    }

    #[test]
    fn patman_x265_statistics_are_aligned_like_patman_x265() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk_0 = write_temp_log(&dir, "log_0.log", PATMAN_X265_CHUNK_0);

        let mut merger = LogMerger::new("x265");
        merger.process_chunk(&chunk_0).expect("chunk 0 should parse");

        let mut output = Vec::new();
        merger
            .write_merged_with_summary(&mut output, &EncodeSummaryStats {
                total_frames:   120,
                media_seconds:  4.0,
                encode_seconds: 1.5,
                total_bytes:    102_265,
            })
            .expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        // Patman's x265 prints its encoding time as a clock and capitalises its
        // progress lines, which must not reach the merged log.
        assert!(
            output.contains("x265 [INFO]: frame I:      1, Avg QP:29.06  kb/s: 856.56  \n"),
            "unexpected output:\n{output}"
        );
        assert!(
            output.ends_with(
                "encoded 120 frames in 00:00:01.500 (80.00 fps), 204.53 kb/s, Avg QP:34.01\n"
            ),
            "unexpected output:\n{output}"
        );
        assert!(
            !output.contains("%]"),
            "merged log leaked progress:\n{output}"
        );
        assert!(
            !output.contains("Frames @"),
            "merged log leaked progress:\n{output}"
        );
    }

    #[test]
    fn patman_builds_are_recognised_by_their_log_prefix() {
        // No summary line at all: the upper case log level of Patman's builds
        // is what tells the two variants apart.
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x265", &[(
            "log_0.log",
            "x265 [INFO]: frame I:      1, Avg QP:29.06  kb/s: 856.56  \n",
        )]);

        assert!(
            output.contains("in 00:00:00.000 ("),
            "unexpected output:\n{output}"
        );

        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x265", &[(
            "log_0.log",
            "x265 [info]: frame I:      1, Avg QP:29.06  kb/s: 856.56  \n",
        )]);

        assert!(
            output.contains("in 0.00s ("),
            "unexpected output:\n{output}"
        );
    }

    #[test]
    fn x264_summary_reproduces_the_encoder_bitrate() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x264", &[("log_0.log", X264_CHUNK_0)]);

        assert!(
            output.contains("x264 [info]: kb/s:297.76\n"),
            "unexpected output:\n{output}"
        );
        // The official x264 prints no duration, so neither does the merged log.
        assert!(
            output.ends_with("encoded 120 frames, 752.64 fps, 297.76 kb/s\n"),
            "unexpected output:\n{output}"
        );
    }

    #[test]
    fn x264_merged_log_has_no_progress_or_per_chunk_detail_lines() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x264", &[("log_0.log", X264_CHUNK_0)]);

        for noise in [
            "%]",
            "frames @",
            "eta ",
            "46 frames:",
            "mb I",
            "mb P",
            "mb B",
            "8x8 transform",
            "coded y,uvDC,uvAC",
            "Weighted P-Frames",
            "ref P L0",
            "ref B L0",
            "ref B L1",
            "consecutive B-frames",
        ] {
            assert!(
                !output.contains(noise),
                "merged log leaked `{noise}`:\n{output}"
            );
        }

        // Configuration headers must be printed exactly once.
        assert_eq!(
            output.matches("using cpu capabilities").count(),
            1,
            "{output}"
        );
        assert_eq!(
            output.matches("profile High, level 1.3").count(),
            1,
            "{output}"
        );
        assert_eq!(output.matches("frame I:").count(), 1, "{output}");
    }

    #[test]
    fn x264_frame_size_is_weighted_by_frame_count() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x264", &[
            (
                "log_0.log",
                "x264 [info]: frame I:2     Avg QP:10.00  size:      100\nx264 [info]: encoded 2 \
                 frames, 1.00 fps, 1.60 kb/s\n",
            ),
            (
                "log_1.log",
                "x264 [info]: frame I:1     Avg QP:20.00  size:      200\nx264 [info]: encoded 1 \
                 frames, 1.00 fps, 1.60 kb/s\n",
            ),
        ]);

        assert!(
            output.contains("x264 [info]: frame I:3     Avg QP:13.33  size:   133\n"),
            "unexpected merged output:\n{output}"
        );
    }

    #[test]
    fn headers_are_taken_from_the_first_chunk_log_that_has_them() {
        let dir = TempDir::new().expect("temp dir should exist");
        // Chunk 0 is missing, e.g. because it was encoded by a resumed run.
        let output =
            merge(
                &dir,
                "x264",
                &[
                    (
                        "log_1.log",
                        "x264 [info]: profile High, level 1.3, 4:2:0, 8-bit\nx264 [info]: frame \
                         I:1     Avg QP:22.65  size:  4998\nencoded 1 frames, 752.64 fps, 297.76 \
                         kb/s\n",
                    ),
                    (
                        "log_2.log",
                        "x264 [info]: profile High, level 1.3, 4:2:0, 8-bit\nx264 [info]: frame \
                         P:1     Avg QP:22.65  size:  4998\nencoded 1 frames, 752.64 fps, 297.76 \
                         kb/s\n",
                    ),
                ],
            );

        assert_eq!(output.matches("profile High").count(), 1, "{output}");
        assert!(output.contains("x264 [info]: frame I:1     Avg QP:22.65  size:  4998\n"));
        assert!(output.contains("x264 [info]: frame P:1     Avg QP:22.65  size:  4998\n"));
    }

    #[test]
    fn x265_statistics_are_aligned_like_x265() {
        let dir = TempDir::new().expect("temp dir should exist");
        let chunk_0 = write_temp_log(&dir, "log_0.log", X265_CHUNK_0);

        let mut merger = LogMerger::new("x265");
        merger.process_chunk(&chunk_0).expect("chunk 0 should parse");

        let mut output = Vec::new();
        merger
            .write_merged_with_summary(&mut output, &EncodeSummaryStats {
                total_frames:   120,
                media_seconds:  4.0,
                encode_seconds: 120.0 / 86.41,
                total_bytes:    102_265,
            })
            .expect("merged log should write");
        let output = String::from_utf8(output).expect("merged log should be utf8");

        // x265 pads the kb/s column to eight characters, exactly like
        // `%-8.2lf`.
        assert!(
            output.contains("x265 [info]: frame I:      1, Avg QP:29.06  kb/s: 856.56  \n"),
            "unexpected output:\n{output}"
        );
        assert!(
            output.contains("x265 [info]: frame P:     33, Avg QP:30.06  kb/s: 403.17  \n"),
            "unexpected output:\n{output}"
        );
        assert!(
            output.contains("x265 [info]: frame B:     86, Avg QP:35.59  kb/s: 120.73  \n"),
            "unexpected output:\n{output}"
        );
        assert!(
            output
                .ends_with("encoded 120 frames in 1.39s (86.41 fps), 204.53 kb/s, Avg QP:34.01\n"),
            "unexpected output:\n{output}"
        );
        assert!(
            !output.contains("%]"),
            "merged log leaked progress:\n{output}"
        );
        assert!(
            !output.contains("Weighted P-Frames"),
            "unexpected output:\n{output}"
        );
    }

    #[test]
    fn x265_headers_are_kept_verbatim() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x265", &[("log_0.log", X265_CHUNK_0)]);

        assert!(output.starts_with("yuv  [info]: 320x240 fps"), "{output}");
        assert!(
            output.contains("x265 [info]: frame threads / pool features       : 3 / wpp(4 rows)\n")
        );
        assert!(
            output.contains("x265 [warning]: Source height < 720p; disabling lookahead-slices\n")
        );
        assert_eq!(
            output.matches("HEVC encoder version").count(),
            1,
            "{output}"
        );
    }

    #[test]
    fn progress_lines_are_not_captured_but_statistics_are() {
        for progress in [
            "[42.5%] 121/285 frames, 2.47 fps, 1445.28 kb/s, eta 0:01:06",
            "x264 [38.2%] 844/2208 frames @ 115.7 fps | 2930.1 kb/s | 0:00:07 [-0:00:11] | 12.89 \
             MB [33.73 MB]",
            "121 frames: 2.47 fps, 1445.28 kb/s",
            "x264 112 frames @ 184.3 fps | 298.17 kb/s | 139.1 KB",
            "[38.2%] 844/2208 Frames @ 115.7 FPS | 2930.1 kb/s | 0:00:07 [-0:00:11] | 12.89 MB \
             [33.73 MB]",
            "844 Frames @ 115.7 FPS | 2930.1 kb/s | 12.89 MB",
            "frame= 100 fps=25 q=28.0 time=00:00:04.00 bitrate=1234.5kbits/s",
            "   ",
        ] {
            assert!(
                is_encoder_progress_line(progress),
                "`{progress}` is progress"
            );
            assert!(
                !should_capture_chunk_log_line(progress),
                "`{progress}` must not be captured"
            );
        }

        for statistics in [
            "x264 [info]: frame I:1     Avg QP:22.65  size:  4998",
            "x264 [info]: Weighted P-Frames: Y:0.0% UV:0.0%",
            "x264 [info]: consecutive B-frames:  5.8%  8.3% 12.5% 73.3%",
            "x264 [info]: ref B L1: 94.7%  5.3%",
            "x264 [info]: kb/s:297.76",
            "encoded 120 frames, 752.64 fps, 297.76 kb/s",
            "x265 [info]: frame I:      1, Avg QP:29.06  kb/s: 856.56",
            "x265 [info]: Weighted B-Frames: Y:0.0% UV:0.0%",
            "x265 [info]: Lookahead / bframes / badapt        : 20 / 4 / 2",
            "x265 [info]: frame threads / pool features       : 3 / wpp(4 rows)",
            "yuv  [info]: 320x240 fps 30000/1000 i420p8 frames 0 - 119 of 120",
            "x265 [INFO]: frame I:      1, Avg QP:29.06  kb/s: 856.56  ",
            "x265 [INFO]: Weighted P-Frames: Y:0.0% UV:0.0%",
            "x264 [INFO]: kb/s:297.76",
            "encoded 120 frames in 00:00:01.389 (86.41 fps), 204.53 kb/s, Avg QP:34.01",
        ] {
            assert!(
                !is_encoder_progress_line(statistics),
                "`{statistics}` is not progress"
            );
            assert!(
                should_capture_chunk_log_line(statistics),
                "`{statistics}` must be captured"
            );
        }
    }

    #[test]
    fn summary_lines_are_aggregated_across_chunks() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x265", &[
            (
                "log_0.log",
                "x265 [info]: frame I:      1, Avg QP:29.06  kb/s: 856.56  \nx265 [info]: frame \
                 P:     20, Avg QP:30.06  kb/s: 403.17  \nx265 [info]: frame B:     99, Avg \
                 QP:35.59  kb/s: 120.73  \nencoded 120 frames in 1.39s (86.41 fps), 204.53 kb/s, \
                 Avg QP:34.01\n",
            ),
            (
                "log_1.log",
                "x265 [info]: frame I:      3, Avg QP:31.06  kb/s: 1000.00  \nx265 [info]: frame \
                 P:     30, Avg QP:32.06  kb/s: 500.00  \nx265 [info]: frame B:     47, Avg \
                 QP:36.00  kb/s: 100.00  \nencoded 80 frames in 0.61s (131.15 fps), 300.00 kb/s, \
                 Avg QP:30.00\n",
            ),
        ]);

        // (1 * 856.56 + 3 * 1000.00) / 4 frames
        assert!(
            output.contains("x265 [info]: frame I:      4, Avg QP:30.56  kb/s: 964.14  \n"),
            "unexpected merged output:\n{output}"
        );
        assert!(
            output
                .ends_with("encoded 200 frames in 2.00s (100.00 fps), 217.86 kb/s, Avg QP:34.50\n"),
            "unexpected merged output:\n{output}"
        );
    }

    #[test]
    fn merger_without_any_statistics_only_writes_the_headers() {
        let dir = TempDir::new().expect("temp dir should exist");
        let output = merge(&dir, "x264", &[(
            "log_0.log",
            "x264 [info]: using cpu capabilities: ARMv8 NEON DotProd\n",
        )]);

        assert!(output.contains("using cpu capabilities"), "{output}");
        assert!(!output.contains("frame I:"), "{output}");
    }
}
