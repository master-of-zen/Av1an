use std::{cmp, cmp::Ordering, collections::HashSet, path::PathBuf, thread::available_parallelism};

use clap::ValueEnum;
use ffmpeg::format::Pixel;
use serde::{Deserialize, Serialize};
use splines::{Interpolation, Key, Spline};
use tracing::{debug, trace};

use crate::{
    broker::EncoderCrash,
    chunk::Chunk,
    progress_bar::update_mp_msg,
    vmaf::read_weighted_vmaf,
    Encoder,
    ProbingSpeed,
    ProbingStatistic,
};

const SCORE_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetQuality {
    pub vmaf_res:              String,
    pub vmaf_scaler:           String,
    pub vmaf_filter:           Option<String>,
    pub vmaf_threads:          usize,
    pub model:                 Option<PathBuf>,
    pub probing_rate:          usize,
    pub probing_speed:         Option<u8>,
    pub probes:                u32,
    pub target:                f64,
    pub min_q:                 u32,
    pub max_q:                 u32,
    pub encoder:               Encoder,
    pub pix_format:            Pixel,
    pub temp:                  String,
    pub workers:               usize,
    pub video_params:          Vec<String>,
    pub vspipe_args:           Vec<String>,
    pub probe_slow:            bool,
    pub probing_vmaf_features: Vec<VmafFeature>,
    pub probing_statistic:     ProbingStatistic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
pub enum VmafFeature {
    #[value(name = "default")]
    Default,
    #[value(name = "weighted")]
    Weighted,
    #[value(name = "neg")]
    Neg,
    #[value(name = "motionless")]
    Motionless,
    #[value(name = "uhd")]
    Uhd,
}

impl TargetQuality {
    fn per_shot_target_quality(
        &self,
        chunk: &Chunk,
        worker_id: Option<usize>,
    ) -> anyhow::Result<u32> {
        // History of probe results as quantizer-score pairs
        let mut quantizer_score_history: Vec<(u32, f64)> = vec![];

        let update_progress_bar = |last_q: u32| {
            if let Some(worker_id) = worker_id {
                update_mp_msg(
                    worker_id,
                    format!(
                        "Targeting Quality {target} - Testing {last_q}",
                        target = self.target
                    ),
                );
            }
        };

        // Initialize quantizer limits from specified minimum and maximum quantizers
        let mut lower_quantizer_limit = self.min_q;
        let mut upper_quantizer_limit = self.max_q;

        loop {
            let next_quantizer = predict_quantizer(
                lower_quantizer_limit,
                upper_quantizer_limit,
                &quantizer_score_history,
                self.target,
            );

            if let Some((quantizer, score)) = quantizer_score_history
                .iter()
                .find(|(quantizer, _)| *quantizer == next_quantizer)
            {
                // Predicted quantizer has already been probed
                log_probes(
                    &quantizer_score_history,
                    self.target,
                    chunk.frames() as u32,
                    self.probing_rate as u32,
                    self.probing_speed,
                    &chunk.name(),
                    *quantizer,
                    *score,
                    SkipProbingReason::None,
                );
                break;
            }

            update_progress_bar(next_quantizer);

            let probe_path = self.vmaf_probe(chunk, next_quantizer as usize)?;
            let score =
                read_weighted_vmaf(&probe_path, self.probing_statistic.clone()).map_err(|e| {
                    Box::new(EncoderCrash {
                        exit_status:        std::process::ExitStatus::default(),
                        source_pipe_stderr: String::new().into(),
                        ffmpeg_pipe_stderr: None,
                        stderr:             format!("VMAF calculation failed: {e}").into(),
                        stdout:             String::new().into(),
                    })
                })?;
            let score_within_tolerance = within_tolerance(score, self.target);

            quantizer_score_history.push((next_quantizer, score));

            if score_within_tolerance || quantizer_score_history.len() >= self.probes as usize {
                log_probes(
                    &quantizer_score_history,
                    self.target,
                    chunk.frames() as u32,
                    self.probing_rate as u32,
                    self.probing_speed,
                    &chunk.name(),
                    next_quantizer,
                    score,
                    if score_within_tolerance {
                        SkipProbingReason::WithinTolerance
                    } else {
                        SkipProbingReason::ProbeLimitReached
                    },
                );
                break;
            }

            if score > self.target {
                lower_quantizer_limit = (next_quantizer + 1).min(upper_quantizer_limit);
            } else {
                upper_quantizer_limit = (next_quantizer - 1).max(lower_quantizer_limit);
            }

            // Ensure quantizer limits are valid
            if lower_quantizer_limit > upper_quantizer_limit {
                log_probes(
                    &quantizer_score_history,
                    self.target,
                    chunk.frames() as u32,
                    self.probing_rate as u32,
                    self.probing_speed,
                    &chunk.name(),
                    next_quantizer,
                    score,
                    if score > self.target {
                        SkipProbingReason::QuantizerTooHigh
                    } else {
                        SkipProbingReason::QuantizerTooLow
                    },
                );
                break;
            }
        }

        let final_quantizer_score = if let Some(highest_quantizer_score_within_tolerance) =
            quantizer_score_history
                .iter()
                .filter(|(_, score)| within_tolerance(*score, self.target))
                .max_by_key(|(quantizer, _)| *quantizer)
        {
            // Multiple probes within tolerance, choose the highest
            highest_quantizer_score_within_tolerance
        } else {
            // No quantizers within tolerance, choose the quantizer closest to target
            quantizer_score_history
                .iter()
                .min_by(|(_, score1), (_, score2)| {
                    let difference1 = (score1 - self.target).abs();
                    let difference2 = (score2 - self.target).abs();
                    difference1.partial_cmp(&difference2).unwrap_or(Ordering::Equal)
                })
                .unwrap()
        };

        Ok(final_quantizer_score.0)
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
            use std::os::unix::io::{AsRawFd, FromRawFd};

            let mut source = if let [pipe_cmd, args @ ..] = &*chunk.source_cmd {
                tokio::process::Command::new(pipe_cmd)
                    .args(args)
                    .stderr(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| EncoderCrash {
                        exit_status:        std::process::ExitStatus::default(),
                        source_pipe_stderr: format!("Failed to spawn source: {e}").into(),
                        ffmpeg_pipe_stderr: None,
                        stderr:             String::new().into(),
                        stdout:             String::new().into(),
                    })?
            } else {
                unreachable!()
            };

            let source_stdout = source.stdout.take().unwrap();
            let source_stdout_fd = source_stdout.as_raw_fd();

            let mut source_pipe = if let [ffmpeg, args @ ..] = &*cmd.0 {
                tokio::process::Command::new(ffmpeg)
                    .args(args)
                    .stdin(unsafe { std::process::Stdio::from_raw_fd(source_stdout_fd) })
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| EncoderCrash {
                        exit_status:        std::process::ExitStatus::default(),
                        source_pipe_stderr: format!("Failed to spawn ffmpeg: {e}").into(),
                        ffmpeg_pipe_stderr: None,
                        stderr:             String::new().into(),
                        stdout:             String::new().into(),
                    })?
            } else {
                unreachable!()
            };

            let source_pipe_stdout = source_pipe.stdout.take().unwrap();
            let source_pipe_stdout_fd = source_pipe_stdout.as_raw_fd();

            let mut enc_pipe = if let [cmd, args @ ..] = &*cmd.1 {
                tokio::process::Command::new(cmd.as_ref())
                    .args(args.iter().map(AsRef::as_ref))
                    .stdin(unsafe { std::process::Stdio::from_raw_fd(source_pipe_stdout_fd) })
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| EncoderCrash {
                        exit_status:        std::process::ExitStatus::default(),
                        source_pipe_stderr: String::new().into(),
                        ffmpeg_pipe_stderr: None,
                        stderr:             format!("Failed to spawn encoder: {e}").into(),
                        stdout:             String::new().into(),
                    })?
            } else {
                unreachable!()
            };

            // Drop stdout to prevent buffer deadlock
            drop(enc_pipe.stdout.take());

            // Consume stderr streams to prevent deadlock
            let source_stderr = source.stderr.take();
            let source_pipe_stderr = source_pipe.stderr.take();
            let enc_stderr = enc_pipe.stderr.take();

            let stderr_handles = tokio::join!(
                async {
                    if let Some(mut stderr) = source_stderr {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        let _ = stderr.read_to_end(&mut buf).await;
                        buf
                    } else {
                        Vec::new()
                    }
                },
                async {
                    if let Some(mut stderr) = source_pipe_stderr {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        let _ = stderr.read_to_end(&mut buf).await;
                        buf
                    } else {
                        Vec::new()
                    }
                },
                async {
                    if let Some(mut stderr) = enc_stderr {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        let _ = stderr.read_to_end(&mut buf).await;
                        buf
                    } else {
                        Vec::new()
                    }
                }
            );

            let enc_result = tokio::join!(source.wait(), source_pipe.wait(), enc_pipe.wait()).2;

            let enc_status = enc_result.map_err(|e| EncoderCrash {
                exit_status:        std::process::ExitStatus::default(),
                source_pipe_stderr: String::new().into(),
                ffmpeg_pipe_stderr: None,
                stderr:             format!("Failed to wait for encoder: {e}").into(),
                stdout:             String::new().into(),
            })?;

            if !enc_status.success() {
                return Err(EncoderCrash {
                    exit_status:        enc_status,
                    source_pipe_stderr: stderr_handles.0.into(),
                    ffmpeg_pipe_stderr: Some(stderr_handles.1.into()),
                    stderr:             stderr_handles.2.into(),
                    stdout:             String::new().into(),
                });
            }

            Ok(())
        };

        let rt = tokio::runtime::Builder::new_current_thread().enable_io().build().unwrap();
        rt.block_on(future)?;

        let extension = match self.encoder {
            crate::encoder::Encoder::x264 => "264",
            crate::encoder::Encoder::x265 => "hevc",
            _ => "ivf",
        };

        let probe_name = std::path::Path::new(&chunk.temp)
            .join("split")
            .join(format!("v_{index:05}_{q}.{extension}", index = chunk.index));
        let fl_path = std::path::Path::new(&chunk.temp)
            .join("split")
            .join(format!("{index}.json", index = chunk.index));

        let features: HashSet<_> = self.probing_vmaf_features.iter().copied().collect();
        let use_weighted = features.contains(&VmafFeature::Weighted);
        let use_neg = features.contains(&VmafFeature::Neg);
        let use_uhd = features.contains(&VmafFeature::Uhd);
        let disable_motion = features.contains(&VmafFeature::Motionless);

        let default_model = match (use_uhd, use_neg) {
            (true, true) => Some(PathBuf::from("vmaf_4k_v0.6.1neg.json")),
            (true, false) => Some(PathBuf::from("vmaf_4k_v0.6.1.json")),
            (false, true) => Some(PathBuf::from("vmaf_v0.6.1neg.json")),
            (false, false) => None,
        };

        let model = if self.model.is_none() {
            default_model.as_ref()
        } else {
            self.model.as_ref()
        };

        if use_weighted {
            crate::vmaf::run_vmaf_weighted(
                &probe_name,
                chunk.source_cmd.as_slice(),
                self.vspipe_args.clone(),
                &fl_path,
                model,
                &self.vmaf_res,
                &self.vmaf_scaler,
                self.probing_rate,
                self.vmaf_filter.as_deref(),
                self.vmaf_threads,
                chunk.frame_rate,
                disable_motion,
            )
            .map_err(|e| {
                Box::new(EncoderCrash {
                    exit_status:        std::process::ExitStatus::default(),
                    source_pipe_stderr: String::new().into(),
                    ffmpeg_pipe_stderr: None,
                    stderr:             format!("VMAF calculation failed: {e}").into(),
                    stdout:             String::new().into(),
                })
            })?;
        } else {
            crate::vmaf::run_vmaf(
                &probe_name,
                chunk.source_cmd.as_slice(),
                self.vspipe_args.clone(),
                &fl_path,
                model,
                &self.vmaf_res,
                &self.vmaf_scaler,
                self.probing_rate,
                self.vmaf_filter.as_deref(),
                self.vmaf_threads,
                chunk.frame_rate,
                disable_motion,
            )?;
        }

        Ok(fl_path)
    }

    #[inline]
    pub fn per_shot_target_quality_routine(
        &self,
        chunk: &mut Chunk,
        worker_id: Option<usize>,
    ) -> anyhow::Result<()> {
        chunk.tq_cq = Some(self.per_shot_target_quality(chunk, worker_id)?);
        Ok(())
    }
}

fn predict_quantizer(
    lower_quantizer_limit: u32,
    upper_quantizer_limit: u32,
    quantizer_score_history: &[(u32, f64)],
    target: f64,
) -> u32 {
    // The midpoint between the upper and lower quantizer bounds
    let binary_search = (lower_quantizer_limit + upper_quantizer_limit) / 2;
    if quantizer_score_history.len() < 2 {
        // Fewer than 2 probes, predict using binary search
        return binary_search;
    }

    // Sort history by quantizer
    let mut sorted_quantizer_score_history = quantizer_score_history.to_vec();
    sorted_quantizer_score_history.sort_by_key(|(quantizer, _)| *quantizer);

    let keys = sorted_quantizer_score_history
        .iter()
        .map(|(quantizer, score)| {
            Key::new(
                *score,
                *quantizer as f64,
                match sorted_quantizer_score_history.len() {
                    0..=1 => unreachable!(),        // Handled in earlier guard
                    2 => Interpolation::Linear,     // 2 probes, use Linear without fitting curve
                    _ => Interpolation::CatmullRom, // 3 or more probes, fit CatmullRom curve
                },
            )
        })
        .collect::<Vec<_>>();

    let spline = Spline::from_vec(keys);
    let predicted_quantizer = spline.sample(target).unwrap_or_else(|| {
        // Probes do not fit Catmull-Rom curve, fallback to Linear
        trace!("Probes do not fit Catmull-Rom curve, falling back to Linear");
        let keys = sorted_quantizer_score_history
            .iter()
            .map(|(quantizer, score)| Key::new(*score, *quantizer as f64, Interpolation::Linear))
            .collect();
        Spline::from_vec(keys).sample(target).unwrap_or_else(|| {
            // Probes do not fit Catmull-Rom curve or Linear, fallback to binary search
            trace!("Probes do not fit Linear curve, falling back to binary search");
            binary_search as f64
        })
    });

    // Ensure predicted quantizer is an integer and within bounds
    (predicted_quantizer.round() as u32).clamp(lower_quantizer_limit, upper_quantizer_limit)
}

fn within_tolerance(score: f64, target: f64) -> bool {
    (score - target).abs() / target < SCORE_TOLERANCE
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
pub enum SkipProbingReason {
    QuantizerTooHigh,
    QuantizerTooLow,
    WithinTolerance,
    ProbeLimitReached,
    None,
}

#[allow(clippy::too_many_arguments)]
pub fn log_probes(
    quantizer_score_history: &[(u32, f64)],
    target: f64,
    frames: u32,
    probing_rate: u32,
    probing_speed: Option<u8>,
    chunk_name: &str,
    target_quantizer: u32,
    target_score: f64,
    skip: SkipProbingReason,
) {
    // Sort history by quantizer
    let mut sorted_quantizer_scores = quantizer_score_history.to_vec();
    sorted_quantizer_scores.sort_by_key(|(quantizer, _)| *quantizer);

    debug!(
        "chunk {name}: Target={target}, P-Rate={rate}, P-Speed={speed:?}, {frame_count} frames
        TQ-Probes: {history:.2?}{suffix}
        Final Q={target_quantizer:.0}, Final Score={target_score:.2}",
        name = chunk_name,
        target = target,
        rate = probing_rate,
        speed = ProbingSpeed::from_repr(probing_speed.unwrap_or(4) as usize),
        frame_count = frames,
        history = sorted_quantizer_scores,
        suffix = match skip {
            SkipProbingReason::None => "",
            SkipProbingReason::QuantizerTooHigh => "Early Skip High Quantizer",
            SkipProbingReason::QuantizerTooLow => " Early Skip Low Quantizer",
            SkipProbingReason::WithinTolerance => " Early Skip Within Tolerance",
            SkipProbingReason::ProbeLimitReached => " Early Skip Probe Limit Reached",
        },
        target_quantizer = target_quantizer,
        target_score = target_score
    );
}

#[inline]
pub const fn adapt_probing_rate(rate: usize) -> usize {
    match rate {
        1..=4 => rate,
        _ => 1,
    }
}
