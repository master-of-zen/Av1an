use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::models::sequence::SequenceConfigHandler;

pub mod common;
pub mod ffmpeg;
pub mod mkvmerge;

pub use common::{Chapters, Metadata};
pub use ffmpeg::{
    FfmpegAttachmentTrack,
    FfmpegConfig,
    FfmpegExtraInput,
    FfmpegTrack,
    FfmpegTrackKind,
    FfmpegTrackMap,
    FfmpegTrackOptions,
};
pub use mkvmerge::{
    AttachmentTrack,
    AudioTrack,
    Compression,
    MkvmergeConfig,
    MkvmergeExtraInput,
    MkvmergeTrack,
    MkvmergeTrackKind,
    SubtitleTrack,
    TrackMap,
    TrackOptions,
    VideoTrack,
};

pub trait SceneConcatenatorConfigHandler
where
    Self: SequenceConfigHandler,
{
    fn scene_concatenator(&self) -> Result<&SceneConcatenatorConfig>;
    fn scene_concatenator_mut(&mut self) -> Result<&mut SceneConcatenatorConfig>;
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SceneConcatenatorConfig
where
    Self: SequenceConfigHandler,
{
    /// The concatenation method and its output settings.
    pub method:           ConcatMethod,
    pub scenes_directory: PathBuf,
    pub output:           Option<PathBuf>,
}

impl Default for SceneConcatenatorConfig {
    #[inline]
    fn default() -> Self {
        Self {
            method:           ConcatMethod::default(),
            scenes_directory: PathBuf::new(),
            output:           None,
        }
    }
}

impl SceneConcatenatorConfig {
    #[inline]
    pub fn new(scenes_directory: &Path) -> Self {
        Self {
            scenes_directory: scenes_directory.to_path_buf(),
            ..Default::default()
        }
    }
}

impl SequenceConfigHandler for SceneConcatenatorConfig {
}

/// The concatenation method and its output settings.
///
/// Each variant carries that muxer's track, chapter, and metadata settings, so
/// the settings live on the method itself rather than in separate properties
/// that could disagree with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ConcatMethod {
    /// MKVToolNix mkvmerge, with its track/chapter/metadata settings.
    MKVMerge(MkvmergeConfig),
    /// FFmpeg, with its track/metadata settings and audio/subtitle filters.
    FFmpeg(FfmpegConfig),
    /// IVF; video only.
    Ivf,
}

impl Default for ConcatMethod {
    #[inline]
    fn default() -> Self {
        Self::MKVMerge(MkvmergeConfig::default())
    }
}

impl Display for ConcatMethod {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::MKVMerge(_) => "mkvmerge",
            Self::FFmpeg(_) => "ffmpeg",
            Self::Ivf => "ivf",
        };
        f.write_str(name)
    }
}

impl ConcatMethod {
    /// This method's mkvmerge settings, or `None` for the other methods.
    #[inline]
    #[must_use]
    pub fn mkvmerge(&self) -> Option<&MkvmergeConfig> {
        match self {
            Self::MKVMerge(config) => Some(config),
            _ => None,
        }
    }

    /// This method's FFmpeg settings, or `None` for the other methods.
    #[inline]
    #[must_use]
    pub fn ffmpeg(&self) -> Option<&FfmpegConfig> {
        match self {
            Self::FFmpeg(config) => Some(config),
            _ => None,
        }
    }

    #[inline]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::MKVMerge(_) => "mkv",
            Self::FFmpeg(_) => "mkv",
            Self::Ivf => "ivf",
        }
    }

    #[inline]
    pub fn with_extension(&self, path: &Path) -> PathBuf {
        let mut path = path.to_path_buf();
        path.set_extension(self.extension());
        path
    }
}
