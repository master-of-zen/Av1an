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

use clap::{AppSettings, Parser};

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
          "{}-unstable (rev {}) ({})

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

fn max_tries_valid(tries: &str) -> Result<(), String> {
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
#[derive(Parser, Debug)]
#[clap(name = "av1an", version = version(), setting = AppSettings::DeriveDisplayOrder)]
pub struct CliOpts {
  /// Input file to encode
  ///
  /// Can be a video or vapoursynth (.py, .vpy) script.
  #[clap(short, parse(from_os_str))]
  pub input: PathBuf,

  /// Video output file
  #[clap(short, parse(from_os_str))]
  pub output_file: Option<PathBuf>,

  /// Temporary directory to use
  ///
  /// If not specified, the temporary directory name is a hash of the input file name.
  #[clap(long, parse(from_os_str))]
  pub temp: Option<PathBuf>,

  /// Disable printing progress to the terminal
  #[clap(short, long, conflicts_with = "verbose")]
  pub quiet: bool,

  /// Print extra progress info and stats to terminal
  #[clap(long)]
  pub verbose: bool,

  /// Log file location [default: <temp dir>/log.log]
  #[clap(short, long)]
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
  #[clap(long, default_value_t = LevelFilter::Info, ignore_case = true, possible_values = &["error", "warn", "info", "debug", "trace"])]
  // "off" is also an allowed value for LevelFilter but we just disable the user from setting it
  pub log_level: LevelFilter,

  /// Resume previous session from temporary directory
  #[clap(short, long)]
  pub resume: bool,

  /// Do not delete the temporary folder after encoding has finished
  #[clap(short, long)]
  pub keep: bool,

  /// Do not check if the encoder arguments specified by -v/--video-params are valid
  #[clap(long)]
  pub force: bool,

  /// Overwrite output file without confirmation
  #[clap(short = 'y')]
  pub overwrite: bool,

  /// Maximum number of chunk restarts for an encode
  #[clap(long, default_value_t = 3, validator = max_tries_valid)]
  pub max_tries: usize,

  /// Number of workers to spawn [0 = automatic]
  #[clap(short, long, default_value_t = 0)]
  pub workers: usize,

  /// Pin each worker to a specific set of threads of this size (disabled by default)
  ///
  /// This is currently only supported on Linux and Windows, and does nothing on unsupported platforms.
  /// Leaving this option unspecified allows the OS to schedule all processes spawned.
  #[clap(long)]
  pub set_thread_affinity: Option<usize>,

  /// File location for scenes
  #[clap(short, long, parse(from_os_str), help_heading = "SCENE DETECTION")]
  pub scenes: Option<PathBuf>,

  /// Method used to determine chunk boundaries
  ///
  /// "av-scenechange" uses an algorithm to analyze which frames of the video are the start of new
  /// scenes, while "none" disables scene detection entirely (and only relies on -x/--extra-split to
  /// add extra scenecuts).
  #[clap(long, possible_values = &["av-scenechange", "none"], default_value_t = SplitMethod::AvScenechange, help_heading = "SCENE DETECTION")]
  pub split_method: SplitMethod,

  /// Scene detection algorithm to use for av-scenechange
  ///
  /// Standard: Most accurate, still reasonably fast. Uses a cost-based algorithm to determine keyframes.
  ///
  /// Fast: Very fast, but less accurate. Determines keyframes based on the raw difference between pixels.
  #[clap(long, possible_values = &["standard", "fast"], default_value_t = ScenecutMethod::Standard, help_heading = "SCENE DETECTION")]
  pub sc_method: ScenecutMethod,

  /// Perform scene detection with this pixel format
  #[clap(long, help_heading = "SCENE DETECTION")]
  pub sc_pix_format: Option<Pixel>,

  /// Optional downscaling for scene detection
  ///
  /// Specify as the desired maximum height to scale to (e.g. "720" to downscale to
  /// 720p — this will leave lower resolution content untouched). Downscaling improves
  /// scene detection speed but lowers accuracy, especially when scaling to very low resolutions.
  ///
  /// By default, no downscaling is performed.
  #[clap(long, help_heading = "SCENE DETECTION")]
  pub sc_downscale_height: Option<usize>,

  /// Maximum scene length
  ///
  /// When a scenecut is found whose distance to the previous scenecut is greater than the value
  /// specified by this option, one or more extra splits (scenecuts) are added. Set this option
  /// to 0 to disable adding extra splits.
  #[clap(
    short = 'x',
    long,
    default_value_t = 240,
    help_heading = "SCENE DETECTION"
  )]
  pub extra_split: usize,

  /// Minimum number of frames for a scenecut
  #[clap(long, default_value_t = 24, help_heading = "SCENE DETECTION")]
  pub min_scene_len: usize,

  /// Video encoder to use
  #[clap(short, long, default_value_t = Encoder::aom, possible_values = &["aom", "rav1e", "vpx", "svt-av1", "x264", "x265"], help_heading = "ENCODING")]
  pub encoder: Encoder,

  /// Parameters for video encoder
  ///
  /// These parameters are for the encoder binary directly, so the ffmpeg syntax cannot be used.
  /// For example, CRF is specified in ffmpeg via "-crf <crf>", but the x264 binary takes this
  /// value with double dashes, as in "--crf <crf>". See the --help output of each encoder for
  /// a list of valid options.
  #[clap(short, long, allow_hyphen_values = true, help_heading = "ENCODING")]
  pub video_params: Option<String>,

  /// Number of encoder passes
  ///
  /// Since aom and vpx benefit from two-pass mode even with constant quality mode (unlike other
  /// encoders in which two-pass mode is used for more accurate VBR rate control), two-pass mode is
  /// used by default for these encoders.
  ///
  /// When using aom or vpx with RT mode (--rt), one-pass mode is always used regardless of the
  /// value specified by this flag (as RT mode in aom and vpx only supports one-pass encoding).
  #[clap(short, long, possible_values = &["1", "2"], help_heading = "ENCODING")]
  pub passes: Option<u8>,

  /// Audio encoding parameters (ffmpeg syntax)
  ///
  /// If not specified, "-c:a copy" is used.
  ///
  /// Do not use ffmpeg's -map syntax with this option. Instead, use the colon
  /// syntax with each parameter you specify.
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
  #[clap(short, long, allow_hyphen_values = true, help_heading = "ENCODING")]
  pub audio_params: Option<String>,

  /// FFmpeg filter options
  #[clap(
    short = 'f',
    long = "ffmpeg",
    allow_hyphen_values = true,
    help_heading = "ENCODING"
  )]
  pub ffmpeg_filter_args: Option<String>,

  /// Method used for piping exact ranges of frames to the encoder
  ///
  /// Methods that require an external vapoursynth plugin:
  ///
  /// lsmash - Generally the best and most accurate method. Does not require intermediate files. Errors generally only
  /// occur if the input file itself is broken (for example, if the video bitstream is invalid in some way, video players usually try
  /// to recover from the errors as much as possible even if it results in visible artifacts, while lsmash will instead throw an error).
  /// Requires the lsmashsource vapoursynth plugin to be installed.
  ///
  /// ffms2 - Accurate and does not require intermediate files. Can sometimes have bizarre bugs that are not present in lsmash (that can
  /// cause artifacts in the piped output). Slightly faster than lsmash for y4m input. Requires the ffms2 vapoursynth plugin to be
  /// installed.
  ///
  /// Methods that only require ffmpeg:
  ///
  /// hybrid - Uses a combination of segment and select. Usually accurate but requires intermediate files (which can be large). Avoids
  /// decoding irrelevant frames by seeking to the first keyframe before the requested frame and decoding only a (usually very small)
  /// number of irrelevant frames until relevant frames are decoded and piped to the encoder.
  ///
  /// select - Extremely slow, but accurate. Does not require intermediate files. Decodes from the first frame to the requested frame,
  /// without skipping irrelevant frames (causing quadratic decoding complexity).
  ///
  /// segment - Create chunks based on keyframes in the source. Not frame exact, as it can only split on keyframes in the source.
  /// Requires intermediate files (which can be large).
  ///
  /// Default: lsmash (if available), otherwise ffms2 (if available), otherwise hybrid.
  #[clap(short = 'm', long, possible_values = &["segment", "select", "ffms2", "lsmash", "hybrid"], help_heading = "ENCODING")]
  pub chunk_method: Option<ChunkMethod>,

  /// Generates a photon noise table and applies it using grain synthesis [strength: 0-64] (disabled by default)
  ///
  /// Photon noise tables are more visually pleasing than the film grain generated by aomenc,
  /// and provide a consistent level of grain regardless of the level of grain in the source.
  /// Strength values correlate to ISO values, e.g. 1 = ISO 100, and 64 = ISO 6400. This
  /// option currently only supports aomenc.
  ///
  /// An encoder's grain synthesis will still work without using this option, by specifying the
  /// correct parameter to the encoder. However, the two should not be used together,
  /// and specifying this option will disable aomenc's internal grain synthesis.
  #[clap(long, help_heading = "ENCODING")]
  pub photon_noise: Option<u8>,

  /// ivf - Experimental concatenation method implemented in av1an itself to concatenate to an ivf
  /// file (which only supports VP8, VP9, and AV1, and does not support audio).
  #[clap(short, long, possible_values = &["ffmpeg", "mkvmerge", "ivf"], default_value_t = ConcatMethod::FFmpeg, help_heading = "ENCODING")]
  pub concat: ConcatMethod,

  /// FFmpeg pixel format
  #[clap(long, default_value = "yuv420p10le", help_heading = "ENCODING")]
  pub pix_format: Pixel,

  /// Plot an SVG of the VMAF for the encode
  ///
  /// This option is independent of --target-quality, i.e. it can be used with or without it.
  /// The SVG plot is created in the same directory as the output file.
  #[clap(long, help_heading = "VMAF")]
  pub vmaf: bool,

  /// Path to VMAF model (used by --vmaf and --target-quality)
  ///
  /// If not specified, ffmpeg's default is used.
  #[clap(long, parse(from_os_str), help_heading = "VMAF")]
  pub vmaf_path: Option<PathBuf>,

  /// Resolution used for VMAF calculation
  #[clap(long, default_value = "1920x1080", help_heading = "VMAF")]
  pub vmaf_res: String,

  /// Number of threads to use for VMAF calculation
  #[clap(long, help_heading = "VMAF")]
  pub vmaf_threads: Option<usize>,

  /// Filter applied to source at VMAF calcualation
  ///
  /// This option should be specified if the source is cropped, for example.
  #[clap(long, help_heading = "VMAF")]
  pub vmaf_filter: Option<String>,

  /// Target a VMAF score for encoding (disabled by default)
  ///
  /// For each chunk, target quality uses an algorithm to find the quantizer/crf needed to achieve a certain VMAF score.
  /// Target quality mode is much slower than normal encoding, but can improve the consistency of quality in some cases.
  ///
  /// The VMAF score range is 0-100 (where 0 is the worst quality, and 100 is the best). Floating-point values are allowed.
  #[clap(long, help_heading = "TARGET QUALITY")]
  pub target_quality: Option<f64>,

  /// Maximum number of probes allowed for target quality
  #[clap(long, default_value_t = 4, help_heading = "TARGET QUALITY")]
  pub probes: u32,

  /// Framerate for probes, 1 - original
  #[clap(long, default_value_t = 4, help_heading = "TARGET QUALITY")]
  pub probing_rate: u32,

  /// Use encoding settings for probes specified by --video-params rather than faster, less accurate settings
  ///
  /// Note that this always performs encoding in one-pass mode, regardless of --passes.
  #[clap(long, help_heading = "TARGET QUALITY")]
  pub probe_slow: bool,

  /// Lower bound for target quality Q-search early exit
  ///
  /// If min_q is tested and the probe's VMAF score is lower than target_quality, the Q-search early exits and
  /// min_q is used for the chunk.
  ///
  /// If not specified, the default value is used (chosen per encoder).
  #[clap(long, help_heading = "TARGET QUALITY")]
  pub min_q: Option<u32>,

  /// Upper bound for target quality Q-search early exit
  ///
  /// If max_q is tested and the probe's VMAF score is higher than target_quality, the Q-search early exits and
  /// max_q is used for the chunk.
  ///
  /// If not specified, the default value is used (chosen per encoder).
  #[clap(long, help_heading = "TARGET QUALITY")]
  pub max_q: Option<u32>,
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
    photon_noise: args.photon_noise,
    sc_pix_format: args.sc_pix_format,
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
    vs_script: None,
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
  let cli_args = CliOpts::parse();
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
