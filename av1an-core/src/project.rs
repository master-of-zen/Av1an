use crate::{
  broker::Broker,
  chunk::Chunk,
  concat::ConcatMethod,
  create_dir, determine_workers, ffmpeg,
  ffmpeg::compose_ffmpeg_pipe,
  finish_multi_progress_bar, get_done, hash_path, init_done, into_vec, invalid_params,
  progress_bar::{
    finish_progress_bar, init_multi_progress_bar, init_progress_bar, update_bar, update_mp_bar,
    update_mp_msg,
  },
  read_chunk_queue, regex, save_chunk_queue,
  scene_detect::av_scenechange_detect,
  split::{extra_splits, segment, write_scenes_to_file},
  suggest_fix, vapoursynth,
  vapoursynth::{create_vs_file, is_vapoursynth},
  vmaf::plot_vmaf,
  ChunkMethod, DashMap, DoneJson, Encoder, SplitMethod, TargetQuality, Verbosity,
};
use anyhow::{bail, ensure};
use crossbeam_utils;
use flexi_logger::{Duplicate, FileSpec, Logger};
use path_abs::PathAbs;
use std::{
  cmp,
  cmp::Reverse,
  collections::{HashSet, VecDeque},
  convert::TryInto,
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

pub struct Project {
  pub frames: usize,
  pub is_vs: bool,

  pub input: String,
  pub temp: String,
  pub output_file: String,

  pub chunk_method: ChunkMethod,
  pub scenes: Option<String>,
  pub split_method: SplitMethod,
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
  pub vmaf_res: Option<String>,

  pub concat: ConcatMethod,

  pub target_quality: Option<f32>,
  pub target_quality_method: Option<String>,
  pub probes: u32,
  pub probe_slow: bool,
  pub min_q: Option<u32>,
  pub max_q: Option<u32>,

  pub probing_rate: u32,
  pub n_threads: Option<u32>,
  pub vmaf_filter: Option<String>,
}

impl Project {
  /// Initialize logging routines and create temporary directories
  pub fn initialize(&mut self) -> anyhow::Result<()> {
    info!("File hash: {}", hash_path(&self.input));

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
    queue_files.retain(|file| file.is_file());
    queue_files.retain(|file| matches!(file.extension().map(|ext| ext == "mkv"), Some(true)));
    crate::concat::sort_files_by_filename(&mut queue_files);

    queue_files
  }

  pub fn create_pipes(
    &self,
    c: &Chunk,
    current_pass: u8,
    worker_id: usize,
  ) -> Result<(), (ExitStatus, VecDeque<String>)> {
    let fpf_file = Path::new(&c.temp)
      .join("split")
      .join(format!("{}_fpf", c.name()));

    let mut enc_cmd = if self.passes == 1 {
      self
        .encoder
        .compose_1_1_pass(self.video_params.clone(), c.output())
    } else if current_pass == 1 {
      self.encoder.compose_1_2_pass(
        self.video_params.clone(),
        &fpf_file.to_str().unwrap().to_owned(),
      )
    } else {
      self.encoder.compose_2_2_pass(
        self.video_params.clone(),
        &fpf_file.to_str().unwrap().to_owned(),
        c.output(),
      )
    };

    if let Some(per_shot_target_quality_cq) = c.per_shot_target_quality_cq {
      enc_cmd = self
        .encoder
        .man_command(enc_cmd, per_shot_target_quality_cq as usize);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    let (exit_status, output) = rt.block_on(async {
      let mut ffmpeg_gen_pipe = tokio::process::Command::new(&c.ffmpeg_gen_cmd[0])
        .args(&c.ffmpeg_gen_cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

      let ffmpeg_gen_pipe_stdout: Stdio =
        ffmpeg_gen_pipe.stdout.take().unwrap().try_into().unwrap();

      let ffmpeg_pipe = compose_ffmpeg_pipe(self.ffmpeg_pipe.clone());
      let mut ffmpeg_pipe = tokio::process::Command::new(&ffmpeg_pipe[0])
        .args(&ffmpeg_pipe[1..])
        .stdin(ffmpeg_gen_pipe_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

      let ffmpeg_pipe_stdout: Stdio = ffmpeg_pipe.stdout.take().unwrap().try_into().unwrap();

      let mut pipe = tokio::process::Command::new(&enc_cmd[0])
        .args(&enc_cmd[1..])
        .stdin(ffmpeg_pipe_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

      let mut frame = 0;

      let mut reader = BufReader::new(pipe.stderr.take().unwrap());

      let mut buf = vec![];
      let mut output = VecDeque::with_capacity(20);

      while let Ok(read) = reader.read_until(b'\r', &mut buf).await {
        if read == 0 {
          break;
        }

        let line = std::str::from_utf8(&buf);

        if let Ok(line) = line {
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
          output.push_back(line.to_string());
        }

        buf.clear();
      }

      let status = pipe.wait_with_output().await.unwrap().status;

      drop(ffmpeg_gen_pipe.kill().await);
      drop(ffmpeg_pipe.kill().await);

      (status, output)
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

    self.frames = if self.is_vs {
      vapoursynth::num_frames(Path::new(&self.input)).unwrap()
    } else if matches!(self.chunk_method, ChunkMethod::FFMS2 | ChunkMethod::LSMASH) {
      let vs = if self.is_vs {
        self.input.clone()
      } else {
        create_vs_file(&self.temp, &self.input, self.chunk_method).unwrap()
      };
      let fr = vapoursynth::num_frames(Path::new(&vs)).unwrap();
      assert!(fr != 0, "vapoursynth reported 0 frames");
      fr
    } else {
      ffmpeg::get_frame_count(&self.input)
    };

    self.frames
  }

  /// returns a list of valid parameters
  #[must_use]
  fn valid_encoder_params(&self) -> HashSet<String> {
    let help = self.encoder.help_command();

    let help_text = String::from_utf8(
      Command::new(&help[0])
        .args(&help[1..])
        .output()
        .unwrap()
        .stdout,
    )
    .unwrap();

    regex!(r"\s+(-\w+|(?:--\w+(?:-\w+)*))")
      .find_iter(&help_text)
      .filter_map(|m| {
        m.as_str()
          .split_ascii_whitespace()
          .next()
          .map(ToString::to_string)
      })
      .collect::<HashSet<String>>()
  }

  // TODO remove all of these extra allocations
  fn validate_input(&self) {
    if self.force {
      return;
    }

    let video_params: Vec<String> = self
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
      .map(ToString::to_string)
      .collect();

    let valid_params = self.valid_encoder_params();

    let invalid_params = invalid_params(video_params.as_slice(), &valid_params);

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
      Path::new(&self.input).exists(),
      "Input file {:?} does not exist!",
      self.input
    );

    self.is_vs = is_vapoursynth(&self.input);

    if which::which("ffmpeg").is_err() {
      bail!("No FFmpeg");
    }

    if let Some(ref vmaf_path) = self.vmaf_path {
      ensure!(Path::new(vmaf_path).exists());
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
    let settings_valid = which::which(&encoder_bin).is_ok();

    if !settings_valid {
      bail!(
        "Encoder {} not found. Is it installed in the system path?",
        encoder_bin
      );
    }

    if self.video_params.is_empty() {
      self.video_params = self.encoder.get_default_arguments();
    }

    self.validate_input();
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

    chunks.sort_unstable_by_key(|chunk| Reverse(chunk.size));

    chunks
  }

  fn calc_split_locations(&self) -> Vec<usize> {
    match self.split_method {
      SplitMethod::AvScenechange => av_scenechange_detect(
        &self.input,
        self.frames,
        self.min_scene_len,
        self.verbosity,
        self.is_vs,
        false,
      )
      .unwrap(),
      SplitMethod::AvScenechangeFast => av_scenechange_detect(
        &self.input,
        self.frames,
        self.min_scene_len,
        self.verbosity,
        self.is_vs,
        true,
      )
      .unwrap(),
      SplitMethod::None => Vec::with_capacity(0),
    }
  }

  // If we are not resuming, then do scene detection. Otherwise: get scenes from
  // scenes.json and return that.
  fn split_routine(&mut self) -> Vec<usize> {
    // TODO make self.frames impossible to misuse
    let _ = self.get_frames();

    let scene_file = self.scenes.as_ref().map_or_else(
      || Path::new(&self.temp).join("scenes.json"),
      |path| Path::new(&path).to_path_buf(),
    );

    let mut scenes = if (self.scenes.is_some() && scene_file.exists()) || self.resume {
      crate::split::read_scenes_from_file(scene_file.as_path())
        .unwrap()
        .0
    } else {
      self.calc_split_locations()
    };
    info!("SC: Found {} scenes", scenes.len() + 1);
    if let Some(split_len) = self.extra_splits_len {
      info!("SC: Applying extra splits every {} frames", split_len);
      scenes = extra_splits(scenes, self.frames, split_len);
      info!("SC: Now at {} scenes", scenes.len() + 1);
    }

    self.write_scenes_to_file(&scenes, scene_file.as_path().to_str().unwrap());

    scenes
  }

  fn write_scenes_to_file(&self, scenes: &[usize], path: &str) {
    write_scenes_to_file(scenes, self.frames, path).unwrap();
  }

  fn create_select_chunk(
    &self,
    index: usize,
    src_path: &str,
    frame_start: usize,
    mut frame_end: usize,
  ) -> Chunk {
    assert!(
      frame_end > frame_start,
      "Can't make a chunk with <= 0 frames!"
    );

    let frames = frame_end - frame_start;
    frame_end -= 1;

    let ffmpeg_gen_cmd: Vec<String> = into_vec![
      "ffmpeg",
      "-y",
      "-hide_banner",
      "-loglevel",
      "error",
      "-i",
      src_path.to_string(),
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

    let output_ext = self.encoder.output_extension().to_owned();
    // use the number of frames to prioritize which chunks encode first, since we don't have file size
    let size = frames;

    Chunk {
      temp: self.temp.clone(),
      index,
      ffmpeg_gen_cmd,
      output_ext,
      size,
      frames,
      ..Chunk::default()
    }
  }

  fn create_vs_chunk(
    &self,
    index: usize,
    vs_script: String,
    frame_start: usize,
    mut frame_end: usize,
  ) -> Chunk {
    assert!(
      frame_end > frame_start,
      "Can't make a chunk with <= 0 frames!"
    );

    let frames = frame_end - frame_start;
    // the frame end boundary is actually a frame that should be included in the next chunk
    frame_end -= 1;

    let vspipe_cmd_gen: Vec<String> = vec![
      "vspipe".into(),
      vs_script,
      "-y".into(),
      "-".into(),
      "-s".into(),
      frame_start.to_string(),
      "-e".into(),
      frame_end.to_string(),
    ];

    let output_ext = self.encoder.output_extension();

    Chunk {
      temp: self.temp.clone(),
      index,
      ffmpeg_gen_cmd: vspipe_cmd_gen,
      output_ext: output_ext.to_owned(),
      // use the number of frames to prioritize which chunks encode first, since we don't have file size
      size: frames,
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

    let vs_script = if self.is_vs {
      self.input.clone()
    } else {
      create_vs_file(&self.temp, &self.input, self.chunk_method).unwrap()
    };

    let chunk_queue: Vec<Chunk> = chunk_boundaries
      .iter()
      .enumerate()
      .map(|(index, (frame_start, frame_end))| {
        self.create_vs_chunk(index, vs_script.clone(), *frame_start, *frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_select(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    let last_frame = self.get_frames();

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
        self.create_select_chunk(index, &self.input, *frame_start, *frame_end)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_segment(&mut self, splits: &[usize]) -> Vec<Chunk> {
    info!("Split video");
    segment(&self.input, &self.temp, splits);
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
    let keyframes = ffmpeg::get_keyframes(&self.input);

    let mut splits = vec![0];
    splits.extend(split_locations);
    splits.push(self.get_frames());

    let segments_set: Vec<(usize, usize)> = splits
      .iter()
      .zip(splits.iter().skip(1))
      .map(|(start, end)| (*start, *end))
      .collect();

    let to_split: Vec<usize> = keyframes
      .iter()
      .filter(|kf| splits.contains(kf))
      .copied()
      .collect();

    info!("Segmenting video");
    segment(&self.input, &self.temp, &to_split[1..]);
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
        self.create_select_chunk(index, &file.as_path().to_string_lossy(), *start, *end)
      })
      .collect();

    chunk_queue
  }

  fn create_chunk_from_segment(&mut self, index: usize, file: &str) -> Chunk {
    let ffmpeg_gen_cmd = vec![
      "ffmpeg".into(),
      "-y".into(),
      "-hide_banner".into(),
      "-loglevel".into(),
      "error".into(),
      "-i".into(),
      file.to_owned(),
      "-strict".into(),
      "-1".into(),
      "-pix_fmt".into(),
      self.pix_format.clone(),
      "-f".into(),
      "yuv4mpegpipe".into(),
      "-".into(),
    ];

    let output_ext = self.encoder.output_extension().to_owned();
    let file_size = File::open(file).unwrap().metadata().unwrap().len();

    Chunk {
      temp: self.temp.clone(),
      frames: self.get_frames(),
      ffmpeg_gen_cmd,
      output_ext,
      index,
      size: file_size as usize,
      ..Chunk::default()
    }
  }

  fn load_or_gen_chunk_queue(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    if self.resume {
      let mut chunks = read_chunk_queue(&self.temp);

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
      let audio_thread = if !self.resume || !get_done().audio_done.load(atomic::Ordering::SeqCst) {
        // Required outside of closure due to borrow checker errors
        let input = self.input.as_str();
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

      // hack to avoid borrow checker errors
      let concat = self.concat;
      let temp = &self.temp;
      let input = &self.input;
      let output_file = &self.output_file;
      let encoder = self.encoder;
      let vmaf = self.vmaf;
      let model = self.vmaf_path.as_ref();
      let keep = self.keep;

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

      info!("Concatenating");

      match concat {
        ConcatMethod::Ivf => {
          crate::concat::ivf(&Path::new(&temp).join("encode"), Path::new(&output_file)).unwrap();
        }
        ConcatMethod::MKVMerge => {
          crate::concat::mkvmerge(temp.clone(), output_file.clone()).unwrap();
        }
        ConcatMethod::FFmpeg => {
          crate::concat::ffmpeg(temp.clone(), output_file.clone(), encoder);
        }
      }

      if vmaf {
        plot_vmaf(&input, &output_file, model).unwrap();
      }

      if !Path::new(&output_file).exists() {
        warn!(
          "Concatenating failed for unknown reasons! Temp folder will not be deleted: {}",
          temp
        );
      } else if !keep {
        if let Err(e) = fs::remove_dir_all(temp) {
          warn!("Failed to delete temp directory: {}", e);
        }
      }
    })
    .unwrap();
  }
}
