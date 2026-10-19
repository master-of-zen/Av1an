use std::{
    sync::{self, Arc, atomic::AtomicBool},
    thread,
    time::SystemTime,
};

use anyhow::{Result, bail};
use thiserror::Error;
use tracing::error;

use crate::{
    core::{
        Condor,
        input::Input,
        sequence::{Sequence, SequenceCompletion, SequenceDetails, SequenceStatus, Status},
    },
    metrics::{self, Engine, OutputIndexing},
    models::{
        input::{ImportMethod, Input as InputModel},
        sequence::{
            SequenceConfigHandler,
            SequenceDataHandler,
            quality_check::{
                QualityCheckConfig,
                QualityCheckConfigHandler,
                QualityCheckDataHandler,
            },
            target_quality::types::{QualityMetric, QualityPass},
        },
    },
    vapoursynth::{
        VapourSynthError,
        get_core,
        plugins::{
            MetricPluginFunction,
            PluginFunction,
            ffms2::Source,
            resize::bicubic::Bicubic,
            standard::{splice::Splice, trim::Trim},
            vship::{
                butteraugli::BUTTERAUGLI,
                cvvdp::CVVDP,
                ssimulacra2::SSIMULACRA2 as VSHIPSSIMULACRA2,
            },
            vszip::{ssimulacra2::SSIMULACRA2, xpsnr::XPSNR},
        },
    },
};

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Quality Check",
    description: "Measure the quality of the video per scene",
    version:     "0.0.1",
};

pub struct QualityCheck {
    pub input: Option<Input>,
}

impl<DataHandler, ConfigHandler> Sequence<DataHandler, ConfigHandler> for QualityCheck
where
    DataHandler: SequenceDataHandler + QualityCheckDataHandler,
    ConfigHandler: SequenceConfigHandler + QualityCheckConfigHandler,
{
    #[inline]
    fn details(&self) -> SequenceDetails {
        DETAILS
    }

    #[inline]
    fn validate(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let mut warnings = vec![];

        if condor.scenes.is_empty() {
            warnings.push(anyhow::Error::new(QualityCheckError::ScenesEmpty));
        }
        if !condor.output.path.exists() {
            warnings.push(anyhow::Error::new(QualityCheckError::OutputMissing {
                path: condor.output.path.clone(),
            }));
        }
        // Quality Check compares the reference input against the encoded
        // output frame-for-frame, so a picture-altering filter makes the two
        // sides incomparable. Harmless filters still score fine, so this is a
        // warning rather than a failure.
        if condor.scenes.iter().any(|scene| scene.encoder.ffmpeg_filter().is_some()) {
            warnings.push(anyhow::Error::new(QualityCheckError::FilteredEncoderOutput));
        }

        Ok(((), warnings))
    }

    #[inline]
    fn initialize(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
        progress_tx: sync::mpsc::Sender<SequenceStatus>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        let config = condor.sequence_config.quality_check()?;
        let input = if let Some(input) = self.input.as_mut() {
            input
        } else if let Some(config) = config
            && let Some(input_model) = &config.input
        {
            &mut Input::from_data(input_model)?
        } else {
            &mut condor.input
        };
        // Initialize input by getting clip_info. For VapourSynth inputs, this may begin
        // a lengthy caching process, hence the separation between validate and
        // initialize.
        progress_tx.send(SequenceStatus::Whole(Status::Processing {
            id:         DETAILS.name.to_owned(),
            completion: SequenceCompletion::Custom {
                name:      DETAILS.name.to_owned(),
                completed: 0.0,
                total:     1.0,
            },
        }))?;
        input.clip_info()?;
        progress_tx.send(SequenceStatus::Whole(Status::Completed {
            id: DETAILS.name.to_owned(),
        }))?;

        Ok(((), warnings))
    }

    #[inline]
    fn execute(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
        progress_tx: sync::mpsc::Sender<SequenceStatus>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let mut warnings = vec![];

        let Some(config) = condor.sequence_config.quality_check()? else {
            return Ok(((), warnings));
        };

        if condor.scenes.is_empty() {
            warnings.push(anyhow::Error::new(QualityCheckError::ScenesEmpty));
            return Ok(((), warnings));
        }
        if !condor.output.path.exists() {
            warnings.push(anyhow::Error::new(QualityCheckError::OutputMissing {
                path: condor.output.path.clone(),
            }));
            return Ok(((), warnings));
        }

        let input = if let Some(input) = self.input.as_mut() {
            input
        } else if let Some(input_model) = &config.input {
            &mut Input::from_data(input_model)?
        } else {
            &mut condor.input
        };

        let scene_frame_indices = condor
            .scenes
            .iter()
            .enumerate()
            .filter_map(|(index, scene)| {
                let indices = config.strategy.frame_indices(scene.start_frame, scene.end_frame);
                let complete = scene
                    .sequence_data
                    .get_quality_check()
                    .is_ok_and(|data| data.quality.scores.len() == indices.len());
                if complete {
                    None
                } else {
                    Some((index, indices))
                }
            })
            .collect::<Vec<_>>();

        if scene_frame_indices.is_empty() {
            progress_tx.send(SequenceStatus::Whole(Status::Completed {
                id: DETAILS.name.to_owned(),
            }))?;
            return Ok(((), warnings));
        }

        // VMAF scores decoded frames through libvmaf rather than a VapourSynth
        // filter graph, so it takes a separate path. Every other metric runs as a
        // plugin over the nodes built by `measure`, unless a native engine takes
        // it first.
        let measurements = if matches!(config.metric, QualityMetric::VMAF { .. }) {
            Self::measure_vmaf(
                input,
                &condor.output.path,
                config,
                &scene_frame_indices,
                &progress_tx,
                &cancelled,
            )?
        } else if let Some(scores) = Self::measure_native(
            input,
            &condor.output.path,
            config,
            &scene_frame_indices,
            &progress_tx,
            &cancelled,
        )? {
            scores
        } else {
            Self::measure(
                input,
                &condor.output.path,
                config,
                &scene_frame_indices,
                progress_tx,
                &cancelled,
            )?
        };

        for ((index, _), measurement) in scene_frame_indices.iter().zip(measurements) {
            let quality_pass = QualityPass {
                quantizer:    0.0,
                scores:       measurement.scores,
                bitrate:      0.0,
                started_on:   measurement.started_on,
                completed_on: measurement.completed_on,
            };
            condor.scenes[*index].sequence_data.get_quality_check_mut()?.quality = quality_pass;
        }

        let mut data = condor.as_data();
        data.scenes = condor.scenes.clone();
        (condor.save_callback)(data).expect("failed to save data");

        Ok(((), warnings))
    }
}

impl QualityCheck {
    pub const DETAILS: SequenceDetails = DETAILS;

    #[inline]
    pub fn new(input: Option<Input>) -> Self {
        Self {
            input,
        }
    }

    fn measure_vmaf(
        input: &mut Input,
        output_path: &std::path::Path,
        config: &QualityCheckConfig,
        scene_frame_indices: &[(usize, Vec<usize>)],
        progress_tx: &sync::mpsc::Sender<SequenceStatus>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<SceneMeasurement>> {
        let (total_frames, selected) = Self::native_selection(scene_frame_indices);

        let reference = input.decoder();
        let mut distorted = Input::from_video(&InputModel::Video {
            path:          output_path.to_path_buf(),
            import_method: ImportMethod::FFMS2 {
                index:      None,
                cache_path: None,
            },
            // The encoded output is already in the input's format, so it must
            // not be converted again.
            filters:       Vec::new(),
        })?;

        // Each score is reported as the metric produces it, mapped back to its
        // global frame number so the UI's per-frame report lines up with the
        // clip.
        let scores = metrics::score_vmaf_frames(
            reference,
            distorted.decoder(),
            &config.metric,
            &selected,
            OutputIndexing::Aligned,
            Some(cancelled),
            |position, score| {
                Self::report_frame_score(&selected, total_frames, position, score, progress_tx);
            },
        )?;

        Self::group_native_scores(&scores, total_frames, scene_frame_indices)
    }

    /// Report one frame's score as it is produced.
    ///
    /// `position` indexes the selection, which is what the metric reports; the
    /// global frame number is what the UI displays.
    #[inline]
    fn report_frame_score(
        selected: &[usize],
        total_frames: usize,
        position: usize,
        score: f64,
        progress_tx: &sync::mpsc::Sender<SequenceStatus>,
    ) {
        // Progress is advisory: a closed receiver means the UI went away, which
        // is not a scoring failure.
        let _ = progress_tx.send(SequenceStatus::Subprocess {
            parent: Status::Processing {
                id:         DETAILS.name.to_owned(),
                completion: SequenceCompletion::Custom {
                    name:      DETAILS.name.to_owned(),
                    completed: (position + 1) as f64,
                    total:     total_frames as f64,
                },
            },
            child:  Status::Processing {
                id:         "Quality".to_owned(),
                completion: SequenceCompletion::FrameScore {
                    frame: selected[position] as u64,
                    score,
                },
            },
        });
    }

    /// Group the per-frame scores back into scenes, recording when each span
    /// was measured so the UI can show per-scene progress.
    #[inline]
    fn group_native_scores(
        scores: &[f64],
        total_frames: usize,
        scene_frame_indices: &[(usize, Vec<usize>)],
    ) -> Result<Vec<SceneMeasurement>> {
        if scores.len() < total_frames {
            error!(
                expected = total_frames,
                actual = scores.len(),
                "produced fewer scores than selected frames"
            );
            bail!(QualityCheckError::QualityMeasurementFailed);
        }

        let now = || {
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Time is valid")
                .as_millis()
        };

        let mut measurements = Vec::with_capacity(scene_frame_indices.len());
        let mut consumed = 0;
        let mut previous_end = now();
        for (_, indices) in scene_frame_indices {
            let end = consumed + indices.len();
            let completed_on = now();
            measurements.push(SceneMeasurement {
                scores: scores[consumed..end].to_vec(),
                started_on: previous_end,
                completed_on,
            });
            previous_end = completed_on;
            consumed = end;
        }

        Ok(measurements)
    }

    /// Score the selection with whichever native engine is preferred, or report
    /// that the native path does not apply.
    ///
    /// Returns `Ok(None)` when the caller should use its VapourSynth branch:
    /// the metric has no native engine, no engine's library is usable, or
    /// scoring failed. Every such case is logged rather than propagated,
    /// because the plugin branches are a complete implementation of these
    /// metrics.
    fn measure_native(
        input: &mut Input,
        output_path: &std::path::Path,
        config: &QualityCheckConfig,
        scene_frame_indices: &[(usize, Vec<usize>)],
        progress_tx: &sync::mpsc::Sender<SequenceStatus>,
        cancelled: &AtomicBool,
    ) -> Result<Option<Vec<SceneMeasurement>>> {
        let Some(engine) = metrics::engine(&config.metric, Self::input_is_vapoursynth(input))
        else {
            return Ok(None);
        };

        let (total_frames, selected) = Self::native_selection(scene_frame_indices);

        let reference = input.decoder();
        let mut distorted = Input::from_video(&InputModel::Video {
            path:          output_path.to_path_buf(),
            import_method: ImportMethod::FFMS2 {
                index:      None,
                cache_path: None,
            },
            // The encoded output is already in the input's format, so it must
            // not be converted again.
            filters:       Vec::new(),
        })?;

        // Each score is reported as the engine produces it.
        let scores = match engine {
            Engine::Vship => metrics::score_vship_frames(
                reference,
                distorted.decoder(),
                &config.metric,
                &selected,
                OutputIndexing::Aligned,
                Some(cancelled),
                |position, score| {
                    Self::report_frame_score(&selected, total_frames, position, score, progress_tx);
                },
            ),
            Engine::Fmetrics => metrics::fmetrics::score_probed_frames(
                reference,
                distorted.decoder(),
                &config.metric,
                &selected,
                OutputIndexing::Aligned,
                Some(cancelled),
                |position, score| {
                    Self::report_frame_score(&selected, total_frames, position, score, progress_tx);
                },
            ),
            // `engine` returns `None` rather than naming a plugin engine, so
            // reaching this means the ordering changed without this dispatch
            // following it.
            Engine::VapourSynth | Engine::Vmaf => return Ok(None),
        };

        let scores = match scores {
            Ok(scores) => scores,
            // A cancelled pass must not be retried through the plugin, which
            // would ignore the abort and score the whole selection again.
            Err(error) if error.is::<metrics::probe::Cancelled>() => return Err(error),
            Err(error) => {
                // The engine is named because the failure is usually
                // library-specific and the fallback is otherwise silent.
                error!(
                    %error,
                    engine = engine.as_str(),
                    metric = config.metric.friendly_name(),
                    "native metric scoring failed, falling back to the VapourSynth plugin"
                );
                return Ok(None);
            },
        };

        Self::group_native_scores(&scores, total_frames, scene_frame_indices).map(Some)
    }

    /// Whether the reference side is a VapourSynth graph rather than a native
    /// decode, which decides whether fmetrics outranks the plugin.
    #[inline]
    fn input_is_vapoursynth(input: &Input) -> bool {
        !matches!(input, Input::Video { .. })
    }

    /// The frames a native pass scores, and how many that is.
    ///
    /// The selection is flattened across scenes in ascending order, which is
    /// the order both the native driver and the VapourSynth trimming walk.
    #[inline]
    fn native_selection(scene_frame_indices: &[(usize, Vec<usize>)]) -> (usize, Vec<usize>) {
        let total_frames = scene_frame_indices.iter().map(|(_, indices)| indices.len()).sum();
        let selected = scene_frame_indices
            .iter()
            .flat_map(|(_, indices)| indices.iter().copied())
            .collect();

        (total_frames, selected)
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn measure(
        input: &mut Input,
        output_path: &std::path::Path,
        config: &QualityCheckConfig,
        scene_frame_indices: &[(usize, Vec<usize>)],
        progress_tx: sync::mpsc::Sender<SequenceStatus>,
        cancelled: &Arc<AtomicBool>,
    ) -> Result<Vec<SceneMeasurement>> {
        // Owned, so it outlives the `&mut v_input` borrow taken below.
        let mut v_input_owned = input.as_vapoursynth_script()?;
        let v_input = v_input_owned.as_mut();
        let decoder = match input {
            Input::VapourSynth {
                decoder, ..
            }
            | Input::VapourSynthScript {
                decoder, ..
            } => decoder,
            Input::Video {
                ..
            } => v_input.expect("Video Input exists").decoder(),
        };
        let vapoursynth_decoder = decoder.get_vapoursynth_impl().expect("Decoder is VapourSynth");
        let env = &vapoursynth_decoder.env;
        let reference_node = vapoursynth_decoder.get_output(
            vapoursynth_decoder.get_output_index(),
            vapoursynth_decoder.get_node_modifier(),
        )?;
        let core = get_core(env)?;

        let reference_node = {
            let frame_nodes: Vec<_> = scene_frame_indices
                .iter()
                .map(|(_, indices)| {
                    indices
                        .iter()
                        .map(|index| {
                            Trim {
                                first: Some(*index as u32),
                                last: Some(*index as u32),
                                ..Default::default()
                            }
                            .invoke(core, &reference_node)
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect();

            Splice::invoke(core, &frame_nodes)?
        };
        let distorted_node = {
            let source_node = Source {
                source: output_path.to_path_buf(),
                ..Default::default()
            }
            .invoke(core)?;
            let frame_nodes: Vec<_> = scene_frame_indices
                .iter()
                .map(|(_, indices)| {
                    indices
                        .iter()
                        .map(|index| {
                            Trim {
                                first: Some(*index as u32),
                                last: Some(*index as u32),
                                ..Default::default()
                            }
                            .invoke(core, &source_node)
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect();

            Splice::invoke(core, &frame_nodes)?
        };

        // Map local frame index to the global frame index
        let global_frames = scene_frame_indices
            .iter()
            .flat_map(|(_, indices)| indices.iter().copied())
            .collect::<Vec<_>>();
        let total_frames = global_frames.len();

        // Scene is complete when the last frame is compared
        let mut scene_last_local_index = Vec::with_capacity(scene_frame_indices.len());
        let mut local_index = 0;
        for (_, indices) in scene_frame_indices {
            local_index += indices.len();
            scene_last_local_index.push(local_index - 1);
        }

        let progress_tx_clone = progress_tx.clone();
        let (compare_progress_tx, compare_progress_rx) = sync::mpsc::channel();
        thread::spawn(move || -> Result<()> {
            for progress in compare_progress_rx {
                match progress {
                    SequenceStatus::Whole(Status::Processing {
                        id: _name,
                        completion,
                    }) => {
                        #[allow(clippy::collapsible_match)]
                        if let SequenceCompletion::Frames {
                            completed,
                            total,
                        } = completion
                        {
                            progress_tx_clone.send(SequenceStatus::Whole(Status::Processing {
                                id:         "Compare".to_owned(),
                                completion: SequenceCompletion::Frames {
                                    completed,
                                    total,
                                },
                            }))?;
                        }
                    },
                    SequenceStatus::Whole(Status::Completed {
                        id: _,
                    }) => {
                        progress_tx_clone.send(SequenceStatus::Whole(Status::Completed {
                            id: "Compare".to_owned(),
                        }))?;
                    },
                    _ => (),
                }
            }
            Ok(())
        });

        // Live per-frame score events
        let scene_count = scene_frame_indices.len();
        let scene_state = Arc::new(sync::Mutex::new((
            vec![Vec::new(); scene_count],
            vec![false; scene_count],
            vec![0u128; scene_count],
            vec![0u128; scene_count],
        )));
        let on_frame = {
            let progress_tx = progress_tx.clone();
            let scene_last_local_index = scene_last_local_index.clone();
            let scene_indices =
                scene_frame_indices.iter().map(|(index, _)| *index).collect::<Vec<_>>();
            let statistic = config.statistic;
            let scene_state_clone = Arc::clone(&scene_state);

            move |local_index: usize, score: &f64| -> Result<(), VapourSynthError> {
                let frame = global_frames[local_index];
                let _ = progress_tx.send(SequenceStatus::Subprocess {
                    parent: Status::Processing {
                        id:         DETAILS.name.to_owned(),
                        completion: SequenceCompletion::Custom {
                            name:      DETAILS.name.to_owned(),
                            completed: (local_index + 1) as f64,
                            total:     total_frames as f64,
                        },
                    },
                    child:  Status::Processing {
                        id:         "Quality".to_owned(),
                        completion: SequenceCompletion::FrameScore {
                            frame: frame as u64,
                            score: *score,
                        },
                    },
                });

                let scene_index = scene_last_local_index
                    .iter()
                    .position(|last| *last >= local_index)
                    .expect("local index belongs to a scene");
                let mut scene_state =
                    scene_state_clone.lock().expect("scene state mutex should acquire lock");
                let (scene_scores, scene_completed, scene_started_on, scene_completed_on) =
                    &mut *scene_state;
                if scene_scores[scene_index].is_empty() {
                    scene_started_on[scene_index] = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("Time is valid")
                        .as_millis();
                }
                scene_scores[scene_index].push(*score);
                if !scene_completed[scene_index]
                    && scene_last_local_index[scene_index] == local_index
                {
                    scene_completed[scene_index] = true;
                    scene_completed_on[scene_index] = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("Time is valid")
                        .as_millis();
                    let scene_score = statistic.calculate(&scene_scores[scene_index]);
                    let _ = progress_tx.send(SequenceStatus::Subprocess {
                        parent: Status::Processing {
                            id:         DETAILS.name.to_owned(),
                            completion: SequenceCompletion::Custom {
                                name:      DETAILS.name.to_owned(),
                                completed: (local_index + 1) as f64,
                                total:     total_frames as f64,
                            },
                        },
                        child:  Status::Processing {
                            id:         "Quality".to_owned(),
                            completion: SequenceCompletion::SceneQuality {
                                index:     scene_indices[scene_index] as u64,
                                quantizer: 0.0,
                                score:     scene_score,
                                bitrate:   0.0,
                            },
                        },
                    });
                }

                Ok(())
            }
        };

        // The SSIMULACRA2, Butteraugli and CVVDP branches are the fallback for
        // the metrics libvship also implements: they are reached only when
        // `measure_vship` found no usable libvship, or its scoring failed.
        match &config.metric {
            // Handled by `measure_vmaf` before this graph is built: VMAF reads
            // decoded frames through libvmaf instead of a VapourSynth filter.
            QualityMetric::VMAF {
                ..
            } => {
                unreachable!("VMAF is measured by `measure_vmaf`, not here");
            },
            QualityMetric::SSIMULACRA2 {
                resolution,
                threads,
                gpu_id,
                ..
            } => {
                let (reference_node, distorted_node) = if let Some((width, height)) = *resolution {
                    let resize = Bicubic {
                        width: Some(width),
                        height: Some(height),
                        ..Default::default()
                    };
                    (
                        resize.invoke(core, &reference_node)?,
                        resize.invoke(core, &distorted_node)?,
                    )
                } else {
                    (reference_node, distorted_node)
                };
                if VSHIPSSIMULACRA2::plugin_is_installed(core) {
                    let plugin = VSHIPSSIMULACRA2 {
                        num_stream: threads.map_or(Some(4), |threads| Some(threads as u32)),
                        gpu_id: gpu_id.map(u32::from),
                        ..Default::default()
                    };
                    let node = plugin.invoke(core, &reference_node, &distorted_node)?;
                    VSHIPSSIMULACRA2::collect_frame_values(
                        &node,
                        compare_progress_tx,
                        on_frame,
                        |frame| {
                            VSHIPSSIMULACRA2::PROPERTY_NAMES
                                .iter()
                                .find_map(|property_name| {
                                    frame.props().get_float(property_name).ok()
                                })
                                .ok_or_else(|| {
                                    VSHIPSSIMULACRA2::new_error(format!(
                                        "Score not found on any of the following properties: {}",
                                        VSHIPSSIMULACRA2::PROPERTY_NAMES.join(", ")
                                    ))
                                })
                        },
                    )?
                } else if SSIMULACRA2::plugin_is_installed(core) {
                    let node = SSIMULACRA2::invoke(core, &reference_node, &distorted_node)?;
                    SSIMULACRA2::collect_frame_values(
                        &node,
                        compare_progress_tx,
                        on_frame,
                        |frame| {
                            SSIMULACRA2::PROPERTY_NAMES
                                .iter()
                                .find_map(|property_name| {
                                    frame.props().get_float(property_name).ok()
                                })
                                .ok_or_else(|| {
                                    SSIMULACRA2::new_error(format!(
                                        "Score not found on any of the following properties: {}",
                                        SSIMULACRA2::PROPERTY_NAMES.join(", ")
                                    ))
                                })
                        },
                    )?
                } else {
                    error!("No VapourSynth SSIMULACRA2 plugin found");
                    bail!(QualityCheckError::QualityMeasurementFailed);
                }
            },
            QualityMetric::BUTTERAUGLI {
                resolution,
                threads,
                intensity_multiplier,
                norm,
                gpu_id,
                ..
            } => {
                let (reference_node, distorted_node) = if let Some((width, height)) = *resolution {
                    let resize = Bicubic {
                        width: Some(width),
                        height: Some(height),
                        ..Default::default()
                    };
                    (
                        resize.invoke(core, &reference_node)?,
                        resize.invoke(core, &distorted_node)?,
                    )
                } else {
                    (reference_node, distorted_node)
                };
                let plugin = BUTTERAUGLI {
                    num_stream: threads.map_or(Some(4), |threads| Some(threads as u32)),
                    intensity_multiplier: *intensity_multiplier,
                    q_norm: norm.map(|norm| norm as u32),
                    gpu_id: gpu_id.map(u32::from),
                    ..Default::default()
                };
                let node = plugin.invoke(core, &reference_node, &distorted_node)?;
                let property_names = norm.and_then(|_| Some(BUTTERAUGLI::QNORM_PROPERTY_NAMES));
                let property_names = property_names.unwrap_or(BUTTERAUGLI::PROPERTY_NAMES);
                BUTTERAUGLI::collect_frame_values(
                    &node,
                    compare_progress_tx,
                    on_frame,
                    move |frame| {
                        property_names
                            .iter()
                            .find_map(|property_name| frame.props().get_float(property_name).ok())
                            .ok_or_else(|| {
                                BUTTERAUGLI::new_error(format!(
                                    "Score not found on any of the following properties: {}",
                                    property_names.join(", ")
                                ))
                            })
                    },
                )?
            },
            QualityMetric::XPSNR {
                resolution, ..
            } => {
                let (reference_node, distorted_node) = if let Some((width, height)) = *resolution {
                    let resize = Bicubic {
                        width: Some(width),
                        height: Some(height),
                        ..Default::default()
                    };
                    (
                        resize.invoke(core, &reference_node)?,
                        resize.invoke(core, &distorted_node)?,
                    )
                } else {
                    (reference_node, distorted_node)
                };
                let plugin = XPSNR {
                    temporal: Some(false),
                    verbose: Some(false),
                    ..Default::default()
                };
                let node = plugin.invoke(core, &reference_node, &distorted_node)?;
                // XPSNR returns a score per plane, combine them into the weighted XPSNR score.
                XPSNR::collect_frame_values(&node, compare_progress_tx, on_frame, |frame| {
                    let plane_scores = XPSNR::PROPERTY_NAMES
                        .iter()
                        .map(|property_name| {
                            frame.props().get_float(property_name).map_err(|error| {
                                XPSNR::new_error(format!(
                                    "Score not found on required property \"{property_name}\": \
                                     {error}"
                                ))
                            })
                        })
                        .collect::<Result<Vec<f64>, _>>()?;
                    match plane_scores.as_slice() {
                        [y, u, v] => Ok(XPSNR::weight_xpsnr(*y, *u, *v)),
                        _ => Err(XPSNR::new_error(
                            "XPSNR should return three plane scores".to_owned(),
                        )),
                    }
                })?
            },
            QualityMetric::CVVDP {
                resolution,
                display_model,
                resize_to_display,
                disable_temporal,
                gpu_id,
                ..
            } => {
                let (reference_node, distorted_node) = if let Some((width, height)) = *resolution {
                    let resize = Bicubic {
                        width: Some(width),
                        height: Some(height),
                        ..Default::default()
                    };
                    (
                        resize.invoke(core, &reference_node)?,
                        resize.invoke(core, &distorted_node)?,
                    )
                } else {
                    (reference_node, distorted_node)
                };
                let plugin = CVVDP {
                    model_name: *display_model,
                    resize_to_display: *resize_to_display,
                    disable_temporal: *disable_temporal,
                    gpu_id: gpu_id.map(u32::from),
                    ..Default::default()
                };
                let node = plugin.invoke(core, &reference_node, &distorted_node)?;
                CVVDP::collect_frame_values(&node, compare_progress_tx, on_frame, |frame| {
                    CVVDP::PROPERTY_NAMES
                        .iter()
                        .find_map(|property_name| frame.props().get_float(property_name).ok())
                        .ok_or_else(|| {
                            CVVDP::new_error(format!(
                                "Score not found on any of the following properties: {}",
                                CVVDP::PROPERTY_NAMES.join(", ")
                            ))
                        })
                })?
            },
        };

        progress_tx.send(SequenceStatus::Whole(Status::Completed {
            id: "Compare".to_owned(),
        }))?;
        drop(progress_tx);

        if cancelled.load(sync::atomic::Ordering::Relaxed) {
            bail!(QualityCheckError::Cancelled);
        }

        let scene_state = scene_state.lock().expect("scene state mutex should acquire lock");
        let (scene_scores, _scene_completed, scene_started_on, scene_completed_on) = &*scene_state;
        let measurements = (0..scene_frame_indices.len())
            .map(|index| SceneMeasurement {
                scores:       scene_scores[index].clone(),
                started_on:   scene_started_on[index],
                completed_on: scene_completed_on[index],
            })
            .collect::<Vec<_>>();

        Ok(measurements)
    }
}

impl Default for QualityCheck {
    #[inline]
    fn default() -> Self {
        Self {
            input: None
        }
    }
}

struct SceneMeasurement {
    scores:       Vec<f64>,
    started_on:   u128,
    completed_on: u128,
}

#[derive(Debug, Clone, Error)]
pub enum QualityCheckError {
    #[error("No Scenes found")]
    ScenesEmpty,
    #[error("Output file not found: {path}")]
    OutputMissing { path: std::path::PathBuf },
    #[error(
        "Quality Check compares unfiltered reference frames against the Parallel Encoder's \
         filtered output; picture-altering filters (crop, invert, ...) make the comparison \
         unrepresentative"
    )]
    FilteredEncoderOutput,
    #[error("Failed to measure quality")]
    QualityMeasurementFailed,
    #[error("Cancelled")]
    Cancelled,
}
