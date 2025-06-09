#[cfg(test)]
mod tests;

use std::{cmp, cmp::Ordering, convert::TryInto, path::PathBuf, thread::available_parallelism};

use ffmpeg::format::Pixel;
use serde::{Deserialize, Serialize};
use splines::{Interpolation, Key, Spline};
use tracing::debug;

use crate::{
    broker::EncoderCrash,
    chunk::Chunk,
    progress_bar::update_mp_msg,
    vmaf::{read_weighted_vmaf, read_weighted_vmaf_alt},
    Encoder,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetQuality {
    pub vmaf_res:      String,
    pub vmaf_scaler:   String,
    pub vmaf_filter:   Option<String>,
    pub vmaf_threads:  usize,
    pub model:         Option<PathBuf>,
    pub probing_rate:  usize,
    pub probing_speed: Option<u8>,
    pub probes:        u32,
    pub target:        f64,
    pub min_q:         u32,
    pub max_q:         u32,
    pub encoder:       Encoder,
    pub pix_format:    Pixel,
    pub temp:          String,
    pub workers:       usize,
    pub video_params:  Vec<String>,
    pub vspipe_args:   Vec<String>,
    pub probe_slow:    bool,
    pub alt_vmaf:      bool,
    pub percentile:    f64,
}

impl TargetQuality {
    fn per_shot_target_quality(
        &self,
        chunk: &Chunk,
        worker_id: Option<usize>,
    ) -> Result<u32, Box<EncoderCrash>> {
        let mut history: Vec<(u32, f64, PathBuf)> = vec![];

        let update_progress_bar = |last_q: u32| {
            if let Some(worker_id) = worker_id {
                update_mp_msg(
                    worker_id,
                    format!("Targeting Quality {} - Testing {}", self.target, last_q),
                );
            }
        };

        let mut low = self.min_q;
        let mut high = self.max_q;

        loop {
            let predicted_q = predict_crf(low, high, &history, self.target);

            if history.iter().any(|(q, _, _)| *q == predicted_q) {
                break;
            }

            update_progress_bar(predicted_q);

            let probe_path = self.vmaf_probe(chunk, predicted_q as usize)?;
            let score = if self.alt_vmaf {
                read_weighted_vmaf_alt(&probe_path, self.percentile, chunk.frame_rate).unwrap()
            } else {
                read_weighted_vmaf(&probe_path, self.percentile).unwrap()
            };

            history.push((predicted_q, score, probe_path.clone()));

            if within_tolerance(score, self.target) || history.len() >= self.probes as usize {
                break;
            }

            if score > self.target {
                low = (predicted_q + 1).min(high);
            } else {
                high = (predicted_q - 1).max(low);
            }

            if low > high {
                break;
            }
        }

        let good_results: Vec<&(u32, f64, PathBuf)> = history
            .iter()
            .filter(|(_, score, _)| within_tolerance(*score, self.target))
            .collect();

        debug!(
            "Good results: {:?}",
            good_results.iter().map(|(q, s, _)| (*q, *s)).collect::<Vec<_>>()
        );

        let best_result = if !good_results.is_empty() {
            good_results.iter().max_by_key(|(q, _, _)| *q).unwrap()
        } else {
            history
                .iter()
                .min_by(|(_, s1, _), (_, s2, _)| {
                    let d1 = (s1 - self.target).abs();
                    let d2 = (s2 - self.target).abs();
                    d1.partial_cmp(&d2).unwrap_or(Ordering::Equal)
                })
                .unwrap()
        };

        debug!(
            "Best result: Q={}, VMAF={:.2}",
            best_result.0, best_result.1
        );

        let mut vmaf_cq: Vec<(f64, u32)> = history.iter().map(|(q, s, _)| (*s, *q)).collect();
        log_probes(
            &mut vmaf_cq,
            chunk.frames() as u32,
            self.probing_rate as u32,
            &chunk.name(),
            best_result.0,
            best_result.1,
            Skip::None,
        );

        Ok(best_result.0)
    }

    fn vmaf_probe(&self, chunk: &Chunk, q: usize) -> Result<PathBuf, Box<EncoderCrash>> {
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
                        std::process::Stdio::null()
                    } else {
                        std::process::Stdio::piped()
                    })
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_stdout: std::process::Stdio =
                source.stdout.take().unwrap().try_into().unwrap();

            let mut source_pipe = if let [ffmpeg, args @ ..] = &*cmd.0 {
                tokio::process::Command::new(ffmpeg)
                    .args(args)
                    .stdin(source_pipe_stdout)
                    .stdout(std::process::Stdio::piped())
                    .stderr(if cfg!(windows) {
                        std::process::Stdio::null()
                    } else {
                        std::process::Stdio::piped()
                    })
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_stdout: std::process::Stdio =
                source_pipe.stdout.take().unwrap().try_into().unwrap();

            let enc_pipe = if let [cmd, args @ ..] = &*cmd.1 {
                tokio::process::Command::new(cmd.as_ref())
                    .args(args.iter().map(AsRef::as_ref))
                    .stdin(source_pipe_stdout)
                    .stdout(std::process::Stdio::piped())
                    .stderr(if cfg!(windows) {
                        std::process::Stdio::null()
                    } else {
                        std::process::Stdio::piped()
                    })
                    .spawn()
                    .unwrap()
            } else {
                unreachable!()
            };

            let source_pipe_output = source_pipe.wait_with_output().await.unwrap();
            let enc_output = enc_pipe.wait_with_output().await.unwrap();

            if !enc_output.status.success() {
                let e = EncoderCrash {
                    exit_status:        enc_output.status,
                    stdout:             enc_output.stdout.into(),
                    stderr:             enc_output.stderr.into(),
                    source_pipe_stderr: source_pipe_output.stderr.into(),
                    ffmpeg_pipe_stderr: None,
                };
                return Err(e);
            }

            Ok(())
        };

        let rt = tokio::runtime::Builder::new_current_thread().enable_io().build().unwrap();
        rt.block_on(future)?;

        let probe_name = std::path::Path::new(&chunk.temp)
            .join("split")
            .join(format!("v_{q}_{}.ivf", chunk.index));
        let fl_path = std::path::Path::new(&chunk.temp)
            .join("split")
            .join(format!("{}.json", chunk.index));

        if self.alt_vmaf {
            crate::vmaf::run_vmaf_alt(
                &probe_name,
                chunk.source_cmd.as_slice(),
                self.vspipe_args.clone(),
                &fl_path,
                self.model.as_ref(),
                &self.vmaf_res,
                &self.vmaf_scaler,
                self.probing_rate,
                self.vmaf_filter.as_deref(),
                self.vmaf_threads,
                chunk.frame_rate,
            )?;
        } else {
            crate::vmaf::run_vmaf(
                &probe_name,
                chunk.source_cmd.as_slice(),
                self.vspipe_args.clone(),
                &fl_path,
                self.model.as_ref(),
                &self.vmaf_res,
                &self.vmaf_scaler,
                self.probing_rate,
                self.vmaf_filter.as_deref(),
                self.vmaf_threads,
            )?;
        }

        Ok(fl_path)
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

fn predict_crf(low: u32, high: u32, history: &[(u32, f64, PathBuf)], target: f64) -> u32 {
    let mut sorted_history = history.to_vec();
    sorted_history.sort_by_key(|(crf, _, _)| *crf);

    let mut crf_score_map: Vec<(u32, f64)> =
        sorted_history.iter().map(|(crf, score, _)| (*crf, *score)).collect();
    crf_score_map.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

    if crf_score_map.len() >= 3 {
        let (scores, crfs): (Vec<f64>, Vec<f64>) =
            crf_score_map.iter().map(|(crf, score)| (*score, *crf as f64)).unzip();

        let keys: Vec<Key<f64, f64>> = scores
            .iter()
            .zip(crfs.iter())
            .map(|(score, crf)| Key::new(*score, *crf, Interpolation::CatmullRom))
            .collect();

        let spline = Spline::from_vec(keys);
        if let Some(predicted) = spline.sample(target) {
            return (predicted.round() as u32).clamp(low, high);
        }
    }

    if crf_score_map.len() == 2 {
        let score_crf_pairs: Vec<(f64, u32)> =
            crf_score_map.iter().map(|(crf, score)| (*score, *crf)).collect();

        let (score1, crf1) = score_crf_pairs[0];
        let (score2, crf2) = score_crf_pairs[1];

        if score1 == score2 {
            return ((crf1 + crf2) / 2).clamp(low, high);
        }

        let slope = (crf2 as f64 - crf1 as f64) / (score2 - score1);
        let predicted = crf1 as f64 + slope * (target - score1);
        return (predicted.round() as u32).clamp(low, high);
    }

    (low + high) / 2
}

fn within_tolerance(score: f64, target: f64) -> bool {
    (score - target).abs() / target < 0.01
}

pub fn vmaf_auto_threads(workers: usize) -> usize {
    const OVER_PROVISION_FACTOR: f64 = 1.25;

    let threads = available_parallelism()
        .expect("Unrecoverable: Failed to get thread count")
        .get();

    cmp::max(
        ((threads / workers) as f64 * OVER_PROVISION_FACTOR) as usize,
        1,
    )
}

#[derive(Copy, Clone)]
pub enum Skip {
    None,
}

pub fn log_probes(
    vmaf_cq_scores: &mut [(f64, u32)],
    frames: u32,
    probing_rate: u32,
    chunk_idx: &str,
    target_q: u32,
    target_vmaf: f64,
    skip: Skip,
) {
    vmaf_cq_scores.sort_by_key(|(_score, q)| *q);

    debug!(
        "chunk {}: P-Rate={}, {} frames",
        chunk_idx, probing_rate, frames
    );
    debug!(
        "chunk {}: TQ-Probes: {:.2?}{}",
        chunk_idx,
        vmaf_cq_scores,
        match skip {
            Skip::None => "",
        }
    );
    debug!(
        "chunk {}: Target Q={:.0}, VMAF={:.2}",
        chunk_idx, target_q, target_vmaf
    );
}

#[inline]
pub const fn adapt_probing_rate(rate: usize) -> usize {
    match rate {
        1..=4 => rate,
        _ => 1,
    }
}
