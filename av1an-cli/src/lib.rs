use anyhow::anyhow;
use anyhow::{bail, ensure};
use av1an_core::into_vec;
use av1an_core::Input;
use av1an_core::ScenecutMethod;
use path_abs::{PathAbs, PathInfo};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use structopt::{clap::AppSettings::ColoredHelp, StructOpt};

use av1an_core::{
  encoder::Encoder,
  hash_path,
  settings::EncodeArgs,
  vapoursynth, Verbosity,
  {concat::ConcatMethod, ChunkMethod, SplitMethod},
};

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
  #[structopt(long, possible_values=&["av-scenechange", "none"], default_value = "av-scenechange")]
  pub split_method: SplitMethod,

  /// Specify scenecut method
  ///
  /// Slow: Most accurate, but 5 times slower than medium
  /// Medium: Gives a reasonable amount of accuracy at a fairly quick speed
  /// Fast: Very fast, but less accurate
  #[structopt(long, possible_values=&["slow", "medium", "fast"], default_value = "medium")]
  pub sc_method: ScenecutMethod,

  /// Optional downscaling for scenecut detection,
  /// specify as the desired maximum height to scale to
  /// (e.g. "720" to downscale to 720p--this will leave lower resolution content untouched).
  /// Downscaling will improve speed but lower scenecut accuracy,
  /// especially when scaling to very low resolutions.
  #[structopt(long)]
  pub sc_downscale_height: Option<usize>,

  /// Number of frames after which make split
  /// Set to 0 to disable
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
  pub vmaf_threads: Option<usize>,

  /// Value to target
  #[structopt(long)]
  pub target_quality: Option<f64>,

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

fn confirm(prompt: &str) -> io::Result<bool> {
  let mut buf = String::with_capacity(4);
  let mut stdout = io::stdout();
  let stdin = io::stdin();
  loop {
    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;
    stdin.read_line(&mut buf)?;

    match buf.as_str().trim() {
      // allows enter to continue
      "y" | "Y" | "" => break Ok(true),
      "n" | "N" => break Ok(false),
      other => {
        println!("Sorry, response {:?} is not understood.", other);
        buf.clear();
        continue;
      }
    }
  }
}

pub fn cli() -> anyhow::Result<()> {
  let args = Args::from_args();

  let temp = if let Some(path) = args.temp {
    path.to_str().unwrap().to_owned()
  } else {
    format!(".{}", hash_path(args.input.as_path()))
  };

  // TODO parse with normal (non proc-macro) clap methods to simplify this
  // Unify Project/Args
  let mut encode = EncodeArgs {
    frames: 0,
    logging: if let Some(log_file) = args.logging {
      Path::new(&format!("{}.log", log_file)).to_owned()
    } else {
      Path::new(&temp).join("log.log")
    },
    ffmpeg: if let Some(args) = args.ffmpeg {
      shlex::split(&args).ok_or_else(|| anyhow!("Failed to split ffmpeg filter arguments"))?
    } else {
      Vec::new()
    },
    temp,
    force: args.force,
    passes: if let Some(passes) = args.passes {
      passes
    } else {
      args.encoder.get_default_pass()
    },
    video_params: if let Some(args) = args.video_params {
      shlex::split(&args).ok_or_else(|| anyhow!("Failed to split video encoder arguments"))?
    } else {
      Vec::new()
    },
    output_file: if let Some(path) = args.output_file.as_deref() {
      let path = PathAbs::new(path)?;

      if let Ok(parent) = path.parent() {
        ensure!(parent.exists(), "Path to file {:?} is invalid", path);
      } else {
        bail!("Failed to get parent directory of path: {:?}", path);
      }

      path.to_string_lossy().to_string()
    } else {
      format!(
        "{}_{}.mkv",
        args
          .input
          .file_stem()
          .unwrap_or(args.input.as_ref())
          .to_string_lossy(),
        args.encoder
      )
    },
    audio_params: if let Some(args) = args.audio_params {
      shlex::split(&args)
        .ok_or_else(|| anyhow!("Failed to split ffmpeg audio encoder arguments"))?
    } else {
      into_vec!["-c:a", "copy"]
    },
    ffmpeg_pipe: Vec::new(),
    chunk_method: args
      .chunk_method
      .unwrap_or_else(vapoursynth::best_available_chunk_method),
    concat: args.concat,
    encoder: args.encoder,
    extra_splits_len: if args.extra_split > 0 {
      Some(args.extra_split)
    } else {
      None
    },
    input: Input::from(args.input.as_path()),
    keep: args.keep,
    min_q: args.min_q,
    max_q: args.max_q,
    min_scene_len: args.min_scene_len,
    vmaf_threads: args.vmaf_threads,
    pix_format: args.pix_format,
    probe_slow: args.probe_slow,
    probes: args.probes,
    probing_rate: args.probing_rate,
    resume: args.resume,
    scenes: args.scenes,
    split_method: args.split_method,
    sc_method: args.sc_method,
    sc_downscale_height: args.sc_downscale_height,
    target_quality: args.target_quality,
    verbosity: if args.quiet {
      Verbosity::Quiet
    } else if args.verbose {
      Verbosity::Verbose
    } else {
      Verbosity::Normal
    },
    vmaf: args.vmaf,
    vmaf_filter: args.vmaf_filter,
    vmaf_path: args.vmaf_path,
    vmaf_res: args.vmaf_res,
    workers: args.workers,
  };

  ctrlc::set_handler(|| {
    println!("Stopped");
    std::process::exit(0);
  })
  .unwrap();

  encode.startup_check()?;

  if let Some(path) = args.output_file.as_deref() {
    if path.exists()
      && !confirm(&format!(
        "Output file {:?} exists. Do you want to overwrite it? [Y/n]: ",
        path
      ))?
    {
      println!("Not overwriting, aborting.");
      return Ok(());
    }
  } else {
    let path: &Path = encode.output_file.as_ref();

    if path.exists()
      && !confirm(&format!(
        "Default output file {:?} exists. Do you want to overwrite it? [Y/n]: ",
        path
      ))?
    {
      println!("Not overwriting, aborting.");
      return Ok(());
    }
  }

  encode.initialize()?;
  encode.encode_file()?;

  Ok(())
}
