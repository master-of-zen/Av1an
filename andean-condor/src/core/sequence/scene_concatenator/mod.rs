use std::{
    ffi::OsString,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{self, Arc, Mutex, atomic::AtomicBool},
    thread,
};

use anyhow::{Context, Result, bail};
use av_format::{
    buffer::AccReader,
    demuxer::{Context as DemuxerContext, Event},
    muxer::{Context as MuxerContext, Writer},
    rational::Ratio,
};
use av_ivf::{demuxer::IvfDemuxer, muxer::IvfMuxer};
use thiserror::Error;
use tracing::{error, trace};

use crate::{
    core::{
        Condor,
        input::Input,
        sequence::{
            Sequence,
            SequenceCompletion,
            SequenceDetails,
            SequenceStatus,
            Status,
            parallel_encoder::ParallelEncoder,
        },
    },
    models::sequence::{
        SequenceConfigHandler,
        SequenceDataHandler,
        scene_concatenator::{
            Chapters,
            ConcatMethod,
            FfmpegConfig,
            FfmpegTrack,
            FfmpegTrackMap,
            FfmpegTrackOptions,
            Metadata,
            MkvmergeConfig,
            MkvmergeExtraInput,
            SceneConcatenatorConfigHandler,
            mkvmerge::tags_xml,
        },
    },
};

mod ffmpeg;
mod ivf;
mod mkvmerge;

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Scene Concatenator",
    description: "Concatenates encoded scenes into a single output file",
    version:     "0.0.1",
};
#[derive(Default)]
pub struct SceneConcatenator {}
impl<DataHandler, ConfigHandler> Sequence<DataHandler, ConfigHandler> for SceneConcatenator
where
    DataHandler: SequenceDataHandler,
    ConfigHandler: SequenceConfigHandler + SceneConcatenatorConfigHandler,
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
        let concatenator = condor.sequence_config.scene_concatenator()?;
        let mut warnings = Vec::new();
        match &concatenator.method {
            ConcatMethod::MKVMerge(mkvmerge) => {
                if which::which("mkvmerge").is_err() {
                    bail!(SceneConcatenatorError::MKVMergeNotInstalled);
                }
                warnings.extend(mkvmerge.validate()?);
            },
            ConcatMethod::FFmpeg(ffmpeg) => {
                if which::which("ffmpeg").is_err() {
                    bail!(SceneConcatenatorError::FFmpegNotInstalled);
                }
                let has_input = matches!(
                    condor.input,
                    Input::Video { .. } | Input::VapourSynth { .. }
                );
                if !has_input && ffmpeg.extra_inputs.is_empty() {
                    warnings.push(SceneConcatenatorError::NoFfmpegInputs.into());
                }
                warnings.extend(ffmpeg.validate()?);
            },
            ConcatMethod::Ivf => (),
        }

        Ok(((), warnings))
    }

    #[inline]
    fn initialize(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let mut warnings = vec![];

        let scenes_directory = &condor.sequence_config.scene_concatenator()?.scenes_directory;
        if !scenes_directory.exists() {
            bail!(SceneConcatenatorError::ScenesDirectoryMissing {
                path: scenes_directory.clone(),
            });
        }
        if !scenes_directory.is_dir() {
            bail!(SceneConcatenatorError::ScenesDirectoryInvalid {
                path: scenes_directory.clone(),
            });
        }
        let scratch_directory = Self::scratch_directory(scenes_directory.as_path());
        if !scratch_directory.exists() {
            std::fs::create_dir_all(scratch_directory)?;
        }

        let scene_files = condor
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| {
                let path = scenes_directory.join(format!(
                    "{}.{}",
                    ParallelEncoder::scene_id(index),
                    scene.encoder.output_extension()
                ));
                let exists = path.exists();

                (index, path, exists)
            })
            .filter(|(_, _, exists)| !*exists)
            .collect::<Vec<_>>();

        if !scene_files.is_empty() {
            warnings.push(anyhow::Error::new(
                SceneConcatenatorError::SceneFilesMissing {
                    scenes: scene_files.iter().map(|(index, _, _)| *index).collect(),
                },
            ));
        }

        Ok(((), warnings))
    }

    #[inline]
    fn execute(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
        progress_tx: sync::mpsc::Sender<SequenceStatus>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        let framerate = condor.input.clip_info()?.frame_rate;
        let input_path = {
            match &condor.input {
                Input::Video {
                    path, ..
                }
                | Input::VapourSynth {
                    path, ..
                } => Some(path.as_path()),
                Input::VapourSynthScript {
                    ..
                } => None, // May be invalid/Optional in the future
            }
        };
        let config = condor.sequence_config.scene_concatenator()?;
        let scenes = condor
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| {
                let path = config.scenes_directory.join(format!(
                    "{}.{}",
                    ParallelEncoder::scene_id(index),
                    scene.encoder.output_extension()
                ));
                let exists = path.exists();

                (index, scene, path, exists)
            })
            .filter(|(_, _, _, exists)| *exists)
            .collect::<Vec<_>>();

        let total_frames = scenes.iter().fold(0, |acc, (_, scene, _, _)| {
            acc + (scene.end_frame - scene.start_frame)
        });
        let scene_paths = scenes.iter().map(|(_, _, path, _)| path.clone()).collect::<Vec<_>>();

        match &config.method {
            ConcatMethod::MKVMerge(mkvmerge) => {
                Self::mkvmerge(
                    &config.scenes_directory,
                    &condor.output.path,
                    &scene_paths,
                    input_path,
                    framerate,
                    Some(mkvmerge),
                    &progress_tx,
                    &cancelled,
                )?;
            },
            ConcatMethod::FFmpeg(ffmpeg) => {
                Self::ffmpeg(
                    &config.scenes_directory,
                    &condor.output.path,
                    &scene_paths,
                    input_path,
                    total_frames,
                    framerate,
                    Some(ffmpeg),
                    &progress_tx,
                    &cancelled,
                )?;
            },
            ConcatMethod::Ivf => {
                Self::ivf(&condor.output.path, &scene_paths, &progress_tx, &cancelled)?;
            },
        };

        progress_tx.send(SequenceStatus::Whole(Status::Completed {
            id: DETAILS.name.to_owned(),
        }))?;

        Ok(((), warnings))
    }
}
impl SceneConcatenator {
    pub const DETAILS: SequenceDetails = DETAILS;

    #[inline]
    fn send_progress(progress_tx: &sync::mpsc::Sender<SequenceStatus>, percentage: f64) {
        if !percentage.is_finite() {
            return;
        }
        let _ = progress_tx.send(SequenceStatus::Whole(Status::Processing {
            id:         DETAILS.name.to_owned(),
            completion: SequenceCompletion::Percentage(percentage.clamp(0.0, 100.0)),
        }));
    }

    pub(crate) fn scratch_directory(scenes_directory: &Path) -> PathBuf {
        scenes_directory.join("Scene Concatenator")
    }
}
#[derive(Debug, Error)]
pub enum SceneConcatenatorError {
    #[error("mkvmerge not installed")]
    MKVMergeNotInstalled,
    #[error("FFmpeg not installed")]
    FFmpegNotInstalled,
    #[error(
        "FFmpeg output settings configure no additional inputs; the output will contain only the \
         encoded video"
    )]
    NoFfmpegInputs,
    #[error("Missing scene files: {scenes:?}")]
    SceneFilesMissing { scenes: Vec<usize> },
    #[error("Missing scenes directory: {path}")]
    ScenesDirectoryMissing { path: PathBuf },
    #[error("Scenes directory is not a directory: {path}")]
    ScenesDirectoryInvalid { path: PathBuf },
    #[error("Failed to concatenate with mkvmerge: {status}\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}")]
    MkvmergeFailed {
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
    #[error("Failed to concatenate with ffmpeg: {status}\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}")]
    FfmpegFailed {
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
}
