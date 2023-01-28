#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::if_not_else)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::drop_ref)]
#![allow(clippy::unsafe_derive_deserialize)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::use_self)]

#[macro_use]
extern crate log;

use std::cmp::max;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::string::ToString;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::thread::available_parallelism;
use std::time::Instant;

use ::ffmpeg::color::TransferCharacteristic;
use anyhow::Context;
use av1_grain::TransferFunction;
use chunk::Chunk;
use dashmap::DashMap;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};
use sysinfo::SystemExt;

use crate::encoder::Encoder;
use crate::progress_bar::{finish_multi_progress_bar, finish_progress_bar};
use crate::target_quality::TargetQuality;

pub mod broker;
pub mod chunk;
pub mod concat;
pub mod encoder;
pub mod ffmpeg;
pub(crate) mod parse;
pub mod progress_bar;
pub mod scene_detect;
mod scenes;
pub mod settings;
pub mod split;
pub mod target_quality;
pub mod util;
pub mod vapoursynth;
pub mod vmaf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Input {
  VapourSynth(PathBuf),
  Video(PathBuf),
}

impl Input {
  /// Returns a reference to the inner path, panicking if the input is not an `Input::Video`.
  pub fn as_video_path(&self) -> &Path {
    match &self {
      Input::Video(path) => path.as_ref(),
      Input::VapourSynth(_) => {
        panic!("called `Input::as_video_path()` on an `Input::VapourSynth` variant")
      }
    }
  }

  /// Returns a reference to the inner path, panicking if the input is not an `Input::VapourSynth`.
  pub fn as_vapoursynth_path(&self) -> &Path {
    match &self {
      Input::VapourSynth(path) => path.as_ref(),
      Input::Video(_) => {
        panic!("called `Input::as_vapoursynth_path()` on an `Input::Video` variant")
      }
    }
  }

  /// Returns a reference to the inner path regardless of whether `self` is
  /// `Video` or `VapourSynth`.
  ///
  /// The caller must ensure that the input type is being properly handled.
  /// This method should not be used unless the code is TRULY agnostic of the
  /// input type!
  pub fn as_path(&self) -> &Path {
    match &self {
      Input::Video(path) | Input::VapourSynth(path) => path.as_ref(),
    }
  }

  pub const fn is_video(&self) -> bool {
    matches!(&self, Input::Video(_))
  }

  pub const fn is_vapoursynth(&self) -> bool {
    matches!(&self, Input::VapourSynth(_))
  }

  pub fn frames(&self) -> anyhow::Result<usize> {
    const FAIL_MSG: &str = "Failed to get number of frames for input video";
    Ok(match &self {
      Input::Video(path) => {
        ffmpeg::num_frames(path.as_path()).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
      Input::VapourSynth(path) => {
        vapoursynth::num_frames(path.as_path()).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
    })
  }

  pub fn frame_rate(&self) -> anyhow::Result<f64> {
    const FAIL_MSG: &str = "Failed to get frame rate for input video";
    Ok(match &self {
      Input::Video(path) => {
        crate::ffmpeg::frame_rate(path.as_path()).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
      Input::VapourSynth(path) => {
        vapoursynth::frame_rate(path.as_path()).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
    })
  }

  pub fn resolution(&self) -> anyhow::Result<(u32, u32)> {
    const FAIL_MSG: &str = "Failed to get resolution for input video";
    Ok(match self {
      Input::VapourSynth(video) => {
        crate::vapoursynth::resolution(video).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
      Input::Video(video) => {
        crate::ffmpeg::resolution(video).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
    })
  }

  pub fn pixel_format(&self) -> anyhow::Result<String> {
    const FAIL_MSG: &str = "Failed to get resolution for input video";
    Ok(match self {
      Input::VapourSynth(video) => {
        crate::vapoursynth::pixel_format(video).map_err(|_| anyhow::anyhow!(FAIL_MSG))?
      }
      Input::Video(video) => {
        let fmt = crate::ffmpeg::get_pixel_format(video).map_err(|_| anyhow::anyhow!(FAIL_MSG))?;
        format!("{fmt:?}")
      }
    })
  }

  fn transfer_function(&self) -> anyhow::Result<TransferFunction> {
    const FAIL_MSG: &str = "Failed to get transfer characteristics for input video";
    Ok(match self {
      Input::VapourSynth(video) => {
        match crate::vapoursynth::transfer_characteristics(video)
          .map_err(|_| anyhow::anyhow!(FAIL_MSG))?
        {
          16 => TransferFunction::SMPTE2084,
          _ => TransferFunction::BT1886,
        }
      }
      Input::Video(video) => {
        match crate::ffmpeg::transfer_characteristics(video)
          .map_err(|_| anyhow::anyhow!(FAIL_MSG))?
        {
          TransferCharacteristic::SMPTE2084 => TransferFunction::SMPTE2084,
          _ => TransferFunction::BT1886,
        }
      }
    })
  }

  pub fn transfer_function_params_adjusted(
    &self,
    enc_params: &[String],
  ) -> anyhow::Result<TransferFunction> {
    if enc_params.iter().any(|p| {
      let p = p.to_ascii_lowercase();
      p == "pq" || p.ends_with("=pq") || p.ends_with("smpte2084")
    }) {
      return Ok(TransferFunction::SMPTE2084);
    }
    if enc_params.iter().any(|p| {
      let p = p.to_ascii_lowercase();
      // If the user specified an SDR transfer characteristic, assume they want to encode to SDR.
      p.ends_with("bt709")
        || p.ends_with("bt.709")
        || p.ends_with("bt601")
        || p.ends_with("bt.601")
        || p.contains("smpte240")
        || p.contains("smpte170")
    }) {
      return Ok(TransferFunction::BT1886);
    }
    self.transfer_function()
  }

  /// Calculates tiles from resolution
  /// Don't convert tiles to encoder specific representation
  /// Default video without tiling is 1,1
  /// Return number of horizontal and vertical tiles
  pub fn calculate_tiles(&self) -> (u32, u32) {
    match self.resolution() {
      Ok((h, v)) => {
        // tile range 0-1440 pixels
        let horizontal = max((h - 1) / 720, 1);
        let vertical = max((v - 1) / 720, 1);

        (horizontal, vertical)
      }
      _ => (1, 1),
    }
  }
}

impl<P: AsRef<Path> + Into<PathBuf>> From<P> for Input {
  #[allow(clippy::option_if_let_else)]
  fn from(path: P) -> Self {
    if let Some(ext) = path.as_ref().extension() {
      if ext == "py" || ext == "vpy" {
        Self::VapourSynth(path.into())
      } else {
        Self::Video(path.into())
      }
    } else {
      Self::Video(path.into())
    }
  }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
struct DoneChunk {
  frames: usize,
  size_bytes: u64,
}

/// Concurrent data structure for keeping track of the finished chunks in an encode
#[derive(Debug, Deserialize, Serialize)]
struct DoneJson {
  frames: AtomicUsize,
  done: DashMap<String, DoneChunk>,
  audio_done: AtomicBool,
}

static DONE_JSON: OnceCell<DoneJson> = OnceCell::new();

// once_cell::sync::Lazy cannot be used here due to Lazy<T> not implementing
// Serialize or Deserialize, we need to get a reference directly to the global
// data
fn get_done() -> &'static DoneJson {
  DONE_JSON.get().unwrap()
}

fn init_done(done: DoneJson) -> &'static DoneJson {
  DONE_JSON.get_or_init(|| done)
}

pub fn list_index(params: &[impl AsRef<str>], is_match: fn(&str) -> bool) -> Option<usize> {
  assert!(!params.is_empty(), "received empty list of parameters");

  params.iter().enumerate().find_map(|(idx, s)| {
    if is_match(s.as_ref()) {
      Some(idx)
    } else {
      None
    }
  })
}

#[derive(Serialize, Deserialize, Debug, EnumString, IntoStaticStr, Display, Clone)]
pub enum SplitMethod {
  #[strum(serialize = "av-scenechange")]
  AvScenechange,
  #[strum(serialize = "none")]
  None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, EnumString, IntoStaticStr, Display)]
pub enum ScenecutMethod {
  #[strum(serialize = "fast")]
  Fast,
  #[strum(serialize = "standard")]
  Standard,
}

#[derive(PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, EnumString, IntoStaticStr)]
pub enum ChunkMethod {
  #[strum(serialize = "select")]
  Select,
  #[strum(serialize = "hybrid")]
  Hybrid,
  #[strum(serialize = "segment")]
  Segment,
  #[strum(serialize = "ffms2")]
  FFMS2,
  #[strum(serialize = "lsmash")]
  LSMASH,
}

#[derive(
  PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, Display, EnumString, IntoStaticStr,
)]
pub enum ChunkOrdering {
  #[strum(serialize = "long-to-short")]
  LongestFirst,
  #[strum(serialize = "short-to-long")]
  ShortestFirst,
  #[strum(serialize = "sequential")]
  Sequential,
  #[strum(serialize = "random")]
  Random,
}

/// Determine the optimal number of workers for an encoder
#[must_use]
pub fn determine_workers(encoder: Encoder) -> u64 {
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = available_parallelism()
    .expect("Unrecoverable: Failed to get thread count")
    .get() as u64;
  // available_memory returns kb, convert to gb
  let ram_gb = system.available_memory() / 10_u64.pow(6);

  std::cmp::max(
    match encoder {
      Encoder::aom | Encoder::rav1e | Encoder::vpx => std::cmp::min(
        (cpu as f64 / 3.0).round() as u64,
        (ram_gb as f64 / 1.5).round() as u64,
      ),
      Encoder::svt_av1 | Encoder::x264 | Encoder::x265 => std::cmp::min(cpu, ram_gb) / 8,
    },
    1,
  )
}

pub fn hash_path(path: &Path) -> String {
  let mut s = DefaultHasher::new();
  path.hash(&mut s);
  format!("{:x}", s.finish())[..7].to_string()
}

fn save_chunk_queue(temp: &str, chunk_queue: &[Chunk]) -> anyhow::Result<()> {
  let mut file = File::create(Path::new(temp).join("chunks.json"))
    .with_context(|| "Failed to create chunks.json file")?;

  file
    // serializing chunk_queue as json should never fail, so unwrap is OK here
    .write_all(serde_json::to_string(&chunk_queue).unwrap().as_bytes())
    .with_context(|| format!("Failed to write serialized chunk_queue data to {:?}", &file))?;

  Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
  Verbose,
  Normal,
  Quiet,
}

fn read_chunk_queue(temp: &Path) -> anyhow::Result<Vec<Chunk>> {
  let file = Path::new(temp).join("chunks.json");

  let contents = fs::read_to_string(&file)
    .with_context(|| format!("Failed to read chunk queue file {:?}", &file))?;

  Ok(serde_json::from_str(&contents)?)
}
