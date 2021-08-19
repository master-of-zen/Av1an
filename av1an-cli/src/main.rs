use std::path::Path;

use av1an_cli::Args;
use av1an_core::vapoursynth;
use av1an_core::{hash_path, is_vapoursynth, Project, Verbosity};
use clap::Clap;
use path_abs::{PathAbs, PathInfo};

pub fn main() {
  let args = Args::parse();

  let temp = if let Some(path) = args.temp {
    path.to_str().unwrap().to_owned()
  } else {
    format!(".{}", hash_path(&args.input.to_str().unwrap().to_owned()))
  };

  // TODO parse with normal (non proc-macro) clap methods to simplify this
  // Unify Project/Args
  let mut project = Project {
    frames: 0,
    is_vs: is_vapoursynth(args.input.to_str().unwrap()),
    logging: if let Some(log_file) = args.logging {
      Path::new(&format!("{}.log", log_file)).to_owned()
    } else {
      Path::new(&temp).join("log.log")
    },
    ffmpeg: if let Some(s) = args.ffmpeg {
      shlex::split(&s).unwrap_or_else(Vec::new)
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
    video_params: if let Some(params) = args.video_params {
      shlex::split(&params).unwrap_or_else(Vec::new)
    } else {
      Vec::new()
    },
    output_file: if let Some(output) = args.output_file {
      let output = PathAbs::new(output).expect(
        "Failed to canonicalize output path: the output file must have a valid parent directory",
      );

      if !output
        .parent()
        .expect("Failed to get parent directory of canonicalized path")
        .exists()
      {
        eprintln!("Path to file is invalid: {:?}", &output);
        std::process::exit(1);
      }

      output.to_str().unwrap().to_owned()
    } else {
      format!(
        "{}_{}.mkv",
        args.input.file_stem().unwrap().to_str().unwrap(),
        args.encoder
      )
    },
    audio_params: if let Some(params) = args.audio_params {
      shlex::split(&params).unwrap_or_else(|| vec!["-c:a".into(), "copy".into()])
    } else {
      vec!["-c:a".into(), "copy".into()]
    },
    ffmpeg_pipe: Vec::new(),
    chunk_method: args
      .chunk_method
      .unwrap_or_else(|| vapoursynth::select_chunk_method().unwrap()),
    concat: args.concat,
    encoder: args.encoder,
    extra_splits_len: Some(args.extra_split),
    input: args.input.to_str().unwrap().to_owned(),
    keep: args.keep,
    min_q: args.min_q,
    max_q: args.max_q,
    min_scene_len: args.min_scene_len,
    n_threads: args.vmaf_threads,
    pix_format: args.pix_format,
    probe_slow: args.probe_slow,
    probes: args.probes,
    probing_rate: args.probing_rate,
    resume: args.resume,
    scenes: args
      .scenes
      .map(|scenes| scenes.to_str().unwrap().to_owned()),
    split_method: args.split_method,
    target_quality: args.target_quality,
    target_quality_method: Some(args.target_quality_method),
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
    vmaf_res: Some(args.vmaf_res),
    workers: args.workers,
  };

  ctrlc::set_handler(|| {
    println!("Stopped");
    std::process::exit(0);
  })
  .unwrap();

  project.startup_check().unwrap();
  project.encode_file();
}
