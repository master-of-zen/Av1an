#![allow(unused)]

#[macro_use]
extern crate log;
extern crate av_format;
extern crate av_ivf;
extern crate failure;

use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{fs::File, io::Write};
use sysinfo::SystemExt;

mod concat;
pub mod vapoursynth;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum Encoder {
  libaom,
  rav1e,
  libvpx,
  SvtAv1,
  SvtVp9,
  x264,
  x265,
}

impl FromStr for Encoder {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    // set to match usage in python code
    match s {
      "aom" => Ok(Self::libaom),
      "rav1e" => Ok(Self::rav1e),
      "vpx" => Ok(Self::libvpx),
      "svt_av1" => Ok(Self::SvtAv1),
      "svt_vp9" => Ok(Self::SvtVp9),
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

pub struct EncodeConfig {
  frames: usize,
  counter: (),
  is_vs: bool,
  input: PathBuf,
  temp: PathBuf,
  output_file: PathBuf,

  concat_method: ConcatMethod,
  config: (),
  webm: (),
  chunk_method: ChunkMethod,
  scenes: PathBuf,
  split_method: SplitMethod,
  extra_split: usize,
  min_scene_len: usize,
  // PySceneDetect split
  threshold: f32,

  // TODO refactor, this should really be in the enum of each encoder
  reuse_first_pass: bool,

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
  no_check: bool,
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

/// Get frame count. Direct counting of frame count using ffmpeg
pub fn ffmpeg_get_frame_count(source: &Path) -> usize {
  let source_path = Path::new(&source);

  let mut cmd = Command::new("ffmpeg");
  cmd.args(&[
    "-hide_banner",
    "-i",
    source_path.to_str().unwrap(),
    "-map",
    "0:v:0",
    "-c",
    "copy",
    "-f",
    "null",
    "-",
  ]);

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output().unwrap();
  assert!(out.status.success());

  let re = Regex::new(r".*frame=\s*([0-9]+)\s").unwrap();
  let output = String::from_utf8(out.stderr).unwrap();

  let cap = re.captures(&output).unwrap();

  let frame_count = cap[cap.len() - 1].parse::<usize>().unwrap();
  frame_count
}

/// Determine the optimal number of workers for an encoder
pub fn determine_workers(encoder: Encoder) -> u64 {
  // TODO look for lighter weight solution? sys-info maybe?
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = num_cpus::get() as u64;
  // get_total_memory returns kb, convert to gb
  let ram_gb = system.get_total_memory() / 10u64.pow(6);

  std::cmp::max(
    match encoder {
      Encoder::libaom | Encoder::rav1e | Encoder::libvpx => std::cmp::min(
        (cpu as f64 / 3.0).round() as u64,
        (ram_gb as f64 / 1.5).round() as u64,
      ),
      Encoder::SvtAv1 | Encoder::SvtVp9 | Encoder::x264 | Encoder::x265 => {
        std::cmp::min(cpu, ram_gb) / 8
      }
    },
    1,
  )
}
