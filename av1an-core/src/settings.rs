use crate::parse::valid_params;
use crate::progress_bar::update_progress_bar_estimates;
use crate::progress_bar::{reset_bar_at, reset_mp_bar_at};
use crate::vapoursynth::{is_ffms2_installed, is_lsmash_installed};
use crate::{
  broker::{Broker, EncoderCrash},
  chunk::Chunk,
  concat::{self, ConcatMethod},
  create_dir, determine_workers, ffmpeg,
  ffmpeg::compose_ffmpeg_pipe,
  finish_multi_progress_bar, get_done, init_done, into_vec,
  progress_bar::{
    finish_progress_bar, inc_bar, inc_mp_bar, init_multi_progress_bar, init_progress_bar,
    update_mp_msg,
  },
  read_chunk_queue, save_chunk_queue,
  scene_detect::av_scenechange_detect,
  split::{extra_splits, segment, write_scenes_to_file},
  vapoursynth::create_vs_file,
  vmaf::{self, validate_libvmaf},
  ChunkMethod, DashMap, DoneJson, Encoder, Input, ScenecutMethod, SplitMethod, TargetQuality,
  Verbosity,
};
use anyhow::{bail, ensure, Context};
use crossbeam_utils;
use ffmpeg_next::format::Pixel;
use itertools::Itertools;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::sync::atomic::AtomicUsize;
use std::thread;
use std::{
  borrow::Cow,
  cmp,
  cmp::Reverse,
  collections::HashSet,
  convert::TryInto,
  ffi::OsString,
  fs,
  fs::File,
  io::Write,
  iter,
  path::{Path, PathBuf},
  process::{exit, Command, Stdio},
  sync::atomic::{self, AtomicBool, AtomicU64},
  sync::{mpsc, Arc},
};

use ansi_term::{Color, Style};

use tokio::io::{AsyncBufReadExt, BufReader};

pub struct PixelFormat {
  pub format: Pixel,
  pub bit_depth: usize,
}

pub enum InputPixelFormat {
  VapourSynth { bit_depth: usize },
  FFmpeg { format: Pixel },
}

#[allow(clippy::struct_excessive_bools)]
pub struct EncodeArgs {
  pub frames: usize,

  pub input: Input,
  pub temp: String,
  pub output_file: String,

  pub vs_script: Option<PathBuf>,

  pub chunk_method: ChunkMethod,
  pub scenes: Option<PathBuf>,
  pub split_method: SplitMethod,
  pub sc_pix_format: Option<Pixel>,
  pub sc_method: ScenecutMethod,
  pub sc_downscale_height: Option<usize>,
  pub extra_splits_len: Option<usize>,
  pub min_scene_len: usize,

  pub max_tries: usize,

  pub passes: u8,
  pub video_params: Vec<String>,
  pub encoder: Encoder,
  pub workers: usize,
  pub set_thread_affinity: Option<usize>,

  // FFmpeg params
  pub ffmpeg_filter_args: Vec<String>,
  pub audio_params: Vec<String>,
  pub input_pix_format: InputPixelFormat,
  pub output_pix_format: PixelFormat,

  pub verbosity: Verbosity,
  pub log_file: PathBuf,
  pub resume: bool,
  pub keep: bool,
  pub force: bool,

  pub vmaf: bool,
  pub vmaf_path: Option<PathBuf>,
  pub vmaf_res: String,

  pub concat: ConcatMethod,

  pub target_quality: Option<f64>,
  pub probes: u32,
  pub probe_slow: bool,
  pub min_q: Option<u32>,
  pub max_q: Option<u32>,

  pub probing_rate: u32,
  pub vmaf_threads: Option<usize>,
  pub vmaf_filter: Option<String>,
}

impl EncodeArgs {
  /// Initialize logging routines and create temporary directories
  pub fn initialize(&mut self) -> anyhow::Result<()> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::level::Level::Fatal);

    if !self.resume && Path::new(&self.temp).is_dir() {
      fs::remove_dir_all(&self.temp)
        .with_context(|| format!("Failed to remove temporary directory {:?}", &self.temp))?;
    }

    create_dir!(Path::new(&self.temp))?;
    create_dir!(Path::new(&self.temp).join("split"))?;
    create_dir!(Path::new(&self.temp).join("encode"))?;

    info!("temporary directory: {}", &self.temp);

    let done_json_exists = Path::new(&self.temp).join("done.json").exists();
    let chunks_json_exists = Path::new(&self.temp).join("chunks.json").exists();

    if self.resume {
      match (done_json_exists, chunks_json_exists) {
        // both files exist, so there is no problem
        (true, true) => {}
        (false, true) => {
          info!(
            "resume was set but done.json does not exist in temporary directory {:?}",
            &self.temp
          );
          self.resume = false;
        }
        (true, false) => {
          info!(
            "resume was set but chunks.json does not exist in temporary directory {:?}",
            &self.temp
          );
          self.resume = false;
        }
        (false, false) => {
          info!(
            "resume was set but neither chunks.json nor done.json exist in temporary directory {:?}",
            &self.temp
          );
          self.resume = false;
        }
      }
    }

    Ok(())
  }

  fn read_queue_files(source_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut queue_files = fs::read_dir(&source_path)
      .with_context(|| {
        format!(
          "Failed to read queue files from source path {:?}",
          source_path
        )
      })?
      .map(|res| res.map(|e| e.path()))
      .collect::<Result<Vec<_>, _>>()?;

    queue_files.retain(|file| {
      file.is_file() && matches!(file.extension().map(|ext| ext == "mkv"), Some(true))
    });
    concat::sort_files_by_filename(&mut queue_files);

    Ok(queue_files)
  }

  /// Returns the number of frames encoded if crashed, to reset the progress bar.
  pub fn create_pipes(
    &self,
    chunk: &Chunk,
    current_pass: u8,
    worker_id: usize,
    tpl_crash_workaround: bool,
  ) -> Result<(), (EncoderCrash, u64)> {
    let fpf_file = Path::new(&chunk.temp)
      .join("split")
      .join(format!("{}_fpf", chunk.name()));

    let mut video_params = self.video_params.clone();
    if tpl_crash_workaround {
      // In aomenc for duplicate arguments, whichever is specified last takes precedence.
      video_params.push("--enable-tpl-model=0".to_string());
    }
    let mut enc_cmd = if self.passes == 1 {
      self.encoder.compose_1_1_pass(video_params, chunk.output())
    } else if current_pass == 1 {
      self
        .encoder
        .compose_1_2_pass(video_params, fpf_file.to_str().unwrap())
    } else {
      self
        .encoder
        .compose_2_2_pass(video_params, fpf_file.to_str().unwrap(), chunk.output())
    };

    if let Some(per_shot_target_quality_cq) = chunk.tq_cq {
      enc_cmd = self
        .encoder
        .man_command(enc_cmd, per_shot_target_quality_cq as usize);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    let (pipe_stderr, enc_output, enc_stderr, frame) = rt.block_on(async {
      let mut source_pipe = if let [source, args @ ..] = &*chunk.source {
        tokio::process::Command::new(source)
          .args(args)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let source_pipe_stdout: Stdio = source_pipe.stdout.take().unwrap().try_into().unwrap();

      // converts the pixel format
      let create_ffmpeg_pipe = |pipe_from: Stdio| {
        let ffmpeg_pipe = compose_ffmpeg_pipe(
          self.ffmpeg_filter_args.as_slice(),
          self.output_pix_format.format,
        );

        let mut ffmpeg_pipe = if let [ffmpeg, args @ ..] = &*ffmpeg_pipe {
          tokio::process::Command::new(ffmpeg)
            .args(args)
            .stdin(pipe_from)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
        } else {
          unreachable!()
        };

        let ffmpeg_pipe_stdout: Stdio = ffmpeg_pipe.stdout.take().unwrap().try_into().unwrap();
        let ffmpeg_pipe_stderr = ffmpeg_pipe.stderr.take().unwrap();
        (ffmpeg_pipe_stdout, ffmpeg_pipe_stderr)
      };

      let source_pipe_stderr = source_pipe.stderr.take().unwrap();

      let (y4m_pipe, y4m_pipe_stderr) = if self.ffmpeg_filter_args.is_empty() {
        match &self.input_pix_format {
          InputPixelFormat::FFmpeg { format } => {
            if self.output_pix_format.format == *format {
              (source_pipe_stdout, source_pipe_stderr)
            } else {
              create_ffmpeg_pipe(source_pipe_stdout)
            }
          }
          InputPixelFormat::VapourSynth { bit_depth } => {
            if self.output_pix_format.bit_depth == *bit_depth {
              (source_pipe_stdout, source_pipe_stderr)
            } else {
              create_ffmpeg_pipe(source_pipe_stdout)
            }
          }
        }
      } else {
        create_ffmpeg_pipe(source_pipe_stdout)
      };

      let mut reader = BufReader::new(y4m_pipe_stderr).lines();

      let pipe_stderr = Arc::new(parking_lot::Mutex::new(String::with_capacity(128)));
      let p_stdr2 = Arc::clone(&pipe_stderr);

      tokio::spawn(async move {
        while let Some(line) = reader.next_line().await.unwrap() {
          p_stdr2.lock().push_str(&line);
          p_stdr2.lock().push('\n');
        }
      });

      let mut enc_pipe = if let [encoder, args @ ..] = &*enc_cmd {
        tokio::process::Command::new(encoder)
          .args(args)
          .stdin(y4m_pipe)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let mut frame = 0;

      let mut reader = BufReader::new(enc_pipe.stderr.take().unwrap());

      let mut buf = Vec::with_capacity(128);
      let mut enc_stderr = String::with_capacity(128);

      while let Ok(read) = reader.read_until(b'\r', &mut buf).await {
        if read == 0 {
          break;
        }

        if let Ok(line) = simdutf8::basic::from_utf8_mut(&mut buf) {
          if self.verbosity == Verbosity::Verbose && !line.contains('\n') {
            update_mp_msg(worker_id, (*line).to_string());
          }
          // This needs to be done before parse_encoded_frames, as it potentially
          // mutates the string
          enc_stderr.push_str(line);
          enc_stderr.push('\n');

          // SAFETY: we do not read the contents of `line` after this function call
          if let Some(new) = unsafe { self.encoder.parse_encoded_frames(line) } {
            // clippy is actually wrong that this does nothing, dropping a mutable
            // reference does prevent it from being used again.
            drop(line);
            if new > frame {
              if self.verbosity == Verbosity::Normal {
                inc_bar((new - frame) as u64);
              } else if self.verbosity == Verbosity::Verbose {
                inc_mp_bar((new - frame) as u64);
              }
              frame = new;
            }
          }
        }

        buf.clear();
      }

      let enc_output = enc_pipe.wait_with_output().await.unwrap();

      let pipe_stderr = pipe_stderr.lock().clone();
      (pipe_stderr, enc_output, enc_stderr, frame)
    });

    if !enc_output.status.success() {
      return Err((
        EncoderCrash {
          exit_status: enc_output.status,
          pipe_stderr: pipe_stderr.into(),
          stderr: enc_stderr.into(),
          stdout: enc_output.stdout.into(),
        },
        frame,
      ));
    }

    Ok(())
  }

  fn validate_encoder_params(&self) {
    #[must_use]
    fn invalid_params<'a>(
      params: &'a [&'a str],
      valid_options: &'a HashSet<Cow<'a, str>>,
    ) -> Vec<&'a str> {
      params
        .iter()
        .filter(|param| !valid_options.contains(Borrow::<str>::borrow(&**param)))
        .copied()
        .collect()
    }

    #[must_use]
    fn suggest_fix<'a>(
      wrong_arg: &str,
      arg_dictionary: &'a HashSet<Cow<'a, str>>,
    ) -> Option<&'a str> {
      // Minimum threshold to consider a suggestion similar enough that it could be a typo
      const MIN_THRESHOLD: f64 = 0.75;

      arg_dictionary
        .iter()
        .map(|arg| (arg, strsim::jaro_winkler(arg, wrong_arg)))
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Less))
        .and_then(|(suggestion, score)| {
          if score > MIN_THRESHOLD {
            Some(suggestion.borrow())
          } else {
            None
          }
        })
    }

    let video_params: Vec<&str> = self
      .video_params
      .as_slice()
      .iter()
      .filter_map(|param| {
        if param.starts_with('-') && [Encoder::aom, Encoder::vpx].contains(&self.encoder) {
          // These encoders require args to be passed using an equal sign,
          // e.g. `--cq-level=30`
          param.split('=').next()
        } else {
          // The other encoders use a space, so we don't need to do extra splitting,
          // e.g. `--crf 30`
          None
        }
      })
      .collect();

    let help_text = {
      let [cmd, arg] = self.encoder.help_command();
      String::from_utf8(Command::new(cmd).arg(arg).output().unwrap().stdout).unwrap()
    };
    let valid_params = valid_params(&help_text, self.encoder);
    let invalid_params = invalid_params(&video_params, &valid_params);

    for wrong_param in &invalid_params {
      eprintln!(
        "'{}' isn't a valid parameter for {}",
        wrong_param, self.encoder,
      );
      if let Some(suggestion) = suggest_fix(wrong_param, &valid_params) {
        eprintln!("\tDid you mean '{}'?", suggestion);
      }
    }

    if !invalid_params.is_empty() {
      println!("\nTo continue anyway, run av1an with '--force'");
      exit(1);
    }
  }

  pub fn startup_check(&mut self) -> anyhow::Result<()> {
    if self.concat == ConcatMethod::Ivf
      && !matches!(
        self.encoder,
        Encoder::rav1e | Encoder::aom | Encoder::svt_av1 | Encoder::vpx
      )
    {
      bail!(".ivf only supports VP8, VP9, and AV1");
    }

    ensure!(self.max_tries > 0);

    ensure!(
      self.input.as_path().exists(),
      "Input file {:?} does not exist!",
      self.input
    );

    if self.target_quality.is_some() || self.vmaf {
      validate_libvmaf()?;
    }

    if which::which("ffmpeg").is_err() {
      bail!("FFmpeg not found. Is it installed in system path?");
    }

    if self.concat == ConcatMethod::MKVMerge && which::which("mkvmerge").is_err() {
      bail!("mkvmerge not found, but `--concat mkvmerge` was specified. Is it installed in system path?");
    }

    if self.encoder == Encoder::x265 && self.concat != ConcatMethod::MKVMerge {
      bail!("mkvmerge is required for concatenating x265, as x265 outputs raw HEVC bitstream files without the timestamps correctly set, which FFmpeg cannot concatenate \
properly into a mkv file. Specify mkvmerge as the concatenation method by setting `--concat mkvmerge`.");
    }

    if self.chunk_method == ChunkMethod::LSMASH {
      ensure!(
        is_lsmash_installed(),
        "LSMASH is not installed, but it was specified as the chunk method"
      );
    }
    if self.chunk_method == ChunkMethod::FFMS2 {
      ensure!(
        is_ffms2_installed(),
        "FFMS2 is not installed, but it was specified as the chunk method"
      );
    }

    if let Some(vmaf_path) = &self.vmaf_path {
      ensure!(vmaf_path.exists());
    }

    if self.probes < 4 {
      println!("Target quality with less than 4 probes is experimental and not recommended");
    }

    let (min, max) = self.encoder.get_default_cq_range();
    match self.min_q {
      None => {
        self.min_q = Some(min as u32);
      }
      Some(min_q) => ensure!(min_q > 1),
    }

    if self.max_q.is_none() {
      self.max_q = Some(max as u32);
    }

    let encoder_bin = self.encoder.bin();
    if which::which(encoder_bin).is_err() {
      bail!(
        "Encoder {} not found. Is it installed in the system path?",
        encoder_bin
      );
    }

    if self.video_params.is_empty() {
      self.video_params = self.encoder.get_default_arguments();
    }

    if self.encoder == Encoder::aom
      && self.concat != ConcatMethod::MKVMerge
      && self
        .video_params
        .iter()
        .any(|param| param == "--enable-keyframe-filtering=2")
    {
      bail!(
        "keyframe filtering mode 2 currently only works when using mkvmerge as the concat method"
      );
    }

    if matches!(self.encoder, Encoder::aom | Encoder::vpx)
      && self.passes != 1
      && self.video_params.iter().any(|param| param == "--rt")
    {
      // --rt must be used with 1-pass mode
      self.passes = 1;
    }

    if !self.force {
      self.validate_encoder_params();
    }

    Ok(())
  }

  fn create_encoding_queue(&mut self, splits: Vec<usize>) -> anyhow::Result<Vec<Chunk>> {
    let mut chunks = match &self.input {
      Input::Video(_) => match self.chunk_method {
        ChunkMethod::FFMS2 | ChunkMethod::LSMASH => {
          let vs_script = self.vs_script.as_ref().unwrap().as_path();
          self.create_video_queue_vs(splits, vs_script)
        }
        ChunkMethod::Hybrid => self.create_video_queue_hybrid(splits)?,
        ChunkMethod::Select => self.create_video_queue_select(splits),
        ChunkMethod::Segment => self.create_video_queue_segment(&splits)?,
      },
      Input::VapourSynth(vs_script) => self.create_video_queue_vs(splits, vs_script.as_path()),
    };

    chunks.sort_unstable_by_key(|chunk| Reverse(chunk.frames));

    Ok(chunks)
  }

  fn calc_split_locations(&self) -> anyhow::Result<(Vec<usize>, usize)> {
    Ok(match self.split_method {
      SplitMethod::AvScenechange => av_scenechange_detect(
        &self.input,
        self.encoder,
        self.frames,
        self.min_scene_len,
        self.verbosity,
        self.sc_pix_format,
        self.sc_method,
        self.sc_downscale_height,
      )?,
      SplitMethod::None => (Vec::new(), self.input.frames()),
    })
  }

  // If we are not resuming, then do scene detection. Otherwise: get scenes from
  // scenes.json and return that.
  fn split_routine(&mut self) -> anyhow::Result<Vec<usize>> {
    let scene_file = self.scenes.as_ref().map_or_else(
      || Cow::Owned(Path::new(&self.temp).join("scenes.json")),
      |path| Cow::Borrowed(path.as_path()),
    );

    let (mut scenes, frames) = if (self.scenes.is_some() && scene_file.exists()) || self.resume {
      crate::split::read_scenes_from_file(scene_file.as_ref())?
    } else {
      self.calc_split_locations()?
    };
    self.frames = frames;
    get_done()
      .frames
      .store(self.frames, atomic::Ordering::SeqCst);
    let scenes_before = scenes.len() + 1;
    if let Some(split_len) = self.extra_splits_len {
      scenes = extra_splits(&scenes, self.frames, split_len);
      let scenes_after = scenes.len() + 1;
      info!(
        "scenecut: found {} scene(s) [with extra_splits({} frames): {}]",
        scenes_before, split_len, scenes_after
      );
    } else {
      info!("scenecut: found {} scene(s)", scenes_before);
    }

    write_scenes_to_file(&scenes, self.frames, scene_file)?;

    Ok(scenes)
  }

  fn create_select_chunk(
    &self,
    index: usize,
    src_path: &Path,
    frame_start: usize,
    mut frame_end: usize,
  ) -> Chunk {
    assert!(
      frame_start < frame_end,
      "Can't make a chunk with <= 0 frames!"
    );

    let frames = frame_end - frame_start;
    frame_end -= 1;

    let ffmpeg_gen_cmd: Vec<OsString> = into_vec![
      "ffmpeg",
      "-y",
      "-hide_banner",
      "-loglevel",
      "error",
      "-i",
      src_path,
      "-vf",
      format!(
        "select=between(n\\,{}\\,{}),setpts=PTS-STARTPTS",
        frame_start, frame_end
      ),
      "-pix_fmt",
      self.output_pix_format.format.descriptor().unwrap().name(),
      "-strict",
      "-1",
      "-f",
      "yuv4mpegpipe",
      "-",
    ];

    let output_ext = self.encoder.output_extension();

    Chunk {
      temp: self.temp.clone(),
      index,
      source: ffmpeg_gen_cmd,
      output_ext: output_ext.to_owned(),
      frames,
      ..Chunk::default()
    }
  }

  fn create_vs_chunk(
    &self,
    index: usize,
    vs_script: &Path,
    frame_start: usize,
    mut frame_end: usize,
  ) -> Chunk {
    assert!(
      frame_start < frame_end,
      "Can't make a chunk with <= 0 frames!"
    );

    let frames = frame_end - frame_start;
    // the frame end boundary is actually a frame that should be included in the next chunk
    frame_end -= 1;

    let vspipe_cmd_gen: Vec<OsString> = into_vec![
      "vspipe",
      vs_script,
      "-y",
      "-",
      "-s",
      format!("{}", frame_start),
      "-e",
      format!("{}", frame_end),
    ];

    let output_ext = self.encoder.output_extension();

    Chunk {
      temp: self.temp.clone(),
      index,
      source: vspipe_cmd_gen,
      output_ext: output_ext.to_owned(),
      frames,
      ..Chunk::default()
    }
  }

  fn create_video_queue_vs(&self, splits: Vec<usize>, vs_script: &Path) -> Vec<Chunk> {
    let chunk_boundaries = iter::once(0)
      .chain(splits)
      .chain(iter::once(self.frames))
      .tuple_windows();

    let chunk_queue: Vec<Chunk> = chunk_boundaries
      .enumerate()
      .map(|(index, (frame_start, frame_end))| {
        self.create_vs_chunk(index, vs_script, frame_start, frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_select(&self, splits: Vec<usize>) -> Vec<Chunk> {
    let last_frame = self.frames;

    let input = self.input.as_video_path();

    let chunk_boundaries = iter::once(0)
      .chain(splits)
      .chain(iter::once(last_frame))
      .tuple_windows();

    let chunk_queue: Vec<Chunk> = chunk_boundaries
      .enumerate()
      .map(|(index, (frame_start, frame_end))| {
        self.create_select_chunk(index, input, frame_start, frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_segment(&self, splits: &[usize]) -> anyhow::Result<Vec<Chunk>> {
    let input = self.input.as_video_path();

    info!("Split video");
    segment(input, &self.temp, splits);
    info!("Split done");

    let source_path = Path::new(&self.temp).join("split");
    let queue_files = Self::read_queue_files(&source_path)?;

    assert!(
      !queue_files.is_empty(),
      "Error: No files found in temp/split, probably splitting not working"
    );

    let chunk_queue: Vec<Chunk> = queue_files
      .iter()
      .enumerate()
      .map(|(index, file)| self.create_chunk_from_segment(index, file.as_path().to_str().unwrap()))
      .collect();

    Ok(chunk_queue)
  }

  fn create_video_queue_hybrid(&self, split_locations: Vec<usize>) -> anyhow::Result<Vec<Chunk>> {
    let mut splits = Vec::with_capacity(2 + split_locations.len());
    splits.push(0);
    splits.extend(split_locations);
    splits.push(self.frames);

    let input = self.input.as_video_path();

    let keyframes = ffmpeg::get_keyframes(input).unwrap();

    let segments_set: Vec<(usize, usize)> = splits.iter().copied().tuple_windows().collect();

    let to_split: Vec<usize> = keyframes
      .iter()
      .filter(|kf| splits.contains(kf))
      .copied()
      .collect();

    info!("Segmenting video");
    segment(input, &self.temp, &to_split[1..]);
    info!("Segment done");

    let source_path = Path::new(&self.temp).join("split");
    let queue_files = Self::read_queue_files(&source_path)?;

    let kf_list = to_split
      .iter()
      .copied()
      .chain(iter::once(self.frames))
      .tuple_windows();

    let mut segments = Vec::with_capacity(segments_set.len());
    for (file, (x, y)) in queue_files.iter().zip(kf_list) {
      for &(s0, s1) in &segments_set {
        if s0 >= x && s1 <= y && s0 < s1 {
          segments.push((file.as_path(), (s0 - x, s1 - x)));
        }
      }
    }

    let chunk_queue: Vec<Chunk> = segments
      .iter()
      .enumerate()
      .map(|(index, &(file, (start, end)))| self.create_select_chunk(index, file, start, end))
      .collect();

    Ok(chunk_queue)
  }

  fn create_chunk_from_segment(&self, index: usize, file: &str) -> Chunk {
    let ffmpeg_gen_cmd: Vec<OsString> = into_vec![
      "ffmpeg",
      "-y",
      "-hide_banner",
      "-loglevel",
      "error",
      "-i",
      file.to_owned(),
      "-strict",
      "-1",
      "-pix_fmt",
      self.output_pix_format.format.descriptor().unwrap().name(),
      "-f",
      "yuv4mpegpipe",
      "-",
    ];

    let output_ext = self.encoder.output_extension();

    Chunk {
      temp: self.temp.clone(),
      frames: self.frames,
      source: ffmpeg_gen_cmd,
      output_ext: output_ext.to_owned(),
      index,
      ..Chunk::default()
    }
  }

  fn load_or_gen_chunk_queue(&mut self, splits: Vec<usize>) -> anyhow::Result<Vec<Chunk>> {
    if self.resume {
      let mut chunks = read_chunk_queue(self.temp.as_ref())?;

      let done = get_done();

      // only keep the chunks that are not done
      chunks.retain(|chunk| !done.done.contains_key(&chunk.name()));

      Ok(chunks)
    } else {
      let chunks = self.create_encoding_queue(splits)?;
      save_chunk_queue(&self.temp, &chunks)?;
      Ok(chunks)
    }
  }

  pub fn encode_file(&mut self) -> anyhow::Result<()> {
    let done_path = Path::new(&self.temp).join("done.json");

    // TODO move this to `initialize`?
    let initial_frames = if self.resume && done_path.exists() {
      let done =
        fs::read_to_string(done_path).with_context(|| "Failed to read contents of done.json")?;
      let done: DoneJson =
        serde_json::from_str(&done).with_context(|| "Failed to parse done.json")?;
      self.frames = done.frames.load(atomic::Ordering::Relaxed);

      // frames need to be recalculated in this case
      if self.frames == 0 {
        self.frames = self.input.frames();
        done.frames.store(self.frames, atomic::Ordering::Relaxed);
      }

      init_done(done)
        .done
        .iter()
        .map(|ref_multi| ref_multi.frames)
        .sum()
    } else {
      init_done(DoneJson {
        frames: AtomicUsize::new(0),
        done: DashMap::new(),
        audio_done: AtomicBool::new(false),
      });

      let mut done_file = fs::File::create(&done_path).unwrap();
      done_file.write_all(serde_json::to_string(get_done())?.as_bytes())?;

      0
    };

    let vspipe_cache =
      // Technically we should check if the vapoursynth cache file exists rather than !self.resume,
      // but the code still works if we are resuming and the cache file doesn't exist (as it gets
      // generated when vspipe is first called), so it's not worth adding all the extra complexity.
      if (self.input.is_vapoursynth()
        || (self.input.is_video()
          && matches!(self.chunk_method, ChunkMethod::LSMASH | ChunkMethod::FFMS2)))
        && !self.resume
      {
        self.vs_script = Some(match &self.input {
          Input::VapourSynth(path) => path.clone(),
          Input::Video(path) => create_vs_file(&self.temp, path, self.chunk_method)?,
        });

        let vs_script = self.vs_script.clone().unwrap();
        Some({
          thread::spawn(move || {
            Command::new("vspipe")
              .arg("-i")
              .arg(vs_script)
              .args(["-i", "-"])
              .stdout(Stdio::piped())
              .stderr(Stdio::piped())
              .spawn()
              .unwrap()
              .wait()
              .unwrap()
          })
        })
      } else {
        None
      };

    let splits = self.split_routine()?;

    let chunk_queue = self.load_or_gen_chunk_queue(splits)?;
    let num_chunks = chunk_queue.len();

    if self.resume {
      let chunks_done = get_done().done.len();
      info!(
        "encoding resumed with {}/{} chunks completed ({} remaining)",
        chunks_done,
        chunk_queue.len() + chunks_done,
        chunk_queue.len()
      );
    }

    if let Some(vspipe_cache) = vspipe_cache {
      vspipe_cache.join().unwrap();
    }

    crossbeam_utils::thread::scope(|s| -> anyhow::Result<()> {
      // vapoursynth audio is currently unsupported
      let audio_size_bytes = Arc::new(AtomicU64::new(0));
      let audio_thread = if self.input.is_video()
        && (!self.resume || !get_done().audio_done.load(atomic::Ordering::SeqCst))
      {
        let input = self.input.as_video_path();
        let temp = self.temp.as_str();
        let audio_params = self.audio_params.as_slice();
        let audio_size_ref = Arc::clone(&audio_size_bytes);
        Some(s.spawn(move |_| {
          let audio_output = ffmpeg::encode_audio(input, temp, audio_params);
          get_done().audio_done.store(true, atomic::Ordering::SeqCst);

          let progress_file = Path::new(temp).join("done.json");
          let mut progress_file = File::create(&progress_file).unwrap();
          progress_file
            .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
            .unwrap();

          if let Some(ref audio_output) = audio_output {
            audio_size_ref.store(
              audio_output.metadata().unwrap().len(),
              atomic::Ordering::SeqCst,
            );
          }

          audio_output.is_some()
        }))
      } else {
        None
      };

      if self.workers == 0 {
        self.workers = determine_workers(self.encoder) as usize;
      }
      self.workers = cmp::min(self.workers, chunk_queue.len());

      eprintln!(
        "{}{} {} {}{} {} {}{} {}\n{}: {}",
        Color::Green.bold().paint("Q"),
        Color::Green.paint("ueue"),
        Color::Green.bold().paint(format!("{}", chunk_queue.len())),
        Color::Blue.bold().paint("W"),
        Color::Blue.paint("orkers"),
        Color::Blue.bold().paint(format!("{}", self.workers)),
        Color::Purple.bold().paint("P"),
        Color::Purple.paint("asses"),
        Color::Purple.bold().paint(format!("{}", self.passes)),
        Style::default().bold().paint("Params"),
        Style::default().dimmed().paint(self.video_params.join(" "))
      );

      if self.verbosity == Verbosity::Normal {
        init_progress_bar(self.frames as u64);
        reset_bar_at(initial_frames as u64);
      } else if self.verbosity == Verbosity::Verbose {
        init_multi_progress_bar(self.frames as u64, self.workers);
        reset_mp_bar_at(initial_frames as u64);
      }

      if !get_done().done.is_empty() {
        let frame_rate = self.input.frame_rate();
        update_progress_bar_estimates(
          frame_rate,
          self.frames,
          self.verbosity,
          audio_size_bytes.load(atomic::Ordering::SeqCst),
        );
      }

      let broker = Broker {
        chunk_queue,
        project: self,
        target_quality: if self.target_quality.is_some() {
          Some(TargetQuality::new(self))
        } else {
          None
        },
        max_tries: self.max_tries,
      };

      let audio_size_ref = Arc::clone(&audio_size_bytes);
      let (tx, rx) = mpsc::channel();
      let handle = s.spawn(|_| {
        broker.encoding_loop(tx, self.set_thread_affinity, audio_size_ref);
      });

      // Queue::encoding_loop only sends a message if there was an error (meaning a chunk crashed)
      // more than MAX_TRIES. So, we have to explicitly exit the program if that happens.
      while rx.recv().is_ok() {
        exit(1);
      }

      handle.join().unwrap();

      if self.verbosity == Verbosity::Normal {
        finish_progress_bar();
      } else if self.verbosity == Verbosity::Verbose {
        finish_multi_progress_bar();
      }

      // TODO add explicit parameter to concatenation functions to control whether audio is also muxed in
      let _audio_output_exists =
        audio_thread.map_or(false, |audio_thread| audio_thread.join().unwrap());

      info!("encoding finished, concatenating with {}", self.concat);

      match self.concat {
        ConcatMethod::Ivf => {
          concat::ivf(
            &Path::new(&self.temp).join("encode"),
            self.output_file.as_ref(),
          )?;
        }
        ConcatMethod::MKVMerge => {
          concat::mkvmerge(
            self.temp.as_ref(),
            self.output_file.as_ref(),
            self.encoder,
            num_chunks,
          )?;
        }
        ConcatMethod::FFmpeg => {
          concat::ffmpeg(self.temp.as_ref(), self.output_file.as_ref())?;
        }
      }

      if self.vmaf {
        if let Err(e) = vmaf::plot(
          self.output_file.as_ref(),
          &self.input,
          self.vmaf_path.as_deref(),
          self.vmaf_res.as_str(),
          1,
          match self.vmaf_filter.as_deref() {
            Some(filter) => Some(filter),
            None => None,
          },
          self.vmaf_threads.unwrap_or_else(num_cpus::get),
        ) {
          error!("{}", e);
          error!("VMAF calculation failed, no plot generated.");
        }
      }

      if !Path::new(&self.output_file).exists() {
        warn!(
          "Concatenation failed for unknown reasons! Temp folder will not be deleted: {}",
          &self.temp
        );
      } else if !self.keep {
        if let Err(e) = fs::remove_dir_all(&self.temp) {
          warn!("Failed to delete temp directory: {}", e);
        }
      }

      Ok(())
    })
    .unwrap()?;

    Ok(())
  }
}
