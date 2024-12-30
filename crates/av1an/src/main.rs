use std::{
    io::{self, Write},
    panic,
    path::{Path, PathBuf},
    process,
    process::exit,
};

use anyhow::{anyhow, bail, ensure, Context};
use av1an_core::{
    context::Av1anContext,
    encoder::Encoder,
    hash_path,
    into_vec,
    settings::{EncodeArgs, InputPixelFormat, PixelFormat},
    util::read_in_dir,
    vapoursynth,
    Input,
    ScenecutMethod,
    SplitMethod,
    TaskMethod,
    TaskOrdering,
    Verbosity,
};
use av1an_logging::init_logging;
use av1an_output::ConcatMethod;
use clap::{value_parser, Parser};
use ffmpeg::format::Pixel;
use path_abs::{PathAbs, PathInfo};
use tracing::{instrument, warn};
fn main() -> anyhow::Result<()> {
    let orig_hook = panic::take_hook();
    // Catch panics in child threads
    panic::set_hook(Box::new(move |panic_info| {
        orig_hook(panic_info);
        process::exit(1);
    }));
    run()
}

/// Cross-platform command-line AV1 / VP9 / HEVC / H264 encoding framework with
/// per-scene quality encoding
#[derive(Parser, Debug)]
#[clap(name = "av1an")]
pub struct CliOpts {
    /// Input file to encode
    ///
    /// Can be a video or vapoursynth (.py, .vpy) script.
    #[clap(short, required = true)]
    pub input: Vec<PathBuf>,

    /// Video output file
    #[clap(short)]
    pub output_file: Option<PathBuf>,

    /// Temporary directory to use
    ///
    /// If not specified, the temporary directory name is a hash of the input
    /// file name.
    #[clap(long)]
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

    /// Resume previous session from temporary directory
    #[clap(short, long)]
    pub resume: bool,

    /// Do not delete the temporary folder after encoding has finished
    #[clap(short, long)]
    pub keep: bool,

    /// Do not check if the encoder arguments specified by -v/--video-params
    /// are valid
    #[clap(long)]
    pub force: bool,

    /// Overwrite output file, without confirmation
    #[clap(short = 'y')]
    pub overwrite: bool,

    /// Never overwrite output file, without confirmation
    #[clap(short = 'n', conflicts_with = "overwrite")]
    pub never_overwrite: bool,

    /// Maximum number of task restarts for an encode
    #[clap(long, default_value_t = 3, value_parser = value_parser!(u32).range(1..))]
    pub max_tries: u32,

    /// Number of workers to spawn [0 = automatic]
    #[clap(short, long, default_value_t = 0)]
    pub workers: usize,

    /// Pin each worker to a specific set of threads of this size (disabled by
    /// default)
    ///
    /// This is currently only supported on Linux and Windows, and does nothing
    /// on unsupported platforms. Leaving this option unspecified allows
    /// the OS to schedule all processes spawned.
    #[clap(long)]
    pub set_thread_affinity: Option<usize>,

    /// Pass python argument(s) to the script environment
    /// --vspipe-args "message=fluffy kittens" "head=empty"
    #[clap(long, num_args(0..))]
    pub vspipe_args: Vec<String>,

    /// File location for scenes
    #[clap(short, long, help_heading = "Scene Detection")]
    pub scenes: Option<PathBuf>,

    /// Maximum scene length, in seconds
    ///
    /// If both frames and seconds are specified, then the number of frames
    /// will take priority.
    #[clap(long, default_value_t = 10.0, help_heading = "Scene Detection")]
    pub extra_split_sec: f64,

    /// Method used to determine task boundaries
    ///
    /// "av-scenechange" uses an algorithm to analyze which frames of the video
    /// are the start of new scenes, while "none" disables scene detection
    /// entirely (and only relies on -x/--extra-split to
    /// add extra scenecuts).
    #[clap(long, default_value_t = SplitMethod::AvScenechange, help_heading = "Scene Detection")]
    pub split_method: SplitMethod,

    /// Scene detection algorithm to use for av-scenechange
    ///
    /// Standard: Most accurate, still reasonably fast. Uses a cost-based
    /// algorithm to determine keyframes.
    ///
    /// Fast: Very fast, but less accurate. Determines keyframes based on the
    /// raw difference between pixels.
    #[clap(long, default_value_t = ScenecutMethod::Standard, help_heading = "Scene Detection")]
    pub sc_method: ScenecutMethod,

    /// Run the scene detection only before exiting
    ///
    /// Requires a scene file with --scenes.
    #[clap(long, requires("scenes"), help_heading = "Scene Detection")]
    pub sc_only: bool,

    /// Perform scene detection with this pixel format
    #[clap(long, help_heading = "Scene Detection")]
    pub sc_pix_format: Option<Pixel>,

    /// Optional downscaling for scene detection
    ///
    /// Specify as the desired maximum height to scale to (e.g. "720" to
    /// downscale to 720p — this will leave lower resolution content
    /// untouched). Downscaling improves scene detection speed but lowers
    /// accuracy, especially when scaling to very low resolutions.
    ///
    /// By default, no downscaling is performed.
    #[clap(long, help_heading = "Scene Detection")]
    pub sc_downscale_height: Option<usize>,

    /// Maximum scene length
    ///
    /// When a scenecut is found whose distance to the previous scenecut is
    /// greater than the value specified by this option, one or more extra
    /// splits (scenecuts) are added. Set this option to 0 to disable
    /// adding extra splits.
    #[clap(short = 'x', long, help_heading = "Scene Detection")]
    pub extra_split: Option<usize>,

    /// Minimum number of frames for a scenecut
    #[clap(long, default_value_t = 24, help_heading = "Scene Detection")]
    pub min_scene_len: usize,

    /// Comma-separated list of frames to force as keyframes
    ///
    /// Can be useful for improving seeking with chapters, etc.
    /// Frame 0 will always be a keyframe and does not need to be specified
    /// here.
    #[clap(long, help_heading = "Scene Detection")]
    pub force_keyframes: Option<String>,

    /// Ignore any detected mismatch between scene frame count and encoder
    /// frame count
    #[clap(long, help_heading = "Encoding")]
    pub ignore_frame_mismatch: bool,

    /// Video encoder to use
    #[clap(short, long, default_value_t = Encoder::aom, help_heading = "Encoding")]
    pub encoder: Encoder,

    /// Parameters for video encoder
    ///
    /// These parameters are for the encoder binary directly, so the ffmpeg
    /// syntax cannot be used. For example, CRF is specified in ffmpeg via
    /// "-crf <crf>", but the x264 binary takes this value with double
    /// dashes, as in "--crf <crf>". See the --help output of each encoder for
    /// a list of valid options.
    #[clap(short, long, allow_hyphen_values = true, help_heading = "Encoding")]
    pub video_params: Option<String>,

    /// Number of encoder passes
    ///
    /// Since aom benefit from two-pass mode even with constant quality
    /// mode (unlike other encoders in which two-pass mode is used for more
    /// accurate VBR rate control), two-pass mode is used by default for
    /// these encoders.
    ///
    /// When using aom with RT mode (--rt), one-pass mode is always used
    /// regardless of the value specified by this flag (as RT mode in aom
    /// supports one-pass encoding).
    #[clap(short, long, value_parser = value_parser!(u8).range(1..=2), help_heading = "Encoding")]
    pub passes: Option<u8>,

    /// Audio encoding parameters (ffmpeg syntax)
    ///
    /// If not specified, "-c:a copy" is used.
    ///
    /// Do not use ffmpeg's -map syntax with this option. Instead, use the
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
    #[clap(short, long, allow_hyphen_values = true, help_heading = "Encoding")]
    pub audio_params: Option<String>,

    /// FFmpeg filter options
    #[clap(
        short = 'f',
        long = "ffmpeg",
        allow_hyphen_values = true,
        help_heading = "Encoding"
    )]
    pub ffmpeg_filter_args: Option<String>,

    /// Method used for piping exact ranges of frames to the encoder
    ///
    /// Methods that require an external vapoursynth plugin:
    ///
    /// lsmash - Generally the best and most accurate method. Does not require
    /// intermediate files. Errors generally only occur if the input file
    /// itself is broken (for example, if the video bitstream is invalid in
    /// some way, video players usually try to recover from the errors as
    /// much as possible even if it results in visible artifacts, while lsmash
    /// will instead throw an error). Requires the lsmashsource vapoursynth
    /// plugin to be installed.
    ///
    /// ffms2 - Accurate and does not require intermediate files. Can sometimes
    /// have bizarre bugs that are not present in lsmash (that can
    /// cause artifacts in the piped output). Slightly faster than lsmash for
    /// y4m input. Requires the ffms2 vapoursynth plugin to be installed.
    ///
    /// dgdecnv - Very fast, but only decodes AVC, HEVC, MPEG-2, and VC1. Does
    /// not require intermediate files. Requires dgindexnv to be present in
    /// system path, NVIDIA GPU that support CUDA video decoding, and dgdecnv
    /// vapoursynth plugin to be installed.
    ///
    /// bestsource - Very slow but accurate. Linearly decodes input files, very
    /// slow. Does not require intermediate files, requires the BestSource
    /// vapoursynth plugin to be installed.
    ///
    /// Methods that only require ffmpeg:
    ///
    /// hybrid - Uses a combination of segment and select. Usually accurate but
    /// requires intermediate files (which can be large). Avoids
    /// decoding irrelevant frames by seeking to the first keyframe before the
    /// requested frame and decoding only a (usually very small)
    /// number of irrelevant frames until relevant frames are decoded and piped
    /// to the encoder.
    ///
    /// select - Extremely slow, but accurate. Does not require intermediate
    /// files. Decodes from the first frame to the requested frame, without
    /// skipping irrelevant frames (causing quadratic decoding complexity).
    ///
    /// segment - Create tasks based on keyframes in the source. Not frame
    /// exact, as it can only split on keyframes in the source.
    /// Requires intermediate files (which can be large).
    ///
    /// Default: lsmash (if available), otherwise ffms2 (if available),
    /// otherwise DGDecNV (if available), otherwise bestsource (if available),
    /// otherwise hybrid.
    #[clap(short = 'm', long, help_heading = "Encoding")]
    pub task_method: Option<TaskMethod>,

    /// The order in which av1an will encode tasks
    ///
    /// Available methods:
    ///
    /// long-to-short - The longest tasks will be encoded first. This method
    /// results in the smallest amount of time with idle cores,
    /// as the encode will not be waiting on a very long task to finish at the
    /// end of the encode after all other tasks have finished.
    ///
    /// short-to-long - The shortest tasks will be encoded first.
    ///
    /// sequential - The tasks will be encoded in the order they appear in the
    /// video.
    ///
    /// random - The tasks will be encoded in a random order. This will
    /// provide a more accurate estimated filesize sooner in the encode.
    #[clap(long, default_value_t = TaskOrdering::LongestFirst, help_heading = "Encoding")]
    pub task_order: TaskOrdering,

    /// Determines method used for concatenating encoded tasks and audio into
    /// output file
    ///
    /// ffmpeg - Uses ffmpeg for concatenation. Unfortunately, ffmpeg sometimes
    /// produces files with partially broken audio seeking, so mkvmerge
    /// should generally be preferred if available. ffmpeg concatenation
    /// also produces broken files with the --enable-keyframe-filtering=2
    /// option in aomenc, so it is disabled if that option is used.
    /// However, ffmpeg can mux into formats other than matroska (.mkv),
    /// such as WebM. To output WebM, use a .webm extension in the output file.
    ///
    /// mkvmerge - Generally the best concatenation method (as it does not have
    /// either of the aforementioned issues that ffmpeg has), but can only
    /// produce matroska (.mkv) files. Requires mkvmerge to be installed.
    ///
    /// ivf - Experimental concatenation method implemented in av1an itself to
    /// concatenate to an ivf file (which only supports VP8, VP9, and AV1,
    /// and does not support audio).
    #[clap(short, long, default_value_t = ConcatMethod::FFmpeg, help_heading = "Encoding")]
    pub concat: ConcatMethod,

    /// FFmpeg pixel format
    #[clap(long, default_value = "yuv420p10le", help_heading = "Encoding")]
    pub pix_format: Pixel,
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
                println!("Sorry, response {other:?} is not understood.");
                buf.clear();
                continue;
            },
        }
    }
}

/// Given Folder and File path as inputs
/// Converts them all to file paths
/// Converting only depth 1 of Folder paths
pub(crate) fn resolve_file_paths(
    path: &Path,
) -> anyhow::Result<Box<dyn Iterator<Item = PathBuf>>> {
    // TODO: to validate file extensions
    // let valid_media_extensions = ["mkv", "mov", "mp4", "webm", "avi", "qt",
    // "ts", "m2t", "py", "vpy"];

    ensure!(
        path.exists(),
        "Input path {:?} does not exist. Please ensure you typed it properly \
         and it has not been moved.",
        path
    );

    if path.is_dir() {
        Ok(Box::new(read_in_dir(path)?))
    } else {
        Ok(Box::new(std::iter::once(path.to_path_buf())))
    }
}

#[tracing::instrument]
pub fn parse_cli(args: CliOpts) -> anyhow::Result<Vec<EncodeArgs>> {
    let input_paths = &*args.input;

    let mut inputs = Vec::new();
    for path in input_paths {
        inputs.extend(resolve_file_paths(path)?);
    }

    let mut valid_args: Vec<EncodeArgs> = Vec::with_capacity(inputs.len());

    for input in inputs {
        let temp = if let Some(path) = args.temp.as_ref() {
            path.to_str().unwrap().to_owned()
        } else {
            format!(".{}", hash_path(input.as_path()))
        };

        let input = Input::from((input, args.vspipe_args.clone()));

        let video_params = if let Some(args) = args.video_params.as_ref() {
            shlex::split(args).ok_or_else(|| {
                anyhow!("Failed to split video encoder arguments")
            })?
        } else {
            Vec::new()
        };
        let output_pix_format = PixelFormat {
            format:    args.pix_format,
            bit_depth: args
                .encoder
                .get_format_bit_depth(args.pix_format)?,
        };

        // TODO make an actual constructor for this
        let arg = EncodeArgs {
            log_file: if let Some(log_file) = args.log_file.as_ref() {
                Path::new(&format!("{log_file}.log")).to_owned()
            } else {
                Path::new(&temp).join("log.log")
            },
            ffmpeg_filter_args: if let Some(args) =
                args.ffmpeg_filter_args.as_ref()
            {
                shlex::split(args).ok_or_else(|| {
                    anyhow!("Failed to split ffmpeg filter arguments")
                })?
            } else {
                Vec::new()
            },
            temp: temp.clone(),
            force: args.force,
            passes: if let Some(passes) = args.passes {
                passes
            } else {
                args.encoder.get_default_pass()
            },
            video_params: video_params.clone(),
            output_file: if let Some(path) = args.output_file.as_ref() {
                let path = PathAbs::new(path)?;

                if let Ok(parent) = path.parent() {
                    ensure!(
                        parent.exists(),
                        "Path to file {:?} is invalid",
                        path
                    );
                } else {
                    bail!("Failed to get parent directory of path: {:?}", path);
                }

                path.to_string_lossy().to_string()
            } else {
                format!(
                    "{}_{}.mkv",
                    input
                        .as_path()
                        .file_stem()
                        .unwrap_or_else(|| input.as_path().as_ref())
                        .to_string_lossy(),
                    args.encoder
                )
            },
            audio_params: if let Some(args) = args.audio_params.as_ref() {
                shlex::split(args).ok_or_else(|| {
                    anyhow!("Failed to split ffmpeg audio encoder arguments")
                })?
            } else {
                into_vec!["-c:a", "copy"]
            },
            task_method: args
                .task_method
                .unwrap_or_else(vapoursynth::best_available_task_method),
            task_order: args.task_order,
            concat: args.concat,
            encoder: args.encoder,
            extra_splits_len: match args.extra_split {
                Some(0) => None,
                Some(x) => Some(x),
                // Make sure it's at least 10 seconds, unless specified by user
                None => match input.frame_rate() {
                    Ok(fps) => Some((fps * args.extra_split_sec) as usize),
                    Err(_) => Some(240_usize),
                },
            },

            sc_pix_format: args.sc_pix_format,
            keep: args.keep,
            max_tries: args.max_tries as usize,
            min_scene_len: args.min_scene_len,
            input_pix_format: {
                match &input {
                    Input::Video {
                        path,
                    } => InputPixelFormat::FFmpeg {
                        format: av1an_ffmpeg::get_pixel_format(path.as_ref())
                            .with_context(|| {
                            format!(
                                "FFmpeg failed to get pixel format for input \
                                 video {path:?}"
                            )
                        })?,
                    },
                    Input::VapourSynth {
                        path, ..
                    } => InputPixelFormat::VapourSynth {
                        bit_depth: crate::vapoursynth::bit_depth(
                            path.as_ref(),
                            input.as_vspipe_args_map()?,
                        )
                        .with_context(|| {
                            format!(
                                "VapourSynth failed to get bit depth for \
                                 input video {path:?}"
                            )
                        })?,
                    },
                }
            },
            input,
            output_pix_format,
            resume: args.resume,
            scenes: args.scenes.clone(),
            split_method: args.split_method.clone(),
            sc_method: args.sc_method,
            sc_only: args.sc_only,
            sc_downscale_height: args.sc_downscale_height,
            force_keyframes: parse_comma_separated_numbers(
                args.force_keyframes.as_deref().unwrap_or(""),
            )?,

            verbosity: if args.quiet {
                Verbosity::Quiet
            } else if args.verbose {
                Verbosity::Verbose
            } else {
                Verbosity::Normal
            },
            workers: args.workers,
            set_thread_affinity: args.set_thread_affinity,
            ignore_frame_mismatch: args.ignore_frame_mismatch,
        };

        if !args.overwrite {
            // UGLY: taking first file for output file
            if let Some(path) = args.output_file.as_ref() {
                if path.exists()
                    && (args.never_overwrite
                        || !confirm(&format!(
                            "Output file {path:?} exists. Do you want to \
                             overwrite it? [Y/n]: "
                        ))?)
                {
                    println!("Not overwriting, aborting.");
                    exit(0);
                }
            } else {
                let path: &Path = arg.output_file.as_ref();

                if path.exists()
                    && (args.never_overwrite
                        || !confirm(&format!(
                            "Default output file {path:?} exists. Do you want \
                             to overwrite it? [Y/n]: "
                        ))?)
                {
                    println!("Not overwriting, aborting.");
                    exit(0);
                }
            }
        }

        valid_args.push(arg)
    }

    Ok(valid_args)
}

#[instrument]
pub fn run() -> anyhow::Result<()> {
    init_logging();

    let cli_args = CliOpts::parse();

    let args = parse_cli(cli_args)?;

    for arg in args {
        Av1anContext::new(arg)?.encode_file()?;
    }

    Ok(())
}

fn parse_comma_separated_numbers(string: &str) -> anyhow::Result<Vec<usize>> {
    let mut result = Vec::new();

    let string = string.trim();
    if string.is_empty() {
        return Ok(result);
    }

    for val in string.split(',') {
        result.push(val.trim().parse()?);
    }
    Ok(result)
}
