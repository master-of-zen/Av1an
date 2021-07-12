#![warn(clippy::needless_pass_by_value)]

#[macro_use]
extern crate log;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use sysinfo::SystemExt;
pub mod concat;
pub mod ffmpeg;
pub mod file_validation;
pub mod logger;
pub mod progress_bar;
pub mod split;
pub mod target_quality;
pub mod vapoursynth;
pub mod vmaf;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum Encoder {
  aom,
  rav1e,
  libvpx,
  svt_av1,
  x264,
  x265,
}

impl FromStr for Encoder {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    // set to match usage in python code
    match s {
      "aom" => Ok(Self::aom),
      "rav1e" => Ok(Self::rav1e),
      "vpx" => Ok(Self::libvpx),
      "svt_av1" => Ok(Self::svt_av1),
      "x264" => Ok(Self::x264),
      "x265" => Ok(Self::x265),
      _ => Err(()),
    }
  }
}

// TODO
pub enum ConcatMethod {
  /// MKVToolNix
  MKVMerge,
  /// FFmpeg
  FFmpeg,
  /// Use native functions implemented in av1an if possible
  Native,
}

pub enum SplitMethod {
  PySceneDetect,
  AOMKeyframes,
  FFmpeg,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChunkMethod {
  Select,
  FFMS2,
  LSMASH,
  Hybrid,
}

impl FromStr for ChunkMethod {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    // set to match usage in python code
    match s {
      "vs_ffms2" => Ok(Self::FFMS2),
      "vs_lsmash" => Ok(Self::LSMASH),
      "hybrid" => Ok(Self::Hybrid),
      "select" => Ok(Self::Select),
      _ => Err(()),
    }
  }
}

#[allow(unused)]
pub struct EncodeConfig {
  frames: usize,
  counter: (),
  is_vs: bool,
  input: PathBuf,
  temp: PathBuf,
  output_file: PathBuf,

  concat_method: ConcatMethod,
  webm: (),
  chunk_method: ChunkMethod,
  scenes: PathBuf,
  split_method: SplitMethod,
  extra_split: usize,
  min_scene_len: usize,
  // PySceneDetect split
  threshold: f32,

  // Encoding
  passes: (),
  video_params: (),
  encoder: Encoder,
  workers: usize,

  // FFmpeg params
  ffmpeg_pipe: (),
  ffmpeg: (),
  audio_params: (),
  pix_format: (),

  quiet: bool,
  logging: (),
  resume: bool,
  keep: bool,
  force: bool,

  // VMAF
  vmaf: bool,
  vmaf_path: PathBuf,
  vmaf_res: (),

  // TODO refactor into VMAF struct, and this struct contains an Option<VMAF> or something
  // which indicates whether or not the encode uses target_quality/vmaf

  // except for the vmaf options which you can use regardless of target_quality, like the
  // vmaf plot option

  // Target quality
  target_quality: u8,
  probes: u16,
  min_q: u8,
  max_q: u8,
  vmaf_plots: bool,
  probing_rate: u32,
  n_threads: usize,
  vmaf_filter: (),
}

/// Check for FFmpeg
pub fn get_ffmpeg_info() -> String {
  let mut cmd = Command::new("ffmpeg");
  cmd.stderr(Stdio::piped());
  String::from_utf8(cmd.output().unwrap().stderr).unwrap()
}

pub fn adapt_probing_rate(rate: usize) -> usize {
  match rate {
    1..=4 => rate,
    _ => 4,
  }
}

/// Determine the optimal number of workers for an encoder
pub fn determine_workers(encoder: Encoder) -> u64 {
  // TODO look for lighter weight solution? sys-info maybe?
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = num_cpus::get() as u64;
  // available_memory returns kb, convert to gb
  let ram_gb = system.available_memory() / 10u64.pow(6);

  std::cmp::max(
    match encoder {
      Encoder::aom | Encoder::rav1e | Encoder::libvpx => std::cmp::min(
        (cpu as f64 / 3.0).round() as u64,
        (ram_gb as f64 / 1.5).round() as u64,
      ),
      Encoder::svt_av1 | Encoder::x264 | Encoder::x265 => std::cmp::min(cpu, ram_gb) / 8,
    },
    1,
  )
}

pub fn get_percentile(scores: &mut [f64], percentile: f64) -> f64 {
  // Calculates percentile from vector of valuees
  scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

  let k = (scores.len() - 1) as f64 * percentile;
  let f = k.floor();
  let c = k.ceil();

  if f == c {
    return scores[k as usize];
  }

  let d0 = scores[f as usize] as f64 * (c - k);
  let d1 = scores[f as usize] as f64 * (k - f);

  d0 + d1
}

#[derive(Deserialize, Debug)]
struct Foo {
  vmaf: f64,
}

#[derive(Deserialize, Debug)]
struct Bar {
  metrics: Foo,
}

#[derive(Deserialize, Debug)]
struct Baz {
  frames: Vec<Bar>,
}

pub fn read_file_to_string(file: &Path) -> Result<String, Error> {
  Ok(fs::read_to_string(&file).unwrap_or_else(|_| panic!("Can't open file {:?}", file)))
}

pub fn read_vmaf_file(file: &Path) -> Result<Vec<f64>, serde_json::Error> {
  let json_str = read_file_to_string(&file).unwrap();
  let bazs = serde_json::from_str::<Baz>(&json_str)?;
  let v = bazs
    .frames
    .into_iter()
    .map(|x| x.metrics.vmaf)
    .collect::<Vec<_>>();

  Ok(v)
}

pub fn read_weighted_vmaf(file: &Path, percentile: f64) -> Result<f64, serde_json::Error> {
  let mut scores = read_vmaf_file(file).unwrap();

  Ok(get_percentile(&mut scores, percentile))
}
