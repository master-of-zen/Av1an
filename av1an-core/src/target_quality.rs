use std::{
    cmp,
    cmp::Ordering,
    convert::TryInto,
    fmt::Error,
    path::{Path, PathBuf},
    process::Stdio,
    thread::available_parallelism,
};

use ffmpeg::format::Pixel;
use serde::{Deserialize, Serialize};
use splines::{Interpolation, Key, Spline};
use tracing::{debug, error};

use crate::{
    broker::EncoderCrash,
    chunk::Chunk,
    metrics::{
        butteraugli::ButteraugliSubMetric,
        ssimulacra2::MetricStatistics,
        vmaf::{self, read_weighted_vmaf},
        xpsnr::{self, read_weighted_xpsnr, XPSNRSubMetric},
    },
    progress_bar::update_mp_msg,
    vapoursynth::{measure_butteraugli, measure_ssimulacra2, measure_xpsnr},
    Encoder,
    TargetMetric,
};

const METRIC_PERCENTILE: f64 = 0.01;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetQuality {
    pub vmaf_res:      String,
    pub vmaf_scaler:   String,
    pub vmaf_filter:   Option<String>,
    pub vmaf_threads:  usize,
    pub model:         Option<PathBuf>,
    pub probe_res:     Option<String>,
    pub probing_rate:  usize,
    pub probing_speed: Option<u8>,
    pub probes:        u32,
    pub target:        f64,
    pub metric:        TargetMetric,
    pub min_q:         u32,
    pub max_q:         u32,
    pub encoder:       Encoder,
    pub pix_format:    Pixel,
    pub temp:          String,
    pub workers:       usize,
    pub video_params:  Vec<String>,
    pub vspipe_args:   Vec<String>,
    pub probe_slow:    bool,
}

impl TargetQuality {
    fn per_shot_target_quality(
        &self,
        chunk: &Chunk,
        worker_id: Option<usize>,
    ) -> Result<u32, Box<EncoderCrash>> {
        let mut score_quality = vec![];
        let frames = chunk.frames();

        // Make middle probe
        let middle_point = (self.min_q + self.max_q) / 2;
        let last_q = middle_point;

        let update_progress_bar = |last_q: u32| {
            if let Some(worker_id) = worker_id {
                update_mp_msg(
                    worker_id,
                    format!(
                        "Targeting {} Quality {} - Testing {}",
                        self.metric, self.target, last_q
                    ),
                );
            }
        };

        update_progress_bar(last_q);

        fn measure_probe(
            tq: &TargetQuality,
            chunk: &Chunk,
            metric: TargetMetric,
            target: usize,
        ) -> f64 {
            match metric {
                TargetMetric::VMAF => {
                    let fl_path = tq.vmaf_probe(chunk, target).unwrap();
                    read_weighted_vmaf(fl_path, METRIC_PERCENTILE).unwrap()
                },
                TargetMetric::SSIMULACRA2 => {
                    let scores = tq.ssimulacra2_probe(chunk, target).unwrap();
                    MetricStatistics::new(scores).average()
                },
                TargetMetric::ButteraugliInf | TargetMetric::BUTTERAUGLI3 => {
                    let scores = tq
                        .butteraugli_probe(
                            chunk,
                            target,
                            if tq.metric == TargetMetric::ButteraugliInf {
                                ButteraugliSubMetric::InfiniteNorm
                            } else {
                                ButteraugliSubMetric::ThreeNorm
                            },
                        )
                        .unwrap();
                    MetricStatistics::new(scores).average()
                },
                TargetMetric::XPSNR | TargetMetric::XPSNRWeighted => {
                    let submetric = if tq.metric == TargetMetric::XPSNR {
                        XPSNRSubMetric::Minimum
                    } else {
                        XPSNRSubMetric::Weighted
                    };
                    if tq.probing_rate > 1 {
                        let scores = tq.xpsnr_vs_probe(chunk, target, submetric).unwrap();
                        MetricStatistics::new(scores).percentile(1)
                    } else {
                        let fl_path = tq.xpsnr_probe(chunk, target).unwrap();
                        read_weighted_xpsnr(fl_path, METRIC_PERCENTILE, submetric).unwrap()
                    }
                },
            }
        }

        let mut score = measure_probe(self, chunk, self.metric, last_q as usize);
        if self.metric == TargetMetric::ButteraugliInf {
            // Butteraugli is inverted, where quality lowers as score increases
            score *= -1.0;
        }
        let target = if self.metric == TargetMetric::ButteraugliInf {
            -self.target
        } else {
            self.target
        };
        score_quality.push((score, last_q));

        // Initialize search boundary
        let mut score_lower = score;
        let mut score_upper = score;
        let mut cq_lower = last_q;
        let mut cq_upper = last_q;

        // Branch
        let next_q = if score < target {
            self.min_q
        } else {
            self.max_q
        };
        update_progress_bar(next_q);

        // Edge case check
        score = measure_probe(self, chunk, self.metric, next_q as usize);
        if self.metric == TargetMetric::ButteraugliInf {
            // Invert butteraugli score for comparison
            score *= -1.0;
        }
        score_quality.push((score, next_q));

        if (next_q == self.min_q && score < target) || (next_q == self.max_q && score > target) {
            // Invert butteraugli scores back to positive for logging
            if self.metric == TargetMetric::ButteraugliInf {
                for (score, _) in score_quality.iter_mut() {
                    *score *= -1.0;
                }
            }

            log_probes(
                &mut score_quality,
                frames as u32,
                self.probing_rate as u32,
                &chunk.name(),
                next_q,
                match self.metric {
                    TargetMetric::ButteraugliInf => -score,
                    _ => score,
                },
                if score < target {
                    Skip::Low
                } else {
                    Skip::High
                },
            );
            return Ok(next_q);
        }

        // Set boundary
        if score < target {
            score_lower = score;
            cq_lower = next_q;
        } else {
            score_upper = score;
            cq_upper = next_q;
        }

        // VMAF search
        for _ in 0..self.probes - 2 {
            let new_point = weighted_search(
                f64::from(cq_lower),
                score_lower,
                f64::from(cq_upper),
                score_upper,
                target,
            );

            if score_quality.iter().map(|(_, x)| *x).any(|x| x == new_point as u32) {
                break;
            }

            update_progress_bar(new_point as u32);

            score = measure_probe(self, chunk, self.metric, new_point as usize);
            if self.metric == TargetMetric::ButteraugliInf {
                // Invert butteraugli score for comparison
                score *= -1.0;
            }
            score_quality.push((score, new_point as u32));

            // Update boundary
            if score < target {
                score_lower = score;
                cq_lower = new_point as u32;
            } else {
                score_upper = score;
                cq_upper = new_point as u32;
            }
        }

        let (q, q_score) = interpolated_target_q(score_quality.clone(), target);

        // Invert butteraugli scores back to positive for logging
        if self.metric == TargetMetric::ButteraugliInf {
            for (score, _) in score_quality.iter_mut() {
                *score *= -1.0;
            }
        }
        log_probes(
            &mut score_quality,
            frames as u32,
            self.probing_rate as u32,
            &chunk.name(),
            q as u32,
            match self.metric {
                TargetMetric::ButteraugliInf => -q_score,
                _ => q_score,
            },
            Skip::None,
        );

        Ok(q as u32)
    }

    fn probe(&self, chunk: &Chunk, q: usize) -> Result<PathBuf, Box<EncoderCrash>> {
        let vmaf_threads = if self.vmaf_threads == 0 {
            vmaf_auto_threads(self.workers)
        } else {
            self.vmaf_threads
        };

        let cmd = self.encoder.probe_cmd(
            self.temp.clone(),
            chunk.index,
            q,
            self.pix_format,
            self.probing_rate,
            self.probing_speed,
            vmaf_threads,
            self.video_params.clone(),
            self.probe_slow,
        );

        let future = async {
            let mut source = if let [pipe_cmd, args @ ..] = &*chunk.source_cmd {
                tokio::process::Command::new(pipe_cmd)
                    .args(args)
                    .stderr(if cfg!(windows) {
                        Stdio::null()
                    } else {
                        Stdio::piped()
                    })
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_stdout: Stdio = source.stdout.take().unwrap().try_into().unwrap();

            let mut source_pipe = if let [ffmpeg, args @ ..] = &*cmd.0 {
                tokio::process::Command::new(ffmpeg)
                    .args(args)
                    .stdin(source_pipe_stdout)
                    .stdout(Stdio::piped())
                    .stderr(if cfg!(windows) {
                        Stdio::null()
                    } else {
                        Stdio::piped()
                    })
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_stdout: Stdio = source_pipe.stdout.take().unwrap().try_into().unwrap();

            let enc_pipe = if let [cmd, args @ ..] = &*cmd.1 {
                tokio::process::Command::new(cmd.as_ref())
                    .args(args.iter().map(AsRef::as_ref))
                    .stdin(source_pipe_stdout)
                    .stdout(Stdio::piped())
                    .stderr(if cfg!(windows) {
                        Stdio::null()
                    } else {
                        Stdio::piped()
                    })
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_output = source_pipe.wait_with_output().await.unwrap();

            // TODO: Expand EncoderCrash to handle io errors as well
            let enc_output = enc_pipe.wait_with_output().await.unwrap();

            if !enc_output.status.success() {
                let e = EncoderCrash {
                    exit_status:        enc_output.status,
                    stdout:             enc_output.stdout.into(),
                    stderr:             enc_output.stderr.into(),
                    source_pipe_stderr: source_pipe_output.stderr.into(),
                    ffmpeg_pipe_stderr: None,
                };
                error!("[chunk {}] {}", chunk.index, e);
                return Err(e);
            }

            Ok(())
        };

        let rt = tokio::runtime::Builder::new_current_thread().enable_io().build().unwrap();

        rt.block_on(future)?;

        let probe_name =
            Path::new(&chunk.temp).join("split").join(format!("v_{q}_{}.ivf", chunk.index));

        Ok(probe_name)
    }

    fn vmaf_probe(&self, chunk: &Chunk, q: usize) -> Result<PathBuf, Box<EncoderCrash>> {
        let probe_name = self.probe(chunk, q)?;
        let fl_path = Path::new(&chunk.temp).join("split").join(format!("{}.json", chunk.index));

        vmaf::run_vmaf(
            &probe_name,
            chunk.source_cmd.as_slice(),
            self.vspipe_args.clone(),
            &fl_path,
            self.model.as_ref(),
            self.probe_res.as_ref().unwrap_or(&self.vmaf_res),
            &self.vmaf_scaler,
            self.probing_rate,
            self.vmaf_filter.as_deref(),
            self.vmaf_threads,
        )?;

        Ok(fl_path)
    }
    fn ssimulacra2_probe(&self, chunk: &Chunk, q: usize) -> Result<Vec<f64>, Box<EncoderCrash>> {
        let probe_name = self.probe(chunk, q)?;

        let scores = measure_ssimulacra2(
            &chunk.input,
            &probe_name,
            (chunk.start_frame as u32, chunk.end_frame as u32),
            self.probe_res.as_ref(),
            self.probing_rate,
        )
        .unwrap();

        Ok(scores)
    }

    fn butteraugli_probe(
        &self,
        chunk: &Chunk,
        q: usize,
        submetric: ButteraugliSubMetric,
    ) -> Result<Vec<f64>, Box<EncoderCrash>> {
        let probe_name = self.probe(chunk, q)?;

        let scores = measure_butteraugli(
            submetric,
            &chunk.input,
            &probe_name,
            (chunk.start_frame as u32, chunk.end_frame as u32),
            self.probe_res.as_ref(),
            self.probing_rate,
        )
        .unwrap();

        Ok(scores)
    }

    fn xpsnr_probe(&self, chunk: &Chunk, q: usize) -> Result<PathBuf, Box<EncoderCrash>> {
        let probe_name = self.probe(chunk, q)?;
        let fl_path = Path::new(&chunk.temp).join("split").join(format!("{}.json", chunk.index));

        xpsnr::run_xpsnr(
            &probe_name,
            chunk.source_cmd.as_slice(),
            self.vspipe_args.clone(),
            &fl_path,
            self.probe_res.as_ref().unwrap_or(&self.vmaf_res),
            &self.vmaf_scaler,
            self.probing_rate,
        )?;

        Ok(fl_path)
    }

    fn xpsnr_vs_probe(
        &self,
        chunk: &Chunk,
        q: usize,
        submetric: XPSNRSubMetric,
    ) -> Result<Vec<f64>, Box<EncoderCrash>> {
        let probe_name = self.probe(chunk, q)?;

        let scores = measure_xpsnr(
            submetric,
            &chunk.input,
            &probe_name,
            (chunk.start_frame as u32, chunk.end_frame as u32),
            self.probe_res.as_ref(),
            self.probing_rate,
        )
        .unwrap();

        Ok(scores)
    }

    #[inline]
    pub fn per_shot_target_quality_routine(
        &self,
        chunk: &mut Chunk,
        worker_id: Option<usize>,
    ) -> Result<(), Box<EncoderCrash>> {
        chunk.tq_cq = Some(self.per_shot_target_quality(chunk, worker_id)?);
        Ok(())
    }
}

pub fn weighted_search(num1: f64, vmaf1: f64, num2: f64, vmaf2: f64, target: f64) -> usize {
    let dif1 = (transform_vmaf(target) - transform_vmaf(vmaf2)).abs();
    let dif2 = (transform_vmaf(target) - transform_vmaf(vmaf1)).abs();

    let tot = dif1 + dif2;

    num1.mul_add(dif1 / tot, num2 * (dif2 / tot)).round() as usize
}

pub fn transform_vmaf(vmaf: f64) -> f64 {
    let x: f64 = 1.0 - vmaf / 100.0;
    if vmaf < 99.99 {
        -x.ln()
    } else {
        9.2
    }
}

/// Returns auto detected amount of threads used for vmaf calculation
pub fn vmaf_auto_threads(workers: usize) -> usize {
    const OVER_PROVISION_FACTOR: f64 = 1.25;

    // Logical CPUs
    let threads = available_parallelism()
        .expect("Unrecoverable: Failed to get thread count")
        .get();

    cmp::max(
        ((threads / workers) as f64 * OVER_PROVISION_FACTOR) as usize,
        1,
    )
}

/// Use linear interpolation to get q/crf values closest to the target value
pub fn interpolate_target_q(scores: Vec<(f64, u32)>, target: f64) -> Result<f64, Error> {
    let mut sorted = scores;
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let keys = sorted
        .iter()
        .map(|(x, y)| Key::new(*x, f64::from(*y), Interpolation::Linear))
        .collect();

    let spline = Spline::from_vec(keys);

    Ok(spline.sample(target).unwrap())
}

/// Use linear interpolation to get vmaf value that expected from q
pub fn interpolate_target_vmaf(scores: Vec<(f64, u32)>, q: f64) -> Result<f64, Error> {
    let mut sorted = scores;
    sorted.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(Ordering::Less));

    let keys = sorted
        .iter()
        .map(|f| Key::new(f64::from(f.1), f.0, Interpolation::Linear))
        .collect();

    let spline = Spline::from_vec(keys);

    Ok(spline.sample(q).unwrap())
}

#[derive(Copy, Clone)]
pub enum Skip {
    High,
    Low,
    None,
}

pub fn log_probes(
    cq_scores: &mut [(f64, u32)],
    frames: u32,
    probing_rate: u32,
    chunk_idx: &str,
    target_q: u32,
    target_score: f64,
    skip: Skip,
) {
    cq_scores.sort_by_key(|(_score, q)| *q);

    // TODO: take chunk id as integer instead and format with {:05}
    debug!(
        "chunk {}: P-Rate={}, {} frames
    TQ-Probes: {:.2?}{}
    Target Q={:.0}, Score={:.2}",
        chunk_idx,
        probing_rate,
        frames,
        cq_scores,
        match skip {
            Skip::High => " Early Skip High Q",
            Skip::Low => " Early Skip Low Q",
            Skip::None => "",
        },
        target_q,
        target_score
    );
}

#[inline]
pub const fn adapt_probing_rate(rate: usize) -> usize {
    match rate {
        1..=4 => rate,
        _ => 1,
    }
}

pub fn interpolated_target_q(scores: Vec<(f64, u32)>, target: f64) -> (f64, f64) {
    let q = interpolate_target_q(scores.clone(), target).unwrap();

    let vmaf = interpolate_target_vmaf(scores, q).unwrap();

    (q, vmaf)
}

#[allow(unused)]
fn lagrange_interpolate(p: &[(u32, f64)], x: u32) -> f64 {
    p.iter()
        .map(|(x0, y0)| {
            let mut num = 1;
            let mut den = 1;
            for (x1, _y1) in p {
                if x0 != x1 {
                    num *= i64::from(x) - i64::from(*x1);
                    den *= i64::from(*x0) - i64::from(*x1);
                }
            }
            y0 * num as f64 / den as f64
        })
        .sum()
}

#[allow(unused)]
fn lagrange_bisect(p: &[(u32, f64)], y: f64) -> (u32, f64) {
    assert!(p.len() >= 2);

    // Re-center the samples at the target value
    let mut sorted = Vec::from(p);
    for v in &mut sorted {
        v.1 -= y;
    }

    // Order samples by distance from target value
    sorted.sort_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap());

    // Take the closest point
    let (mut xb, mut yb) = sorted[0];
    // Take the next close point that brackets the root
    let (mut xa, mut ya) = sorted.iter().find(|&&v| v.1 * yb < 0.).unwrap_or(&(xb, yb));

    loop {
        let x0 = (xa + xb).div_ceil(2);
        if x0 == xb || x0 == xa {
            break;
        }

        let y0 = lagrange_interpolate(&sorted, x0);
        if ya * y0 < 0. {
            xb = x0;
            yb = y0;
        } else {
            xa = x0;
            ya = y0;
        }
        if ya.abs() < yb.abs() {
            std::mem::swap(&mut xa, &mut xb);
            std::mem::swap(&mut ya, &mut yb);
        }
    }

    (xb, yb + y)
}

#[cfg(test)]
mod tests {
    use crate::target_quality::lagrange_bisect;

    #[test]
    fn test_bisect() {
        let sorted = vec![(0, 0.0), (1, 1.0), (256, 256.0 * 256.0)];

        assert!(lagrange_bisect(&sorted, 0.0).0 == 0);
        assert!(lagrange_bisect(&sorted, 1.0).0 == 1);
        assert!(lagrange_bisect(&sorted, 256.0 * 256.0).0 == 256);

        assert!(lagrange_bisect(&sorted, 8.0).0 == 3);
        assert!(lagrange_bisect(&sorted, 9.0).0 == 3);

        assert!(lagrange_bisect(&sorted, -1.0).0 == 0);
        assert!(lagrange_bisect(&sorted, 2.0 * 256.0 * 256.0).0 == 256);
    }
}
