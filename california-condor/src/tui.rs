use std::{
    collections::BTreeMap,
    io::{IsTerminal, Write},
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};

use andean_condor::{
    core::{
        Condor,
        input::{Input, OpenProgress, clip_info::ClipInfo},
        sequence::{
            Sequence,
            benchmarker::Benchmarker,
            bitrate_optimizer::BitrateOptimizer,
            noise_detector::NoiseDetector,
            noise_scaler::NoiseScaler,
            parallel_encoder::ParallelEncoder,
            quality_check::QualityCheck,
            scene_concatenator::SceneConcatenator,
            scene_detector::SceneDetector,
            speed_scaler::SpeedScaler,
            target_quality::TargetQuality,
        },
    },
    models::input::Input as InputModel,
};
use anyhow::{Result, bail};
use thiserror::Error as ThisError;
use tracing::{debug, error, info, trace, warn};

use crate::{
    apps::{
        TuiApp,
        benchmarker::BenchmarkerApp,
        noise_detection::NoiseDetectionApp,
        parallel_encoder::ParallelEncoderApp,
        quality_check::QualityCheckApp,
        scene_concatenator::SceneConcatenatorApp,
        scene_detection::SceneDetectionApp,
        target_quality::TargetQualityApp,
    },
    configuration::{CliSequenceConfig, CliSequenceData},
};

/// Opens `data`, logging its progress, if available.
///
/// Only FFMS2 and DGDecNV report real progress. A VapourSynth input indexes
/// inside its source plugin, where no callback can reach it, so it only gets
/// the single `Opening input...` line.
///
/// Returns the input together with its probed clip info, so callers do not have
/// to reopen it just to read the header.
pub fn open_input(data: &InputModel, cancelled: &Arc<AtomicBool>) -> Result<(Input, ClipInfo)> {
    info!("Opening input...");
    // FFMS2 reports every whole percent, so only the tens are worth a line.
    let mut last_decile = 0;
    let mut input = Input::ensure_indexed(
        data,
        |progress| {
            let OpenProgress::Indexing {
                current,
                total,
            } = progress
            else {
                return;
            };
            if total == 0 {
                return;
            }
            let decile = (current * 100 / total).min(100) / 10;
            if decile > last_decile {
                last_decile = decile;
                trace!("Indexing input: {}%", decile * 10);
            }
        },
        Some(cancelled),
    )?;
    let clip_info = input.clip_info()?;
    Ok((input, clip_info))
}

#[tracing::instrument(skip_all)]
pub fn run_scene_detector_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let initial_frames = condor.scenes.iter().fold(0, |acc, scene| {
        acc + (scene.end_frame - scene.start_frame) as u64
    });
    let initial_scenes = condor
        .scenes
        .iter()
        .map(|scene| (scene.start_frame as u64, scene.end_frame as u64))
        .collect::<Vec<_>>();

    debug!("Instantiating Scene Detector Input");
    // Without a `scene_detector.input` override there is nothing to open:
    // `SceneDetector` reads `condor.input` itself when it holds no input of
    // its own, so re-opening the same model here would only index it twice.
    let (input, clip_info) = if let Some(configured) = &condor.sequence_config.scene_detector.input
    {
        let (input, clip_info) = open_input(configured, &cancelled)?;
        (Some(input), clip_info)
    } else {
        (None, condor.input.clip_info()?)
    };

    if initial_frames as usize == clip_info.num_frames {
        return Ok(());
    }

    let (input, clip_info) = if clip_info.num_frames == condor.input.clip_info()?.num_frames {
        (input, clip_info)
    } else {
        let input_frames = condor.input.clip_info()?.num_frames;
        let scd_input_frames = clip_info.num_frames;
        let main_input_data = condor.input.as_data();
        let scd_input_data = condor
            .sequence_config
            .scene_detector
            .input
            .clone()
            .unwrap_or_else(|| main_input_data.clone());

        if scd_input_data.has_time_altering_filters()
            || !main_input_data.has_time_altering_filters()
        {
            bail!(TUIError::FramesMismatchWithInputFilters(
                input_frames as u64,
                scd_input_frames as u64
            ));
        }

        // Try to recover by appending the time altering filters from the input to the
        // scd input
        warn!(
            "Input and Scene Detector Input have mismatched frames and Scene Detector Input is \
             missing time altering filters such as trim or splice."
        );
        info!("Adding known time altering filters from Input to Scene Detector Input");
        let start = Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= Duration::from_secs(5) {
                break;
            }
            if std::io::stdout().is_terminal() {
                print!("\rResuming in {} seconds...", 5 - elapsed.as_secs());
                std::io::stdout().flush()?;
            } else {
                eprint!("\rResuming in {} seconds...", 5 - elapsed.as_secs());
                std::io::stderr().flush()?;
            }
            thread::sleep(Duration::from_secs(1));
        }

        let mut combined = scd_input_data;
        combined.append_input_filters(&main_input_data)?;
        let (scd_input, clip_info) = open_input(&combined, &cancelled)?;

        // Double check that the frames match
        if clip_info.num_frames != condor.input.clip_info()?.num_frames {
            error!("Failed to recover with time altering filters");
            bail!(TUIError::FramesMismatchWithInputFilters(
                input_frames as u64,
                scd_input_frames as u64
            ));
        }

        (Some(scd_input), clip_info)
    };

    let mut scene_detector = SceneDetector {
        input,
        method: condor.sequence_config.scene_detector.method.clone(),
    };

    debug!("Validating Scene Detector"); // Input should alrady be validated but just in case
    let (_, validation_warnings) = scene_detector.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Scene Detector"); // Input should already be indexed but just in case
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = scene_detector.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Scene Detector");
    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut scd_app = SceneDetectionApp::new(
            initial_frames,
            clip_info.num_frames as u64,
            initial_scenes,
            clip_info,
        );
        scd_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = scene_detector.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Scene Detection TUI thread panicked"))??;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_noise_detector_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    debug!("Instantiating Noise Detector Input");
    let Some(noise_detector_config) = &condor.sequence_config.noise_detector else {
        return Ok(());
    };

    let (input, clip_info) = if let Some(input) = &noise_detector_config.input {
        let (nd_input, clip_info) = open_input(input, &cancelled)?;
        (Some(nd_input), clip_info)
    } else {
        (None, condor.input.clip_info()?)
    };

    let total_scenes = condor.scenes.len();
    if total_scenes == 0
        || condor.scenes.iter().all(|scene| scene.sequence_data.noise_detection.is_some())
    {
        return Ok(());
    }

    let mut noise_detector = NoiseDetector {
        input,
    };

    debug!("Validating Noise Detector"); // Input should alrady be validated but just in case
    let (_, validation_warnings) = noise_detector.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Noise Detector"); // Input should already be indexed but just in case
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = noise_detector.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Noise Detector");
    let initial_scenes = condor.scenes.clone();
    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut nd_app = NoiseDetectionApp::new(&initial_scenes, clip_info);
        nd_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = noise_detector.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Noise Detection TUI thread panicked"))??;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_noise_scaler_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let mut noise_scaler = NoiseScaler::default();
    debug!("Validating Noise Scaler");
    let (_, validation_warnings) = noise_scaler.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Noise Scaler");
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = noise_scaler.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Noise Scaler");
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let (_, processing_warnings) = noise_scaler.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_benchmarker_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let mut benchmarker = Benchmarker::default();

    debug!("Validating Benchmarker"); // Input should alrady be validated but just in case
    let (_, validation_warnings) = benchmarker.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Benchmarker"); // Input should already be indexed but just in case
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = benchmarker.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Benchmarker");
    let encoder = condor.encoder.clone();
    let clip_info = condor.input.clip_info()?;

    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut benchmarker_app = BenchmarkerApp::new(encoder, clip_info);
        benchmarker_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = benchmarker.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Benchmarker TUI thread panicked"))??;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_target_quality_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    debug!("Instantiating Target Quality Input");
    let (target_quality_input, clip_info) = if let Some(Some(input)) =
        &condor.sequence_config.target_quality.as_ref().map(|tq| tq.input.clone())
    {
        let (tq_input, clip_info) = open_input(input, &cancelled)?;
        (Some(tq_input), clip_info)
    } else if let Some(pe_input) = &condor.sequence_config.parallel_encoder.input {
        debug!("Falling back to Parallel Encoder input");
        let (tq_input, clip_info) = open_input(pe_input, &cancelled)?;
        (Some(tq_input), clip_info)
    } else {
        (None, condor.input.clip_info()?)
    };
    let target_quality_metric_input = match condor
        .sequence_config
        .target_quality
        .as_ref()
        .and_then(|tq| tq.metric_input.clone())
    {
        Some(input) => Some(open_input(&input, &cancelled)?.0),
        None => None,
    };

    let mut target_quality = TargetQuality::new(target_quality_input, target_quality_metric_input);

    debug!("Validating Target Quality"); // Input should alrady be validated but just in case
    let (_, validation_warnings) = target_quality.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Target Quality"); // Input should already be indexed but just in case
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = target_quality.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Target Quality");
    let mut encoder = condor.encoder.clone();
    if let Some(target_quality_encoder_options) = condor
        .sequence_config
        .target_quality
        .as_ref()
        .map(|tq| tq.probing.clone())
        .and_then(|p| p.encoder_options)
    {
        encoder.parameters_mut().clear();
        encoder.parameters_mut().extend(target_quality_encoder_options);
    }
    let initial_scenes = condor.scenes.clone();
    let probe_statistic = condor
        .sequence_config
        .target_quality
        .as_ref()
        .expect("target_quality is required")
        .probing
        .statistic;
    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut tq_app = TargetQualityApp::new(clip_info, initial_scenes, encoder, probe_statistic);
        tq_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = target_quality.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Target Quality TUI thread panicked"))??;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_quality_check_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    debug!("Instantiating Quality Check Input");
    let (quality_check_input, clip_info) = if let Some(Some(input)) =
        &condor.sequence_config.quality_check.as_ref().map(|qc| qc.input.clone())
    {
        let (qc_input, clip_info) = open_input(input, &cancelled)?;
        (Some(qc_input), clip_info)
    } else if let Some(pe_input) = &condor.sequence_config.parallel_encoder.input {
        debug!("Falling back to Parallel Encoder input");
        let (qc_input, clip_info) = open_input(pe_input, &cancelled)?;
        (Some(qc_input), clip_info)
    } else {
        (None, condor.input.clip_info()?)
    };

    let mut quality_check = QualityCheck::new(quality_check_input);

    debug!("Validating Quality Check");
    let (_, validation_warnings) = quality_check.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Quality Check");
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = quality_check.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Quality Check");
    let initial_scenes = condor.scenes.clone();
    let quality_check_config = condor
        .sequence_config
        .quality_check
        .as_ref()
        .expect("quality_check is required");
    let metric = quality_check_config.metric.clone();
    let statistic = quality_check_config.statistic;
    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut qc_app = QualityCheckApp::new(clip_info, &initial_scenes, metric, statistic);
        qc_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = quality_check.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Quality Check TUI thread panicked"))??;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_bitrate_optimizer_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let mut bitrate_optimizer = BitrateOptimizer::default();
    debug!("Validating Bitrate Optimizer");
    let (_, validation_warnings) = bitrate_optimizer.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Bitrate Optimizer");
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = bitrate_optimizer.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Bitrate Optimizer");
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let (_, processing_warnings) = bitrate_optimizer.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_speed_scaler_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let mut convex_hull = SpeedScaler::default();
    debug!("Validating Convex Hull");
    let (_, validation_warnings) = convex_hull.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Convex Hull");
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = convex_hull.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Convex Hull");
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    let (_, processing_warnings) = convex_hull.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_parallel_encoder_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    debug!("Instantiating Parallel Encoder Input");
    let (parallel_encoder_input, clip_info) =
        if let Some(input) = &condor.sequence_config.parallel_encoder.input {
            let (pe_input, clip_info) = open_input(input, &cancelled)?;
            (Some(pe_input), clip_info)
        } else {
            (None, condor.input.clip_info()?)
        };

    let workers = condor.sequence_config.parallel_encoder.workers.unwrap_or(1);
    let mut parallel_encoder = ParallelEncoder::new(parallel_encoder_input);

    debug!("Validating Parallel Encoder"); // Input should alrady be validated but just in case
    let (_, validation_warnings) = parallel_encoder.validate(condor)?;

    for warning in validation_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Initializing Parallel Encoder"); // Input should already be indexed but just in case
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = parallel_encoder.initialize(condor, init_progress_tx)?;

    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Parallel Encoder");
    let encoder = condor.encoder.clone();
    let initial_scenes = condor
        .scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            (
                index,
                scene,
                condor
                    .sequence_config
                    .parallel_encoder
                    .scenes_directory
                    .join(format!(
                        "{}.{}",
                        ParallelEncoder::scene_id(index),
                        scene.encoder.output_extension()
                    ))
                    .exists(),
            )
        })
        .collect::<Vec<_>>();
    let mut scenes_map = BTreeMap::new();
    for (index, scene, already_encoded) in initial_scenes {
        let scene_frames_encoded = if already_encoded {
            (scene.end_frame - scene.start_frame) as u64
        } else {
            0
        };
        scenes_map.insert(index as u64, (scene_frames_encoded, scene.clone()));
    }

    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut pe_app = ParallelEncoderApp::new(workers, encoder, scenes_map, clip_info);
        pe_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    // Joined before propagating either result: a TUI thread still running owns raw
    // mode and the panic hook, so letting one escape into the next sequence would
    // hand both to two apps at once.
    let executed = parallel_encoder.execute(condor, progress_tx, cancelled);

    let tui_result = tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Parallel Encoder TUI thread panicked"))?;

    let (_, processing_warnings) = executed?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_result?;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn run_scene_concatenator_tui(
    condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    cancelled: Arc<AtomicBool>,
) -> Result<()> {
    let scenes_len = condor.scenes.len();
    let clip_info = condor.input.clip_info()?;
    let method = condor.sequence_config.scene_concatenator.method.clone();

    let mut scene_concatenator = SceneConcatenator::default();

    // Validate - Input should be already validated
    let (_, _validation_warnings) = scene_concatenator.validate(condor)?;

    // Initialize - Input should be already indexed
    let (init_progress_tx, _init_progress_rx) = std::sync::mpsc::channel();
    let (_, initialization_warnings) = scene_concatenator.initialize(condor, init_progress_tx)?;
    for warning in initialization_warnings.iter() {
        warn!("{}", warning);
    }

    debug!("Running Scene Concatenator");
    let ctrlc_cancelled = Arc::clone(&cancelled);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel();
    let tui_handle = thread::spawn(move || -> Result<()> {
        let mut sc_app = SceneConcatenatorApp::new(clip_info, scenes_len, method);
        sc_app.run(progress_rx, ctrlc_cancelled)?;
        Ok(())
    });
    let (_, processing_warnings) = scene_concatenator.execute(condor, progress_tx, cancelled)?;

    for warning in processing_warnings.iter() {
        warn!("{}", warning);
    }

    tui_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Scene Concatenator TUI thread panicked"))??;

    Ok(())
}

#[derive(Debug, ThisError)]
pub enum TUIError {
    #[error("Input and Scene Detector Input have a different number of frames: {0} != {0}")]
    FramesMismatch(u64, u64),
    #[error(
        "Input and Scene Detector Input have a different number of frames: {0} != {0}. Check the \
         input filters."
    )]
    FramesMismatchWithInputFilters(u64, u64),
}
