use anyhow::anyhow;
use anyhow::{bail, ensure};
use av1an_core::into_vec;
use av1an_core::settings::PixelFormat;
use av1an_core::Input;
use av1an_core::ScenecutMethod;
use ffmpeg_next::format::Pixel;
use path_abs::{PathAbs, PathInfo};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use structopt::{clap::AppSettings::ColoredHelp, StructOpt};

use av1an_core::{
  encoder::Encoder,
  hash_path,
  settings::EncodeArgs,
  vapoursynth, Verbosity,
  {concat::ConcatMethod, ChunkMethod, SplitMethod},
};

pub fn version() -> &'static str {
  // This string has to be constructed at compile-time,
  // since structopt requires a &'static str
  concat!(
    env!("VERGEN_BUILD_SEMVER"),
    " (rev ",
    env!("VERGEN_GIT_SHA_SHORT"),
    ") (",
    env!("VERGEN_CARGO_PROFILE"),
    ")",
    "\n\n* Compiler\n  rustc ",
    env!("VERGEN_RUSTC_SEMVER"),
    " (LLVM ",
    env!("VERGEN_RUSTC_LLVM_VERSION"),
    ")\n\n* Target Triple\n  ",
    env!("VERGEN_CARGO_TARGET_TRIPLE"),
    "\n\n* Date Info",
    "\n   Build Date:  ",
    env!("VERGEN_BUILD_DATE"),
    "\n  Commit Date:  ",
    env!("VERGEN_GIT_COMMIT_DATE"),
  )
}

/// Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with per-scene quality encoding
#[derive(StructOpt, Debug)]
#[structopt(name = "av1an", setting = ColoredHelp, version = version())]
pub struct CliOpts {
  /// Input file or vapoursynth (.py, .vpy) script
  #[structopt(short, parse(from_os_str))]
  pub input: PathBuf,

  /// Temporary directory to use
  #[structopt(long, parse(from_os_str))]
  pub temp: Option<PathBuf>,

  /// Specify output file
  #[structopt(short, parse(from_os_str))]
  pub output_file: Option<PathBuf>,

  /// Method to use for concatenating encoded chunks
  #[structopt(short, long, possible_values = &["ffmpeg", "mkvmerge", "ivf"], default_value = "ffmpeg")]
  pub concat: ConcatMethod,

  /// Disable printing progress to the terminal
  #[structopt(short, long)]
  pub quiet: bool,

  /// Print extra progress info and stats to terminal
  #[structopt(long)]
  pub verbose: bool,

  /// Specify this option to log to a non-default file
  #[structopt(short, long)]
  pub logging: Option<String>,

  /// Resume previous session
  #[structopt(short, long)]
  pub resume: bool,

  /// Do not delete the temporary folder after encoding has finished
  #[structopt(short, long)]
  pub keep: bool,

  /// Method for creating chunks
  #[structopt(short = "m", long, possible_values=&["segment", "select", "ffms2", "lsmash", "hybrid"])]
  pub chunk_method: Option<ChunkMethod>,

  /// File location for scenes
  #[structopt(short, long, parse(from_os_str))]
  pub scenes: Option<PathBuf>,

  /// Method used to detect scenecuts. `av-scenechange` uses an algorithm to analyze which frames of
  /// the video are the start of new scenes, while `none` disable scene detection entirely (and only
  /// rely on `-x`/`--extra-split` to add extra scenecuts).
  #[structopt(long, possible_values=&["av-scenechange", "none"], default_value = "av-scenechange")]
  pub split_method: SplitMethod,

  /// Specify scenecut method
  ///
  /// Standard: Most accurate, still reasonably fast.
  /// Fast: Very fast, but less accurate.
  #[structopt(long, possible_values=&["standard", "fast"], default_value = "standard")]
  pub sc_method: ScenecutMethod,

  /// Optional downscaling for scenecut detection.
  /// Specify as the desired maximum height to scale to
  /// (e.g. "720" to downscale to 720p--this will leave lower resolution content untouched).
  /// Downscaling will improve speed but lower scenecut accuracy,
  /// especially when scaling to very low resolutions.
  #[structopt(long)]
  pub sc_downscale_height: Option<usize>,

  /// Maximum scene length
  ///
  /// When a scenecut is found whose distance to the previous scenecut is greater than the value
  /// specified by this option, one or more extra splits (scenecuts) are added. Set this option
  /// to 0 to disable adding extra splits.
  #[structopt(short = "x", long, default_value = "240")]
  pub extra_split: usize,

  /// Minimum number of frames for a scenecut
  #[structopt(long, default_value = "60")]
  pub min_scene_len: usize,

  /// Specify number encoding passes
  ///
  /// When using vpx or aom with RT, set this option to 1.
  #[structopt(short, long)]
  pub passes: Option<u8>,

  /// Video encoder parameters
  #[structopt(short, long)]
  pub video_params: Option<String>,

  /// Video encoder to use
  #[structopt(short, long, default_value = "aom", possible_values=&["aom", "rav1e", "vpx", "svt-av1", "x264", "x265"])]
  pub encoder: Encoder,

  /// Number of workers. 0 = automatic
  #[structopt(short, long, default_value = "0")]
  pub workers: usize,

  /// Do not check if the encoder arguments specified by `--video-params` are valid
  #[structopt(long)]
  pub force: bool,

  /// FFmpeg filter options
  #[structopt(short = "f", long = "ffmpeg")]
  pub ffmpeg_filter_args: Option<String>,

  /// Audio encoding parameters. If not specified, "-c:a copy" is used
  ///
  /// Example to encode the audio with libopus: -a="-c:a libopus -b:a 128k -ac 2"
  #[structopt(short, long)]
  pub audio_params: Option<String>,

  /// FFmpeg pixel format
  #[structopt(long, default_value = "yuv420p10le")]
  pub pix_format: Pixel,

  /// Calculate and plot the VMAF of the encode
  #[structopt(long)]
  pub vmaf: bool,

  /// Path to VMAF model
  #[structopt(long, parse(from_os_str))]
  pub vmaf_path: Option<PathBuf>,

  /// Resolution used for VMAF calculation
  #[structopt(long, default_value = "1920x1080")]
  pub vmaf_res: String,

  /// Number of threads to use for VMAF calculation
  #[structopt(long)]
  pub vmaf_threads: Option<usize>,

  /// VMAF score to target
  #[structopt(long)]
  pub target_quality: Option<f64>,

  /// Maximum number of probes allowed for target quality
  #[structopt(long, default_value = "4")]
  pub probes: u32,

  /// Framerate for probes, 1 - original
  #[structopt(long, default_value = "4")]
  pub probing_rate: u32,

  /// Use encoding settings for probes
  #[structopt(long)]
  pub probe_slow: bool,

  /// Min q for target quality
  #[structopt(long)]
  pub min_q: Option<u32>,

  /// Max q for target quality
  #[structopt(long)]
  pub max_q: Option<u32>,

  /// Filter applied to source at VMAF calcualation. This option should
  /// be specified if the source is cropped.
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

pub fn parse_cli() -> anyhow::Result<EncodeArgs> {
  let args = CliOpts::from_args();

  let temp = if let Some(path) = args.temp {
    path.to_str().unwrap().to_owned()
  } else {
    format!(".{}", hash_path(args.input.as_path()))
  };

  let input = Input::from(args.input.as_path());

  let mut encode_args = EncodeArgs {
    frames: input.frames(),
    logging: if let Some(log_file) = args.logging {
      Path::new(&format!("{}.log", log_file)).to_owned()
    } else {
      Path::new(&temp).join("log.log")
    },
    ffmpeg_filter_args: if let Some(args) = args.ffmpeg_filter_args {
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
    input,
    keep: args.keep,
    min_q: args.min_q,
    max_q: args.max_q,
    min_scene_len: args.min_scene_len,
    vmaf_threads: args.vmaf_threads,
    pix_format: PixelFormat {
      format: args.pix_format,
      bit_depth: args.encoder.get_format_bit_depth(args.pix_format)?,
    },
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

  encode_args.startup_check()?;

  if let Some(path) = args.output_file.as_deref() {
    if path.exists()
      && !confirm(&format!(
        "Output file {:?} exists. Do you want to overwrite it? [Y/n]: ",
        path
      ))?
    {
      println!("Not overwriting, aborting.");
      exit(0);
    }
  } else {
    let path: &Path = encode_args.output_file.as_ref();

    if path.exists()
      && !confirm(&format!(
        "Default output file {:?} exists. Do you want to overwrite it? [Y/n]: ",
        path
      ))?
    {
      println!("Not overwriting, aborting.");
      exit(0);
    }
  }

  Ok(encode_args)
}

pub fn run() -> anyhow::Result<()> {
  let mut args = parse_cli()?;

  ctrlc::set_handler(|| {
    println!("Stopped");
    exit(0);
  })?;

  args.initialize()?;
  args.encode_file()?;

  Ok(())
}
