use clap::AppSettings::ColoredHelp;
use clap::Clap;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

// Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with per scene quality encoding
#[derive(Clap, Debug, Serialize, Deserialize)]
#[clap(name = "av1an", setting = ColoredHelp, version)]
pub struct Args {
  /// Input file or vapoursynth (.py, .vpy) script
  #[clap(short, long, parse(from_os_str))]
  input: Option<PathBuf>,

  /// Temporary directory to use
  #[clap(long, parse(from_os_str))]
  temp: Option<PathBuf>,

  /// Specify output file
  #[clap(short, long, parse(from_os_str))]
  output_file: Option<PathBuf>,

  /// Concatenation method to use for splits
  #[clap(short, long, possible_values = &["ffmpeg", "mkvmerge", "ivf"], default_value = "ffmpeg")]
  concat: String,

  /// Disable printing progress to terminal
  #[clap(short, long)]
  quiet: bool,

  /// Enable logging
  #[clap(short, long)]
  logging: Option<String>,

  /// Resume previous session
  #[clap(short, long)]
  resume: bool,

  /// Keep temporary folder after encode
  #[clap(long)]
  keep: bool,

  /// Output to webm
  #[clap(long)]
  webm: bool,

  /// Method for creating chunks
  #[clap(short = 'm', long, possible_values=&["segment", "select", "vs_ffms2", "vs_lsmash", "hybrid"])]
  chunk_method: Option<String>,

  /// File location for scenes
  #[clap(short, long, parse(from_os_str))]
  scenes: Option<PathBuf>,

  /// Specify splitting method
  #[clap(long, possible_values = &["ffmpeg", "pyscene", "aom_keyframes"])]
  split_method: Option<String>,

  /// Number of frames after which make split
  #[clap(short = 'x', long, default_value = "240")]
  extra_split: usize,

  /// PySceneDetect Threshold
  #[clap(short, long, default_value = "35.0")]
  threshold: f64,

  /// Minimum number of frames in a split
  #[clap(long, default_value = "60")]
  min_scene_len: usize,

  /// Specify encoding passes
  #[clap(short, long)]
  passes: Option<u8>,

  /// Parameters passed to the encoder
  #[clap(short, long)]
  video_params: Option<String>,

  #[clap(short, long, default_value = "aom", possible_values=&["aom", "rav1e", "vpx", "svt_av1", "x264", "x265"])]
  encoder: String,

  /// Number of workers
  #[clap(short, long, default_value = "0")]
  workers: usize,

  /// Do not check encodings
  #[clap(long)]
  no_check: bool,

  /// Force encoding if input args seen as invalid
  #[clap(long)]
  force: bool,

  /// FFmpeg commands
  #[clap(short = 'f', long)]
  ffmpeg: Option<String>,

  /// FFmpeg audio parameters
  #[clap(short, long, default_value = "-c:a copy")]
  audio_params: String,

  /// FFmpeg pixel format
  #[clap(long, default_value = "yuv420p10le")]
  pix_format: String,

  /// Calculate VMAF after encode
  #[clap(long)]
  vmaf: bool,

  /// Path to VMAF models
  #[clap(long, parse(from_os_str))]
  vmaf_path: Option<PathBuf>,

  /// Resolution used in VMAF calculation
  #[clap(long, default_value = "1920x1080")]
  vmaf_res: String,

  /// Number of threads to use for VMAF calculation
  #[clap(long)]
  vmaf_threads: Option<usize>,

  /// Value to target
  #[clap(short, long)]
  target_quality: Option<f64>,

  /// Method selection for target quality
  #[clap(long, possible_values = &[ "per_shot"], default_value = "per_shot")]
  target_quality_method: String,

  /// Number of probes to make for target_quality
  #[clap(long, default_value = "4")]
  probes: usize,

  /// Use encoding settings for probes
  #[clap(long)]
  probe_slow: bool,

  /// Framerate for probes, 1 - original
  #[clap(long, default_value = "4")]
  probing_rate: usize,

  /// Min q for target_quality
  #[clap(long)]
  min_q: Option<u8>,

  /// Max q for target_quality
  #[clap(long)]
  max_q: Option<u8>,

  /// Make plots of probes in temp folder
  #[clap(long)]
  vmaf_plots: bool,

  /// Filter applied to source at vmaf calcualation, use if you crop source
  #[clap(long)]
  vmaf_filter: Option<String>,
}

/// Parse args
pub fn parse_args() -> String {
  let commands = env::args();
  let cmds: Vec<String> = commands.into_iter().collect();
  let parsed = Args::parse_from(&cmds[1..]);
  serde_json::to_string(&parsed).unwrap()
}

/// Get default values of args
pub fn default_args() -> String {
  serde_json::to_string(&Args::parse_from(&["av1an"])).unwrap()
}
