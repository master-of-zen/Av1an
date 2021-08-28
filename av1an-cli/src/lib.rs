use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use structopt::clap::AppSettings::ColoredHelp;
use structopt::StructOpt;

use av1an_core::encoder::Encoder;
use av1an_core::{ChunkMethod, ConcatMethod, SplitMethod};

/// Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with per scene quality encoding
#[derive(StructOpt, Debug, Serialize, Deserialize)]
#[structopt(name = "av1an", setting = ColoredHelp)]
pub struct Args {
  /// Input file or vapoursynth (.py, .vpy) script
  #[structopt(short, parse(from_os_str))]
  pub input: PathBuf,

  /// Temporary directory to use
  #[structopt(long, parse(from_os_str))]
  pub temp: Option<PathBuf>,

  /// Specify output file
  #[structopt(short, parse(from_os_str))]
  pub output_file: Option<PathBuf>,

  /// Concatenation method to use for splits
  #[structopt(short, long, possible_values = &["ffmpeg", "mkvmerge", "ivf"], default_value = "ffmpeg")]
  pub concat: ConcatMethod,

  /// Disable printing progress to terminal
  #[structopt(short, long)]
  pub quiet: bool,

  /// Print extra progress info and stats to terminal
  #[structopt(long)]
  pub verbose: bool,

  /// Enable logging
  #[structopt(short, long)]
  pub logging: Option<String>,

  /// Resume previous session
  #[structopt(short, long)]
  pub resume: bool,

  /// Keep temporary folder after encode
  #[structopt(long)]
  pub keep: bool,

  /// Method for creating chunks
  #[structopt(short = "m", long, possible_values=&["segment", "select", "ffms2", "lsmash", "hybrid"])]
  pub chunk_method: Option<ChunkMethod>,

  /// File location for scenes
  #[structopt(short, long, parse(from_os_str))]
  pub scenes: Option<PathBuf>,

  /// Specify splitting method
  #[structopt(long, possible_values=&["av-scenechange", "av-scenechange-fast", "none"], default_value = "av-scenechange")]
  pub split_method: SplitMethod,

  /// Number of frames after which make split
  #[structopt(short = "x", long, default_value = "240")]
  pub extra_split: usize,

  /// Minimum number of frames in a split
  #[structopt(long, default_value = "60")]
  pub min_scene_len: usize,

  /// Specify encoding passes
  #[structopt(short, long)]
  pub passes: Option<u8>,

  /// Parameters passed to the encoder
  #[structopt(short, long)]
  pub video_params: Option<String>,

  #[structopt(short, long, default_value = "aom", possible_values=&["aom", "rav1e", "vpx", "svt-av1", "x264", "x265"])]
  pub encoder: Encoder,

  /// Number of workers
  #[structopt(short, long, default_value = "0")]
  pub workers: usize,

  /// Force encoding if input args seen as invalid
  #[structopt(long)]
  pub force: bool,

  /// FFmpeg commands
  #[structopt(short = "f", long)]
  pub ffmpeg: Option<String>,

  /// FFmpeg commands
  #[structopt(short, long)]
  pub audio_params: Option<String>,

  /// FFmpeg pixel format
  #[structopt(long, default_value = "yuv420p10le")]
  pub pix_format: String,

  /// Calculate VMAF after encode
  #[structopt(long)]
  pub vmaf: bool,

  /// Path to VMAF models
  #[structopt(long, parse(from_os_str))]
  pub vmaf_path: Option<PathBuf>,

  /// Resolution used in VMAF calculation
  #[structopt(long, default_value = "1920x1080")]
  pub vmaf_res: String,

  /// Number of threads to use for VMAF calculation
  #[structopt(long)]
  pub vmaf_threads: Option<u32>,

  /// Value to target
  #[structopt(long)]
  pub target_quality: Option<f32>,

  /// Method selection for target quality
  #[structopt(long, possible_values = &["per_shot"], default_value = "per_shot")]
  pub target_quality_method: String,

  /// Number of probes to make for target_quality
  #[structopt(long, default_value = "4")]
  pub probes: u32,

  /// Framerate for probes, 1 - original
  #[structopt(long, default_value = "4")]
  pub probing_rate: u32,

  /// Use encoding settings for probes
  #[structopt(long)]
  pub probe_slow: bool,

  /// Min q for target_quality
  #[structopt(long)]
  pub min_q: Option<u32>,

  /// Max q for target_quality
  #[structopt(long)]
  pub max_q: Option<u32>,

  /// Filter applied to source at vmaf calcualation, use if you crop source
  #[structopt(long)]
  pub vmaf_filter: Option<String>,
}
