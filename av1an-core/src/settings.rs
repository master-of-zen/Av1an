use crate::{
  broker::Broker,
  chunk::Chunk,
  concat::{self, ConcatMethod},
  create_dir, determine_workers, ffmpeg,
  ffmpeg::compose_ffmpeg_pipe,
  finish_multi_progress_bar, get_done, hash_path, init_done, into_vec,
  progress_bar::{
    finish_progress_bar, init_multi_progress_bar, init_progress_bar, update_bar, update_mp_bar,
    update_mp_msg,
  },
  read_chunk_queue, regex, save_chunk_queue,
  scene_detect::av_scenechange_detect,
  split::{extra_splits, segment, write_scenes_to_file},
  vapoursynth,
  vapoursynth::create_vs_file,
  vmaf, ChunkMethod, DashMap, DoneJson, Encoder, Input, ScenecutMethod, SplitMethod, TargetQuality,
  Verbosity,
};
use anyhow::{bail, ensure};
use crossbeam_utils;
use flexi_logger::{Duplicate, FileSpec, Logger};
use itertools::Itertools;
use path_abs::PathAbs;
use std::{
  borrow::Cow,
  cmp,
  cmp::{Ordering, Reverse},
  collections::HashSet,
  convert::TryInto,
  ffi::OsString,
  fs,
  fs::File,
  io::Write,
  iter,
  path::{Path, PathBuf},
  process::{Command, ExitStatus, Stdio},
  sync::atomic::{AtomicBool, AtomicUsize},
  sync::{atomic, mpsc},
};
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct EncodeArgs {
  pub frames: usize,

  pub input: Input,
  pub temp: String,
  pub output_file: String,

  pub chunk_method: ChunkMethod,
  pub scenes: Option<PathBuf>,
  pub split_method: SplitMethod,
  pub sc_method: ScenecutMethod,
  pub sc_downscale_height: Option<usize>,
  pub extra_splits_len: Option<usize>,
  pub min_scene_len: usize,

  pub passes: u8,
  pub video_params: Vec<String>,
  pub encoder: Encoder,
  pub workers: usize,

  // FFmpeg params
  pub ffmpeg_pipe: Vec<String>,
  pub ffmpeg: Vec<String>,
  pub audio_params: Vec<String>,
  pub pix_format: String,

  pub verbosity: Verbosity,
  pub logging: PathBuf,
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

    info!("File hash: {}", hash_path(self.input.as_path()));

    self.resume = self.resume && Path::new(&self.temp).join("done.json").exists();

    if !self.resume && Path::new(&self.temp).is_dir() {
      if let Err(e) = fs::remove_dir_all(&self.temp) {
        warn!("Failed to delete temp directory: {}", e);
      }
    }

    create_dir!(&self.temp)?;
    create_dir!(Path::new(&self.temp).join("split"))?;
    create_dir!(Path::new(&self.temp).join("encode"))?;

    Logger::try_with_str("info")
      .unwrap()
      .log_to_file(FileSpec::try_from(PathAbs::new(&self.logging).unwrap()).unwrap())
      .duplicate_to_stderr(Duplicate::Warn)
      .start()?;

    Ok(())
  }

  fn read_queue_files(source_path: &Path) -> Vec<PathBuf> {
    let mut queue_files = fs::read_dir(&source_path)
      .unwrap()
      .map(|res| res.map(|e| e.path()))
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    queue_files.retain(|file| {
      file.is_file() && matches!(file.extension().map(|ext| ext == "mkv"), Some(true))
    });
    concat::sort_files_by_filename(&mut queue_files);

    queue_files
  }

  pub fn create_pipes(
    &self,
    chunk: &Chunk,
    current_pass: u8,
    worker_id: usize,
  ) -> Result<(), (ExitStatus, String)> {
    let fpf_file = Path::new(&chunk.temp)
      .join("split")
      .join(format!("{}_fpf", chunk.name()));

    let mut enc_cmd = if self.passes == 1 {
      self
        .encoder
        .compose_1_1_pass(self.video_params.clone(), chunk.output())
    } else if current_pass == 1 {
      self.encoder.compose_1_2_pass(
        self.video_params.clone(),
        &fpf_file.to_str().unwrap().to_owned(),
      )
    } else {
      self.encoder.compose_2_2_pass(
        self.video_params.clone(),
        &fpf_file.to_str().unwrap().to_owned(),
        chunk.output(),
      )
    };

    if let Some(per_shot_target_quality_cq) = chunk.per_shot_target_quality_cq {
      enc_cmd = self
        .encoder
        .man_command(enc_cmd, per_shot_target_quality_cq as usize);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    let (exit_status, output) = rt.block_on(async {
      let mut ffmpeg_gen_pipe = if let [source, args @ ..] = &*chunk.source {
        tokio::process::Command::new(source)
          .args(args)
          .stdout(Stdio::piped())
          .stderr(Stdio::null())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let ffmpeg_gen_pipe_stdout: Stdio =
        ffmpeg_gen_pipe.stdout.take().unwrap().try_into().unwrap();

      let ffmpeg_pipe = compose_ffmpeg_pipe(self.ffmpeg_pipe.clone());

      let mut ffmpeg_pipe = if let [ffmpeg, args @ ..] = &*ffmpeg_pipe {
        tokio::process::Command::new(ffmpeg)
          .args(args)
          .stdin(ffmpeg_gen_pipe_stdout)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let ffmpeg_pipe_stdout: Stdio = ffmpeg_pipe.stdout.take().unwrap().try_into().unwrap();

      let mut pipe = if let [encoder, args @ ..] = &*enc_cmd {
        tokio::process::Command::new(encoder)
          .args(args)
          .stdin(ffmpeg_pipe_stdout)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let mut frame = 0;

      let mut reader = BufReader::new(pipe.stderr.take().unwrap());

      let mut buf = Vec::with_capacity(64);
      let mut output = String::with_capacity(64);

      while let Ok(read) = reader.read_until(b'\r', &mut buf).await {
        if read == 0 {
          break;
        }

        if let Ok(line) = std::str::from_utf8(&buf) {
          if self.verbosity == Verbosity::Verbose && !line.contains('\n') {
            update_mp_msg(worker_id, line.to_string());
          }
          if let Some(new) = self.encoder.match_line(line) {
            if new > frame {
              if self.verbosity == Verbosity::Normal {
                update_bar((new - frame) as u64);
              } else if self.verbosity == Verbosity::Verbose {
                update_mp_bar((new - frame) as u64);
              }
              frame = new;
            }
          }
          output.push_str(line);
          output.push('\n');
        }

        buf.clear();
      }

      let exit_status = pipe.wait_with_output().await.unwrap().status;

      drop(ffmpeg_gen_pipe.kill().await);
      drop(ffmpeg_pipe.kill().await);

      (exit_status, output)
    });

    if !exit_status.success() {
      return Err((exit_status, output));
    }

    Ok(())
  }

  fn get_frames(&mut self) -> usize {
    if self.frames != 0 {
      return self.frames;
    }

    self.frames = match &self.input {
      Input::Video(path) => {
        if matches!(self.chunk_method, ChunkMethod::FFMS2 | ChunkMethod::LSMASH) {
          let script = create_vs_file(&self.temp, path.as_ref(), self.chunk_method).unwrap();
          vapoursynth::num_frames(script.as_ref()).unwrap()
        } else {
          ffmpeg::num_frames(path.as_ref()).unwrap()
        }
      }
      Input::VapourSynth(path) => vapoursynth::num_frames(path.as_ref()).unwrap(),
    };

    self.frames
  }

  fn validate_encoder_params(&self) {
    /// Returns the set of valid parameters given a help text of an encoder
    #[must_use]
    fn valid_params(help_text: &str) -> HashSet<&str> {
      regex!(r"\s+(-\w+|(?:--\w+(?:-\w+)*))")
        .find_iter(help_text)
        .filter_map(|m| m.as_str().split_ascii_whitespace().next())
        .collect()
    }

    #[must_use]
    fn invalid_params<'a>(
      params: &'a [&'a str],
      valid_options: &'a HashSet<&'a str>,
    ) -> Vec<&'a str> {
      params
        .iter()
        .filter(|param| !valid_options.contains(*param))
        .copied()
        .collect()
    }

    #[must_use]
    fn suggest_fix<'a>(wrong_arg: &str, arg_dictionary: &'a HashSet<&'a str>) -> Option<&'a str> {
      // Minimum threshold to consider a suggestion similar enough that it could be a typo
      const MIN_THRESHOLD: f64 = 0.75;

      arg_dictionary
        .iter()
        .map(|arg| (arg, strsim::jaro_winkler(arg, wrong_arg)))
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Less))
        .and_then(|(suggestion, score)| {
          if score > MIN_THRESHOLD {
            Some(*suggestion)
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
        if param.starts_with('-') {
          param.split('=').next()
        } else {
          None
        }
      })
      .collect();

    let help_text = {
      let [cmd, arg] = self.encoder.help_command();
      String::from_utf8(Command::new(cmd).arg(arg).output().unwrap().stdout).unwrap()
    };
    let valid_params = valid_params(&help_text);
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
      std::process::exit(1);
    }
  }

  pub fn startup_check(&mut self) -> anyhow::Result<()> {
    if !matches!(
      self.encoder,
      Encoder::rav1e | Encoder::aom | Encoder::svt_av1 | Encoder::vpx
    ) && self.concat == ConcatMethod::Ivf
    {
      bail!(".ivf only supports VP8, VP9, and AV1");
    }

    ensure!(
      self.input.as_path().exists(),
      "Input file {:?} does not exist!",
      self.input
    );

    if which::which("ffmpeg").is_err() {
      bail!("FFmpeg not found. Is it installed in system path?");
    }

    if self.concat == ConcatMethod::MKVMerge && which::which("mkvmerge").is_err() {
      bail!("mkvmerge not found, but `--concat mkvmerge` was specified. Is it installed in system path?");
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
      Some(min_q) => assert!(min_q > 1),
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

    if !self.force {
      self.validate_encoder_params();
    }
    self.initialize().unwrap();

    self.ffmpeg_pipe = self.ffmpeg.clone();
    self.ffmpeg_pipe.extend([
      "-strict".into(),
      "-1".into(),
      "-pix_fmt".into(),
      self.pix_format.clone(),
      "-f".into(),
      "yuv4mpegpipe".into(),
      "-".into(),
    ]);

    Ok(())
  }

  fn create_encoding_queue(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    let mut chunks = match self.chunk_method {
      ChunkMethod::FFMS2 | ChunkMethod::LSMASH => self.create_video_queue_vs(splits),
      ChunkMethod::Hybrid => self.create_video_queue_hybrid(splits),
      ChunkMethod::Select => self.create_video_queue_select(splits),
      ChunkMethod::Segment => self.create_video_queue_segment(&splits),
    };

    chunks.sort_unstable_by_key(|chunk| Reverse(chunk.frames));

    chunks
  }

  fn calc_split_locations(&self) -> Vec<usize> {
    match self.split_method {
      SplitMethod::AvScenechange => av_scenechange_detect(
        &self.input,
        self.frames,
        self.min_scene_len,
        self.verbosity,
        self.sc_method,
        self.sc_downscale_height,
      )
      .unwrap(),
      SplitMethod::None => Vec::new(),
    }
  }

  // If we are not resuming, then do scene detection. Otherwise: get scenes from
  // scenes.json and return that.
  fn split_routine(&mut self) -> Vec<usize> {
    // TODO make self.frames impossible to misuse
    let _ = self.get_frames();

    let scene_file = self
      .scenes
      .clone()
      .unwrap_or_else(|| Path::new(&self.temp).join("scenes.json"));

    let mut scenes = if (self.scenes.is_some() && scene_file.exists()) || self.resume {
      crate::split::read_scenes_from_file(scene_file.as_path())
        .unwrap()
        .0
    } else {
      self.calc_split_locations()
    };
    info!("SC: Found {} scenes", scenes.len() + 1);
    if self.verbosity == Verbosity::Verbose {
      eprintln!("Found {} scenes", scenes.len() + 1);
    }
    if let Some(split_len) = self.extra_splits_len {
      info!("SC: Applying extra splits every {} frames", split_len);
      scenes = extra_splits(&scenes, self.frames, split_len);
      info!("SC: Now at {} scenes", scenes.len() + 1);
      if self.verbosity == Verbosity::Verbose {
        eprintln!("Applying extra splits every {} frames", split_len);
        eprintln!("Now at {} scenes", scenes.len() + 1);
      }
    }

    self.write_scenes_to_file(&scenes, scene_file.to_str().unwrap());

    scenes
  }

  fn write_scenes_to_file(&self, scenes: &[usize], path: &str) {
    write_scenes_to_file(scenes, self.frames, path).unwrap();
  }

  fn create_select_chunk(
    &self,
    index: usize,
    src_path: &Path,
    frame_start: usize,
    mut frame_end: usize,
  ) -> Chunk {
    assert!(
      frame_end > frame_start,
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
      self.pix_format.clone(),
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
      frame_start.to_string(),
      "-e",
      frame_end.to_string(),
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

  fn create_video_queue_vs(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    let last_frame = self.get_frames();

    let mut split_locs = vec![0];
    split_locs.extend(splits);
    split_locs.push(last_frame);

    let chunk_boundaries: Vec<(usize, usize)> = split_locs
      .iter()
      .zip(split_locs.iter().skip(1))
      .map(|(start, end)| (*start, *end))
      .collect();

    let vs_script: Cow<Path> = match &self.input {
      Input::VapourSynth(path) => Cow::Borrowed(path.as_ref()),
      Input::Video(path) => {
        Cow::Owned(create_vs_file(&self.temp, path, self.chunk_method).unwrap())
      }
    };

    let chunk_queue: Vec<Chunk> = chunk_boundaries
      .iter()
      .enumerate()
      .map(|(index, (frame_start, frame_end))| {
        self.create_vs_chunk(index, &vs_script, *frame_start, *frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_select(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    let last_frame = self.get_frames();

    let input = self.input.as_video_path();

    let mut split_locs = vec![0];
    split_locs.extend(splits);
    split_locs.push(last_frame);

    let chunk_boundaries: Vec<(usize, usize)> = split_locs
      .iter()
      .zip(split_locs.iter().skip(1))
      .map(|(start, end)| (*start, *end))
      .collect();

    let chunk_queue: Vec<Chunk> = chunk_boundaries
      .iter()
      .enumerate()
      .map(|(index, (frame_start, frame_end))| {
        self.create_select_chunk(index, input, *frame_start, *frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_segment(&mut self, splits: &[usize]) -> Vec<Chunk> {
    let input = self.input.as_video_path();

    info!("Split video");
    segment(input, &self.temp, splits);
    info!("Split done");

    let source_path = Path::new(&self.temp).join("split");
    let queue_files = Self::read_queue_files(&source_path);

    assert!(
      !queue_files.is_empty(),
      "Error: No files found in temp/split, probably splitting not working"
    );

    let chunk_queue: Vec<Chunk> = queue_files
      .iter()
      .enumerate()
      .map(|(index, file)| self.create_chunk_from_segment(index, file.as_path().to_str().unwrap()))
      .collect();

    chunk_queue
  }

  fn create_video_queue_hybrid(&mut self, split_locations: Vec<usize>) -> Vec<Chunk> {
    let mut splits = Vec::with_capacity(2 + split_locations.len());
    splits.push(0);
    splits.extend(split_locations);
    splits.push(self.get_frames());

    let input = self.input.as_video_path();

    let keyframes = ffmpeg::get_keyframes(input);

    let segments_set: Vec<(usize, usize)> = splits
      .iter()
      .tuple_windows()
      .map(|(&x, &y)| (x, y))
      .collect();

    let to_split: Vec<usize> = keyframes
      .iter()
      .filter(|kf| splits.contains(kf))
      .copied()
      .collect();

    info!("Segmenting video");
    segment(input, &self.temp, &to_split[1..]);
    info!("Segment done");

    let source_path = Path::new(&self.temp).join("split");
    let queue_files = Self::read_queue_files(&source_path);

    let kf_list: Vec<(usize, usize)> = to_split
      .iter()
      .zip(to_split.iter().skip(1).chain(iter::once(&self.frames)))
      .map(|(start, end)| (*start, *end))
      .collect();

    let mut segments = Vec::with_capacity(segments_set.len());
    for (file, (x, y)) in queue_files.iter().zip(kf_list.iter()) {
      for (s0, s1) in &segments_set {
        if s0 >= x && s1 <= y && s0 - x < s1 - x {
          segments.push((file.clone(), (s0 - x, s1 - x)));
        }
      }
    }

    let chunk_queue: Vec<Chunk> = segments
      .iter()
      .enumerate()
      .map(|(index, (file, (start, end)))| {
        self.create_select_chunk(index, file.as_path(), *start, *end)
      })
      .collect();

    chunk_queue
  }

  fn create_chunk_from_segment(&mut self, index: usize, file: &str) -> Chunk {
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
      self.pix_format.clone(),
      "-f",
      "yuv4mpegpipe",
      "-",
    ];

    let output_ext = self.encoder.output_extension();

    Chunk {
      temp: self.temp.clone(),
      frames: self.get_frames(),
      source: ffmpeg_gen_cmd,
      output_ext: output_ext.to_owned(),
      index,
      ..Chunk::default()
    }
  }

  fn load_or_gen_chunk_queue(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    if self.resume {
      let mut chunks = read_chunk_queue(self.temp.as_ref());

      let done = get_done();

      // only keep the chunks that are not done
      chunks.retain(|chunk| !done.done.contains_key(&chunk.name()));

      chunks
    } else {
      let chunks = self.create_encoding_queue(splits);
      save_chunk_queue(&self.temp, &chunks);
      chunks
    }
  }

  pub fn encode_file(&mut self) {
    let done_path = Path::new(&self.temp).join("done.json");

    let splits = self.split_routine();

    let mut initial_frames: usize = 0;

    if self.resume && done_path.exists() {
      info!("Resuming...");

      let done = fs::read_to_string(done_path).unwrap();
      let done: DoneJson = serde_json::from_str(&done).unwrap();
      init_done(done);

      initial_frames = get_done()
        .done
        .iter()
        .map(|ref_multi| *ref_multi.value())
        .sum();
      info!("Resumed with {} encoded clips done", get_done().done.len());
    } else {
      let total = self.get_frames();

      init_done(DoneJson {
        frames: AtomicUsize::new(total),
        done: DashMap::new(),
        audio_done: AtomicBool::new(false),
      });

      let mut done_file = fs::File::create(&done_path).unwrap();
      done_file
        .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
        .unwrap();
    }

    let chunk_queue = self.load_or_gen_chunk_queue(splits);

    crossbeam_utils::thread::scope(|s| {
      // vapoursynth audio is currently unsupported
      let audio_thread = if self.input.is_video()
        && (!self.resume || !get_done().audio_done.load(atomic::Ordering::SeqCst))
      {
        let input = self.input.as_video_path();
        let temp = self.temp.as_str();
        let audio_params = self.audio_params.as_slice();
        Some(s.spawn(move |_| {
          let audio_output_exists = ffmpeg::encode_audio(input, temp, audio_params);
          get_done().audio_done.store(true, atomic::Ordering::SeqCst);

          let progress_file = Path::new(temp).join("done.json");
          let mut progress_file = File::create(&progress_file).unwrap();
          progress_file
            .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
            .unwrap();

          audio_output_exists
        }))
      } else {
        None
      };

      if self.workers == 0 {
        self.workers = determine_workers(self.encoder) as usize;
      }
      self.workers = cmp::min(self.workers, chunk_queue.len());
      println!(
        "Queue: {} Workers: {} Passes: {}\nParams: {}\n",
        chunk_queue.len(),
        self.workers,
        self.passes,
        self.video_params.join(" ")
      );

      if self.verbosity == Verbosity::Normal {
        init_progress_bar((self.frames - initial_frames) as u64);
      } else if self.verbosity == Verbosity::Verbose {
        init_multi_progress_bar((self.frames - initial_frames) as u64, self.workers);
      }

      let broker = Broker {
        chunk_queue,
        project: self,
        target_quality: if self.target_quality.is_some() {
          Some(TargetQuality::new(self))
        } else {
          None
        },
      };

      let (tx, rx) = mpsc::channel();
      let handle = s.spawn(|_| {
        broker.encoding_loop(tx);
      });

      // Queue::encoding_loop only sends a message if there was an error (meaning a chunk crashed)
      // more than MAX_TRIES. So, we have to explicitly exit the program if that happens.
      while let Ok(()) = rx.recv() {
        std::process::exit(1);
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

      info!("Concatenating with {}", self.concat);

      match self.concat {
        ConcatMethod::Ivf => {
          concat::ivf(
            &Path::new(&self.temp).join("encode"),
            self.output_file.as_ref(),
          )
          .unwrap();
        }
        ConcatMethod::MKVMerge => {
          concat::mkvmerge(self.temp.as_ref(), self.output_file.as_ref()).unwrap();
        }
        ConcatMethod::FFmpeg => {
          concat::ffmpeg(self.temp.as_ref(), self.output_file.as_ref(), self.encoder);
        }
      }

      if self.vmaf {
        vmaf::plot(
          self.output_file.as_ref(),
          &self.input,
          self.vmaf_path.as_deref(),
          self.vmaf_res.as_str(),
          1,
          match &self.vmaf_filter {
            Some(filter) => Some(filter.as_str()),
            None => None,
          },
          self.vmaf_threads.unwrap_or_else(num_cpus::get),
        )
        .unwrap();
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
    })
    .unwrap();
  }
}
