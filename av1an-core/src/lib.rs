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

#[macro_use]
extern crate log;

use crate::{
  encoder::Encoder,
  progress_bar::{finish_multi_progress_bar, finish_progress_bar},
  target_quality::TargetQuality,
};
use chunk::Chunk;
use dashmap::DashMap;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::{
  collections::hash_map::DefaultHasher,
  fs,
  fs::File,
  hash::{Hash, Hasher},
  io::Write,
  path::{Path, PathBuf},
  string::ToString,
  sync::atomic::{AtomicBool, AtomicUsize},
  time::Instant,
};
use sysinfo::SystemExt;

pub mod broker;
pub mod chunk;
pub mod concat;
pub mod encoder;
pub mod ffmpeg;
pub mod progress_bar;
pub mod scene_detect;
pub mod settings;
pub mod split;
pub mod target_quality;
pub mod util;
pub mod vapoursynth;
pub mod vmaf;

#[derive(Debug)]
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

  pub fn frames(&self) -> usize {
    const FAIL_MSG: &str = "failed to get number of frames for input video";
    match &self {
      Input::Video(path) => ffmpeg::num_frames(path.as_path()).expect(FAIL_MSG),
      Input::VapourSynth(path) => vapoursynth::num_frames(path.as_path()).expect(FAIL_MSG),
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

/// Concurrent data structure for keeping track of the finished
/// chunks in an encode
#[derive(Debug, Deserialize, Serialize)]
struct DoneJson {
  frames: AtomicUsize,
  done: DashMap<String, usize>,
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

#[derive(Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum SplitMethod {
  #[strum(serialize = "av-scenechange")]
  AvScenechange,
  #[strum(serialize = "none")]
  None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, strum::EnumString, strum::IntoStaticStr)]
pub enum ScenecutMethod {
  #[strum(serialize = "fast")]
  Fast,
  #[strum(serialize = "standard")]
  Standard,
}

#[derive(
  PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr,
)]
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

/// Determine the optimal number of workers for an encoder
#[must_use]
pub fn determine_workers(encoder: Encoder) -> u64 {
  // TODO look for lighter weight solution? sys-info maybe?
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = num_cpus::get() as u64;
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

fn save_chunk_queue(temp: &str, chunk_queue: &[Chunk]) {
  let mut file = File::create(Path::new(temp).join("chunks.json")).unwrap();

  file
    .write_all(serde_json::to_string(&chunk_queue).unwrap().as_bytes())
    .unwrap();
}

#[derive(Clone, Copy, PartialEq)]
pub enum Verbosity {
  Verbose,
  Normal,
  Quiet,
}

fn read_chunk_queue(temp: &Path) -> Vec<Chunk> {
  let contents = fs::read_to_string(Path::new(temp).join("chunks.json")).unwrap();

  serde_json::from_str(&contents).unwrap()
}
