use clap::AppSettings::ColoredHelp;
use clap::Clap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use av1an_core::{ChunkMethod, ConcatMethod, Encoder, SplitMethod};

/// Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with per scene quality encoding
#[derive(Clap, Debug, Serialize, Deserialize)]
#[clap(name = "av1an", setting = ColoredHelp, version)]
pub struct Args {
  /// Input file or vapoursynth (.py, .vpy) script
  #[clap(short, parse(from_os_str))]
  pub input: PathBuf,

  /// Temporary directory to use
  #[clap(long, parse(from_os_str))]
  pub temp: Option<PathBuf>,

  /// Specify output file
  #[clap(short, parse(from_os_str))]
  pub output_file: Option<PathBuf>,

  /// Concatenation method to use for splits
  #[clap(short, long, possible_values = &["ffmpeg", "mkvmerge", "ivf"], default_value = "ffmpeg")]
  pub concat: ConcatMethod,

  /// Disable printing progress to terminal
  #[clap(short, long)]
  pub quiet: bool,

  /// Print extra progress info and stats to terminal
  #[clap(long)]
  pub verbose: bool,

  /// Enable logging
  #[clap(short, long)]
  pub logging: Option<String>,

  /// Resume previous session
  #[clap(short, long)]
  pub resume: bool,

  /// Keep temporary folder after encode
  #[clap(long)]
  pub keep: bool,

  /// Output to webm
  #[clap(long)]
  pub webm: bool,

  /// Method for creating chunks
  #[clap(short = 'm', long, possible_values=&["segment", "select", "ffms2", "lsmash", "hybrid"])]
  pub chunk_method: Option<ChunkMethod>,

  /// File location for scenes
  #[clap(short, long, parse(from_os_str))]
  pub scenes: Option<PathBuf>,

  /// Specify splitting method
  #[clap(long, possible_values=&["av-scenechange", "none"], default_value = "av-scenechange")]
  pub split_method: SplitMethod,

  /// Number of frames after which make split
  #[clap(short = 'x', long, default_value = "240")]
  pub extra_split: usize,

  /// Minimum number of frames in a split
  #[clap(long, default_value = "60")]
  pub min_scene_len: usize,

  /// Specify encoding passes
  #[clap(short, long)]
  pub passes: Option<u8>,

  /// Parameters passed to the encoder
  #[clap(short, long)]
  pub video_params: Option<String>,

  #[clap(short, long, default_value = "aom", possible_values=&["aom", "rav1e", "vpx", "svt-av1", "x264", "x265"])]
  pub encoder: Encoder,

  /// Number of workers
  #[clap(short, long, default_value = "0")]
  pub workers: usize,

  /// Force encoding if input args seen as invalid
  #[clap(long)]
  pub force: bool,

  /// FFmpeg commands
  #[clap(short = 'f', long)]
  pub ffmpeg: Option<String>,

  /// FFmpeg commands
  #[clap(short, long)]
  pub audio_params: Option<String>,

  /// FFmpeg pixel format
  #[clap(long, default_value = "yuv420p10le")]
  pub pix_format: String,

  /// Calculate VMAF after encode
  #[clap(long)]
  pub vmaf: bool,

  /// Path to VMAF models
  #[clap(long, parse(from_os_str))]
  pub vmaf_path: Option<PathBuf>,

  /// Resolution used in VMAF calculation
  #[clap(long, default_value = "1920x1080")]
  pub vmaf_res: String,

  /// Number of threads to use for VMAF calculation
  #[clap(long)]
  pub vmaf_threads: Option<u32>,

  /// Value to target
  #[clap(long)]
  pub target_quality: Option<f32>,

  /// Method selection for target quality
  #[clap(long, possible_values = &["per_shot"], default_value = "per_shot")]
  pub target_quality_method: String,

  /// Number of probes to make for target_quality
  #[clap(long, default_value = "4")]
  pub probes: u32,

  /// Framerate for probes, 1 - original
  #[clap(long, default_value = "4")]
  pub probing_rate: u32,

  /// Use encoding settings for probes
  #[clap(long)]
  pub probe_slow: bool,

  /// Min q for target_quality
  #[clap(long)]
  pub min_q: Option<u32>,

  /// Max q for target_quality
  #[clap(long)]
  pub max_q: Option<u32>,

  /// Filter applied to source at vmaf calcualation, use if you crop source
  #[clap(long)]
  pub vmaf_filter: Option<String>,
}
