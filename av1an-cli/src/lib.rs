use ansi_term::{Color, Style};
use anyhow::{anyhow, Context};
use anyhow::{bail, ensure};
use av1an_core::progress_bar::{get_first_multi_progress_bar, get_progress_bar};
use av1an_core::settings::{InputPixelFormat, PixelFormat};
use av1an_core::Input;
use av1an_core::ScenecutMethod;
use av1an_core::{ffmpeg, into_vec};
use ffmpeg_next::format::Pixel;
use flexi_logger::writers::LogWriter;
use flexi_logger::{FileSpec, Level, LevelFilter, LogSpecBuilder, Logger};
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

use once_cell::sync::OnceCell;

// needs to be static, runtime allocated string to avoid evil hacks to
// concatenate non-trivial strings at compile-time
fn version() -> &'static str {
  static INSTANCE: OnceCell<String> = OnceCell::new();
  INSTANCE.get_or_init(|| {
    match (
      option_env!("VERGEN_GIT_SHA_SHORT"),
      option_env!("VERGEN_CARGO_PROFILE"),
      option_env!("VERGEN_RUSTC_SEMVER"),
      option_env!("VERGEN_RUSTC_LLVM_VERSION"),
      option_env!("VERGEN_CARGO_TARGET_TRIPLE"),
      option_env!("VERGEN_BUILD_DATE"),
      option_env!("VERGEN_GIT_COMMIT_DATE"),
    ) {
      (
        Some(git_hash),
        Some(cargo_profile),
        Some(rustc_ver),
        Some(llvm_ver),
        Some(target_triple),
        Some(build_date),
        Some(commit_date),
      ) => {
        format!(
          "{} (rev {}) ({})

* Compiler
  rustc {} (LLVM {})

* Target Triple
  {}

* Date Info
   Build Date:  {}
  Commit Date:  {}",
          env!("CARGO_PKG_VERSION"),
          git_hash,
          cargo_profile,
          rustc_ver,
          llvm_ver,
          target_triple,
          build_date,
          commit_date
        )
      }
      // only include the semver on a release (when git information isn't available)
      _ => env!("CARGO_PKG_VERSION").into(),
    }
  })
}

fn max_tries_valid(tries: String) -> Result<(), String> {
  match tries.parse::<usize>() {
    Ok(tries) => {
      if tries == 0 {
        Err("max_tries must be greater than 0".into())
      } else {
        Ok(())
      }
    }
    Err(e) => Err(format!("{}", e)),
  }
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
  #[structopt(short, long, conflicts_with = "verbose")]
  pub quiet: bool,

  /// Print extra progress info and stats to terminal
  #[structopt(long)]
  pub verbose: bool,

  /// Specify this option to log to a non-default file
  #[structopt(short, long)]
  pub log_file: Option<String>,

  /// Set log level for log file (does not affect command-line log level)
  ///
  /// error: Designates very serious errors.
  ///
  /// warn: Designates hazardous situations.
  ///
  /// info: Designates useful information.
  ///
  /// debug: Designates lower priority information. Includes rav1e scenechange decision info.
  ///
  /// trace: Designates very low priority, often extremely verbose, information.
  #[structopt(long, default_value = "info", possible_values=&["error", "warn", "info", "debug", "trace"])]
  // "off" is also an allowed value for LevelFilter but we just disable the user from setting it
  pub log_level: LevelFilter,

  /// Resume previous session
  #[structopt(short, long)]
  pub resume: bool,

  /// Do not delete the temporary folder after encoding has finished
  #[structopt(short, long)]
  pub keep: bool,

  /// Overwrite output file without confirmation
  #[structopt(short = "y")]
  pub overwrite: bool,

  /// Maximum number of chunk restarts for an encode
  #[structopt(long, default_value = "3", validator = max_tries_valid)]
  pub max_tries: usize,

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
  #[structopt(long, default_value = "24")]
  pub min_scene_len: usize,

  /// Specify number encoding passes
  ///
  /// When using vpx or aom with RT, set this option to 1.
  #[structopt(short, long, possible_values=&["1", "2"])]
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

  /// Optionally pin each worker to this many threads
  ///
  /// This option does nothing on unsupported platforms (currently only
  /// supported on Linux and Windows)
  #[structopt(long)]
  pub set_thread_affinity: Option<usize>,

  /// Do not check if the encoder arguments specified by `--video-params` are valid
  #[structopt(long)]
  pub force: bool,

  /// FFmpeg filter options
  #[structopt(short = "f", long = "ffmpeg")]
  pub ffmpeg_filter_args: Option<String>,

  /// Audio encoding parameters. If not specified, "-c:a copy" is used.
  /// Do not use FFmpeg's `-map` syntax with this option. Instead, use the
  /// colon syntax with each parameter you specify.
  ///
  /// Subtitles are always copied by default.
  ///
  /// Example to encode all audio tracks with libopus at 128k:
  ///
  /// -a="-c:a libopus -b:a 128k"
  ///
  /// Example to encode the first audio track with libopus at 128k, and the
  /// second audio track with aac at 24k, where only the second track is
  /// downmixed to a single channel:
  ///
  /// -a="-c:a:0 libopus -b:a:0 128k -c:a:1 aac -ac:a:1 1 -b:a:1 24k"
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

  /// Min Q for target quality
  #[structopt(long)]
  pub min_q: Option<u32>,

  /// Max Q for target quality
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

pub fn parse_cli(args: CliOpts) -> anyhow::Result<EncodeArgs> {
  let temp = if let Some(path) = args.temp.as_ref() {
    path.to_str().unwrap().to_owned()
  } else {
    format!(".{}", hash_path(args.input.as_path()))
  };

  let input = Input::from(args.input.as_path());

  // TODO make an actual constructor for this
  let mut encode_args = EncodeArgs {
    frames: 0,
    log_file: if let Some(log_file) = args.log_file.as_ref() {
      Path::new(&format!("{}.log", log_file)).to_owned()
    } else {
      Path::new(&temp).join("log.log")
    },
    ffmpeg_filter_args: if let Some(args) = args.ffmpeg_filter_args.as_ref() {
      shlex::split(args).ok_or_else(|| anyhow!("Failed to split ffmpeg filter arguments"))?
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
    video_params: if let Some(args) = args.video_params.as_ref() {
      shlex::split(args).ok_or_else(|| anyhow!("Failed to split video encoder arguments"))?
    } else {
      Vec::new()
    },
    output_file: if let Some(path) = args.output_file.as_ref() {
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
          .unwrap_or_else(|| args.input.as_ref())
          .to_string_lossy(),
        args.encoder
      )
    },
    audio_params: if let Some(args) = args.audio_params.as_ref() {
      shlex::split(args).ok_or_else(|| anyhow!("Failed to split ffmpeg audio encoder arguments"))?
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
    keep: args.keep,
    max_tries: args.max_tries,
    min_q: args.min_q,
    max_q: args.max_q,
    min_scene_len: args.min_scene_len,
    vmaf_threads: args.vmaf_threads,
    input_pix_format: {
      match &input {
        Input::Video(path) => InputPixelFormat::FFmpeg {
          format: ffmpeg::get_pixel_format(path.as_ref()).with_context(|| {
            format!(
              "FFmpeg failed to get pixel format for input video {:?}",
              path
            )
          })?,
        },
        Input::VapourSynth(path) => InputPixelFormat::VapourSynth {
          bit_depth: crate::vapoursynth::bit_depth(path.as_ref()).with_context(|| {
            format!(
              "VapourSynth failed to get bit depth for input video {:?}",
              path
            )
          })?,
        },
      }
    },
    input,
    output_pix_format: PixelFormat {
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
    set_thread_affinity: args.set_thread_affinity,
  };

  encode_args.startup_check()?;

  if !args.overwrite {
    if let Some(path) = args.output_file.as_ref() {
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
  }

  Ok(encode_args)
}

pub struct StderrLogger {
  level: Level,
}

impl LogWriter for StderrLogger {
  fn write(
    &self,
    _now: &mut flexi_logger::DeferredNow,
    record: &flexi_logger::Record,
  ) -> std::io::Result<()> {
    if record.level() > self.level {
      return Ok(());
    }

    let style = match record.level() {
      Level::Error => Style::default().fg(Color::Fixed(196)).bold(),
      Level::Warn => Style::default().fg(Color::Fixed(208)).bold(),
      Level::Info => Style::default().dimmed(),
      _ => Style::default(),
    };

    let msg = style.paint(format!("{}", record.args()));

    macro_rules! create_format_args {
      () => {
        format_args!(
          "{} [{}] {}",
          style.paint(format!("{}", record.level())),
          record.module_path().unwrap_or("<unnamed>"),
          msg
        )
      };
    }

    if let Some(pbar) = get_first_multi_progress_bar() {
      pbar.println(std::fmt::format(create_format_args!()));
    } else if let Some(pbar) = get_progress_bar() {
      pbar.println(std::fmt::format(create_format_args!()));
    } else {
      eprintln!("{}", create_format_args!());
    }

    Ok(())
  }

  fn flush(&self) -> std::io::Result<()> {
    Ok(())
  }
}

pub fn run() -> anyhow::Result<()> {
  let cli_args = CliOpts::from_args();
  let log_level = cli_args.log_level;
  let mut args = parse_cli(cli_args)?;

  ctrlc::set_handler(|| {
    println!("Stopped");
    exit(0);
  })?;

  let log = LogSpecBuilder::new().default(log_level).build();

  Logger::with(log)
    .log_to_file_and_writer(
      FileSpec::try_from(PathAbs::new(&args.log_file)?)?,
      Box::new(StderrLogger {
        level: match args.verbosity {
          Verbosity::Normal | Verbosity::Quiet => Level::Warn,
          Verbosity::Verbose => Level::Info,
        },
      }),
    )
    .start()?;

  args.initialize()?;

  args.encode_file()?;

  Ok(())
}
