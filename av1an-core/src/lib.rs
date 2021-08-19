#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_wrap)]

#[macro_use]
extern crate log;

use crate::target_quality::TargetQuality;
use chunk::Chunk;
use dashmap::DashMap;
use path_abs::{PathAbs, PathInfo};
use serde::{Deserialize, Serialize};
use std::cmp;
use std::cmp::Ordering;
use std::fmt::{Display, Error};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::Sender;
use std::sync::{atomic, mpsc};
use sysinfo::SystemExt;

use itertools::Itertools;

use crate::encoder::Encoder;
use regex::Regex;

use flexi_logger::{Duplicate, FileSpec, Logger};

pub mod chunk;
pub mod concat;
pub mod encoder;
pub mod ffmpeg;
pub mod file_validation;
pub mod progress_bar;
pub mod split;
pub mod target_quality;
pub mod vapoursynth;
pub mod vmaf;
#[macro_export]
macro_rules! into_vec {
  ($($x:expr),* $(,)?) => {
    vec![
      $(
        $x.into(),
      )*
    ]
  };
}

pub fn compose_ffmpeg_pipe(params: Vec<String>) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params);

  p
}

pub fn list_index_of_regex(params: &[String], regex_str: &str) -> Option<usize> {
  let re = Regex::new(regex_str).unwrap();

  assert!(
    !params.is_empty(),
    "List index of regex got empty list of params"
  );

  for (i, cmd) in params.iter().enumerate() {
    if re.is_match(cmd) {
      return Some(i);
    }
  }
  panic!("No match found for params: {:#?}", params)
}

#[derive(
  PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr,
)]
pub enum ConcatMethod {
  #[strum(serialize = "mkvmerge")]
  MKVMerge,
  #[strum(serialize = "ffmpeg")]
  FFmpeg,
  #[strum(serialize = "ivf")]
  Ivf,
}

impl Display for ConcatMethod {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(<&'static str>::from(self))
  }
}

#[derive(Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum SplitMethod {
  #[strum(serialize = "av-scenechange")]
  AvScenechange,
  #[strum(serialize = "av-scenechange-fast")]
  AvScenechangeFast,
  #[strum(serialize = "none")]
  None,
}

#[derive(
  PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr,
)]
pub enum ChunkMethod {
  #[strum(serialize = "select")]
  Select,
  #[strum(serialize = "hybrid")]
  Hybrid,
  #[strum(serialize = "segment")]
  Segment,
  #[strum(serialize = "ffms2")]
  FFMS2,
  #[strum(serialize = "lsmash")]
  LSMASH,
}

/// Check for `FFmpeg`
pub fn get_ffmpeg_info() -> String {
  let mut cmd = Command::new("ffmpeg");
  cmd.stderr(Stdio::piped());
  String::from_utf8(cmd.output().unwrap().stderr).unwrap()
}

/// Determine the optimal number of workers for an encoder
#[must_use]
pub fn determine_workers(encoder: Encoder) -> u64 {
  // TODO look for lighter weight solution? sys-info maybe?
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = num_cpus::get() as u64;
  // available_memory returns kb, convert to gb
  let ram_gb = system.available_memory() / 10_u64.pow(6);

  std::cmp::max(
    match encoder {
      Encoder::aom | Encoder::rav1e | Encoder::vpx => std::cmp::min(
        (cpu as f64 / 3.0).round() as u64,
        (ram_gb as f64 / 1.5).round() as u64,
      ),
      Encoder::svt_av1 | Encoder::x264 | Encoder::x265 => std::cmp::min(cpu, ram_gb) / 8,
    },
    1,
  )
}

/// Calculates percentile from vector of scores
pub fn get_percentile(scores: &mut [f64], percentile: f64) -> f64 {
  scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

  let k = (scores.len() - 1) as f64 * percentile;
  let f = k.floor();
  let c = k.ceil();

  if f as u64 == c as u64 {
    return scores[k as usize];
  }

  let d0 = scores[f as usize] as f64 * (c - k);
  let d1 = scores[f as usize] as f64 * (k - f);

  d0 + d1
}

#[derive(Deserialize, Debug)]
struct Foo {
  vmaf: f64,
}

#[derive(Deserialize, Debug)]
struct Bar {
  metrics: Foo,
}

#[derive(Deserialize, Debug)]
struct Baz {
  frames: Vec<Bar>,
}

pub fn read_file_to_string(file: impl AsRef<Path>) -> Result<String, Error> {
  Ok(fs::read_to_string(&file).unwrap_or_else(|_| panic!("Can't open file {:?}", file.as_ref())))
}

pub fn read_vmaf_file(file: impl AsRef<Path>) -> Result<Vec<f64>, serde_json::Error> {
  let json_str = read_file_to_string(file).unwrap();
  let bazs = serde_json::from_str::<Baz>(&json_str)?;
  let v = bazs
    .frames
    .into_iter()
    .map(|x| x.metrics.vmaf)
    .collect::<Vec<_>>();

  Ok(v)
}

pub fn read_weighted_vmaf(
  file: impl AsRef<Path>,
  percentile: f64,
) -> Result<f64, serde_json::Error> {
  let mut scores = read_vmaf_file(file).unwrap();

  Ok(get_percentile(&mut scores, percentile))
}

use anyhow::anyhow;
use once_cell::sync::{Lazy, OnceCell};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::progress_bar::finish_progress_bar;
use crate::progress_bar::init_progress_bar;
use crate::progress_bar::update_bar;
use crate::progress_bar::{
  finish_multi_progress_bar, init_multi_progress_bar, update_mp_bar, update_mp_msg,
};
use crate::split::extra_splits;
use crate::split::segment;
use crate::split::write_scenes_to_file;
use crate::vmaf::plot_vmaf;

use std::cmp::Reverse;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::Write;
use std::iter;
use std::string::ToString;
use std::time::Instant;

pub fn hash_path(path: &str) -> String {
  let mut s = DefaultHasher::new();
  path.hash(&mut s);
  format!("{:x}", s.finish())[..7].to_string()
}

fn create_vs_file(temp: &str, source: &str, chunk_method: ChunkMethod) -> anyhow::Result<String> {
  // only for python code, remove if being called by rust
  let temp = Path::new(temp);
  let source = Path::new(source).canonicalize()?;

  let load_script_path = temp.join("split").join("loadscript.vpy");

  if load_script_path.exists() {
    return Ok(load_script_path.to_string_lossy().to_string());
  }
  let mut load_script = File::create(&load_script_path)?;

  let cache_file = PathAbs::new(temp.join("split").join(format!(
    "cache.{}",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffindex",
      ChunkMethod::LSMASH => "lwi",
      _ => return Err(anyhow!("invalid chunk method")),
    }
  )))
  .unwrap();

  load_script.write_all(
    // TODO should probably check if the syntax for rust strings and escaping utf and stuff like that is the same as in python
    format!(
      "from vapoursynth import core\n\
core.{}({:?}, cachefile={:?}).set_output()",
      match chunk_method {
        ChunkMethod::FFMS2 => "ffms2.Source",
        ChunkMethod::LSMASH => "lsmas.LWLibavSource",
        _ => unreachable!(),
      },
      source,
      cache_file
    )
    .as_bytes(),
  )?;

  // TODO use vapoursynth crate instead
  Command::new("vspipe")
    .arg("-i")
    .arg(&load_script_path)
    .args(&["-i", "-"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?
    .wait()?;

  Ok(load_script_path.to_string_lossy().to_string())
}

fn frame_probe(source: &str) -> usize {
  if is_vapoursynth(source) {
    crate::vapoursynth::num_frames(Path::new(source)).unwrap()
  } else {
    // TODO evaluate vapoursynth script in-memory if ffms2 or lsmash exists
    ffmpeg::get_frame_count(source)
  }
}

pub fn av_scenechange_detect(
  input: &str,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  is_vs: bool,
  fast_analysis: bool,
) -> anyhow::Result<Vec<usize>> {
  if verbosity != Verbosity::Quiet {
    println!("Scene detection");
    progress_bar::init_progress_bar(total_frames as u64);
  }

  let mut frames = av1an_scene_detection::av_scenechange::scene_detect(
    Path::new(input),
    if verbosity == Verbosity::Quiet {
      None
    } else {
      Some(Box::new(|frames, _keyframes| {
        progress_bar::set_pos(frames as u64);
      }))
    },
    min_scene_len,
    is_vs,
    fast_analysis,
  )?;

  progress_bar::finish_progress_bar();

  if frames[0] == 0 {
    // TODO refactor the chunk creation to not require this
    // Currently, this is required for compatibility with create_video_queue_vs
    frames.remove(0);
  }

  Ok(frames)
}

pub fn is_vapoursynth(s: &str) -> bool {
  [".vpy", ".py"].iter().any(|ext| s.ends_with(ext))
}

struct Queue<'a> {
  chunk_queue: Vec<Chunk>,
  project: &'a Project,
  target_quality: Option<TargetQuality<'a>>,
}

impl<'a> Queue<'a> {
  fn encoding_loop(self, tx: Sender<()>) {
    if !self.chunk_queue.is_empty() {
      let (sender, receiver) = crossbeam_channel::bounded(self.chunk_queue.len());

      let workers = self.project.workers;

      for chunk in &self.chunk_queue {
        sender.send(chunk.clone()).unwrap();
      }
      drop(sender);

      crossbeam_utils::thread::scope(|s| {
        let consumers: Vec<_> = (0..workers)
          .map(|i| (receiver.clone(), &self, i))
          .map(|(rx, queue, consumer_idx)| {
            let tx = tx.clone();
            s.spawn(move |_| {
              while let Ok(mut chunk) = rx.recv() {
                if queue.encode_chunk(&mut chunk, consumer_idx).is_err() {
                  tx.send(()).unwrap();
                  return Err(());
                }
              }
              Ok(())
            })
          })
          .collect();
        for consumer in consumers {
          let _ = consumer.join().unwrap();
        }
      })
      .unwrap();

      if self.project.verbosity == Verbosity::Normal {
        finish_progress_bar();
      } else if self.project.verbosity == Verbosity::Verbose {
        finish_multi_progress_bar();
      }
    }
  }

  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), VecDeque<String>> {
    let st_time = Instant::now();

    info!("Enc: {}, {} fr", chunk.index, chunk.frames);

    // Target Quality mode
    if self.project.target_quality.is_some() {
      if let Some(ref method) = self.project.target_quality_method {
        if method == "per_shot" {
          if let Some(ref tq) = self.target_quality {
            tq.per_shot_target_quality_routine(chunk);
          }
        }
      }
    }

    // Run all passes for this chunk
    const MAX_TRIES: usize = 3;
    for current_pass in 1..=self.project.passes {
      for r#try in 1..=MAX_TRIES {
        let res = self.project.create_pipes(chunk, current_pass, worker_id);
        if let Err((exit_status, output)) = res {
          warn!(
            "Encoder failed (on chunk {}) with {}:\n{}",
            chunk.index,
            exit_status,
            textwrap::indent(&output.iter().join("\n"), /* 8 spaces */ "        ")
          );
          if r#try == MAX_TRIES {
            error!(
              "Encoder crashed (on chunk {}) {} times, terminating thread",
              chunk.index, MAX_TRIES
            );
            return Err(output);
          }
        } else {
          break;
        }
      }
    }

    let encoded_frames = Self::frame_check_output(chunk, chunk.frames);

    if encoded_frames == chunk.frames {
      let progress_file = Path::new(&self.project.temp).join("done.json");
      get_done().done.insert(chunk.name(), encoded_frames);

      let mut progress_file = File::create(&progress_file).unwrap();
      progress_file
        .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
        .unwrap();

      let enc_time = st_time.elapsed();

      info!(
        "Done: {} Fr: {}/{}",
        chunk.index, encoded_frames, chunk.frames
      );
      info!(
        "Fps: {:.2} Time: {:?}",
        encoded_frames as f64 / enc_time.as_secs_f64(),
        enc_time
      );
    }

    Ok(())
  }

  fn frame_check_output(chunk: &Chunk, expected_frames: usize) -> usize {
    let actual_frames = frame_probe(&chunk.output_path());

    if actual_frames != expected_frames {
      info!(
        "Chunk #{}: {}/{} fr",
        chunk.index, actual_frames, expected_frames
      );
    }

    actual_frames
  }
}

pub async fn process_pipe(pipe: tokio::process::Child, chunk_index: usize) -> Result<(), String> {
  let status = pipe.wait_with_output().await.unwrap();

  if !status.status.success() {
    return Err(format!(
      "Encoder encountered an error on chunk {}: {:?}",
      chunk_index, status
    ));
  }

  Ok(())
}

fn save_chunk_queue(temp: &str, chunk_queue: &[Chunk]) {
  let mut file = File::create(Path::new(temp).join("chunks.json")).unwrap();

  file
    .write_all(serde_json::to_string(&chunk_queue).unwrap().as_bytes())
    .unwrap();
}

#[derive(Clone, Copy, PartialEq)]
pub enum Verbosity {
  Verbose,
  Normal,
  Quiet,
}

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

static HELP_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+(-\w+|(?:--\w+(?:-\w+)*))").unwrap());

// TODO refactor to make types generic
fn invalid_params(params: &[String], valid_options: &HashSet<String>) -> Vec<String> {
  params
    .iter()
    .filter(|param| !valid_options.contains(*param))
    .map(ToString::to_string)
    .collect()
}

fn suggest_fix(wrong_arg: &str, arg_dictionary: &HashSet<String>) -> Option<String> {
  // Minimum threshold to consider a suggestion similar enough that it could be a typo
  const MIN_THRESHOLD: f64 = 0.75;

  arg_dictionary
    .iter()
    .map(|arg| (arg, strsim::jaro_winkler(arg, wrong_arg)))
    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Less))
    .and_then(|(suggestion, score)| {
      if score > MIN_THRESHOLD {
        Some((*suggestion).clone())
      } else {
        None
      }
    })
}

fn read_chunk_queue(temp: &str) -> Vec<Chunk> {
  let contents = fs::read_to_string(Path::new(temp).join("chunks.json")).unwrap();

  serde_json::from_str(&contents).unwrap()
}

/// Concurrent data structure for keeping track of the finished
/// chunks in an encode
#[derive(Debug, Deserialize, Serialize)]
struct DoneJson {
  frames: AtomicUsize,
  done: DashMap<String, usize>,
  audio_done: AtomicBool,
}

static DONE_JSON: OnceCell<DoneJson> = OnceCell::new();

// once_cell::sync::Lazy cannot be used here due to Lazy<T> not implementing
// Serialize or Deserialize, we need to get a reference directly to the global
// data
fn get_done() -> &'static DoneJson {
  DONE_JSON.get().unwrap()
}

fn init_done(done: DoneJson) -> &'static DoneJson {
  DONE_JSON.get_or_init(|| done)
}

/// Attempts to create the directory if it does not exist, logging and returning
/// and error if creating the directory failed.
macro_rules! create_dir {
  ($loc:expr) => {
    match fs::create_dir(&$loc) {
      Ok(_) => Ok(()),
      Err(e) => match e.kind() {
        io::ErrorKind::AlreadyExists => Ok(()),
        _ => {
          error!("Error while creating directory {:?}: {}", &$loc, e);
          Err(e)
        }
      },
    }
  };
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

  fn create_pipes(
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
}

impl Project {
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
      if fr > 0 {
        fr
      } else {
        panic!("vapoursynth reported 0 frames")
      }
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

    HELP_REGEX
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
        eprintln!("\tDid you mean '{}'?", suggestion)
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
      panic!(".ivf only supports VP8, VP9, and AV1");
    }

    assert!(
      Path::new(&self.input).exists(),
      "Input file {:?} does not exist!",
      self.input
    );

    self.is_vs = is_vapoursynth(&self.input);

    if which::which("ffmpeg").is_err() {
      panic!("No FFmpeg");
    }

    if let Some(ref vmaf_path) = self.vmaf_path {
      assert!(Path::new(vmaf_path).exists());
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
      panic!(
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

    let mut scenes = if self.scenes.is_some() && scene_file.exists() {
      crate::split::read_scenes_from_file(scene_file.as_path())
        .unwrap()
        .0
    } else if self.resume {
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

    let ffmpeg_gen_cmd: Vec<String> = vec![
      "ffmpeg".into(),
      "-y".into(),
      "-hide_banner".into(),
      "-loglevel".into(),
      "error".into(),
      "-i".into(),
      src_path.to_string(),
      "-vf".into(),
      format!(
        "select=between(n\\,{}\\,{}),setpts=PTS-STARTPTS",
        frame_start, frame_end
      ),
      "-pix_fmt".into(),
      self.pix_format.clone(),
      "-strict".into(),
      "-1".into(),
      "-f".into(),
      "yuv4mpegpipe".into(),
      "-".into(),
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
        let input = &self.input;
        let temp = &self.temp;
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

      let queue = Queue {
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
        queue.encoding_loop(tx);
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
          crate::concat::mkvmerge(temp.clone(), output_file.clone()).unwrap()
        }
        ConcatMethod::FFmpeg => {
          crate::concat::ffmpeg(temp.clone(), output_file.clone(), encoder);
        }
      }

      if vmaf {
        plot_vmaf(&input, &output_file, model).unwrap();
      }

      if !keep {
        if let Err(e) = fs::remove_dir_all(temp) {
          warn!("Failed to delete temp directory: {}", e);
        }
      }
    })
    .unwrap();
  }
}
