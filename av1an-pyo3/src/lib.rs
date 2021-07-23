use av1an_core::{vapoursynth, SplitMethod};
use av1an_core::{ChunkMethod, Encoder};

use anyhow::anyhow;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use itertools::Itertools;

use av1an_core::progress_bar::{
  finish_multi_progress_bar, init_multi_progress_bar, update_mp_bar, update_mp_msg,
};
use av1an_core::split::extra_splits;
use std::cmp;
use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::fs;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::Write;
use std::iter;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;
use std::{collections::hash_map::DefaultHasher, path::PathBuf};

fn adapt_probing_rate(rate: usize, _frames: usize) -> usize {
  av1an_core::adapt_probing_rate(rate)
}

fn get_keyframes(source: &str) -> anyhow::Result<Vec<usize>> {
  let pt = Path::new(source);
  let kf = av1an_core::ffmpeg::get_keyframes(pt);
  Ok(kf)
}

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

  let cache_file = std::env::current_dir()?.join(temp.join("split").join(format!(
    "cache.{}",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffindex",
      ChunkMethod::LSMASH => "lwi",
      _ => return Err(anyhow!("invalid chunk method")),
    }
  )));

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

fn get_ffmpeg_info() -> String {
  av1an_core::get_ffmpeg_info()
}

fn determine_workers(encoder: Encoder) -> anyhow::Result<u64> {
  Ok(av1an_core::determine_workers(encoder))
}

// same as frame_probe, but you can call it without the python GIL
fn frame_probe(source: &str) -> usize {
  if is_vapoursynth(source) {
    av1an_core::vapoursynth::num_frames(Path::new(source)).unwrap()
  } else {
    // TODO evaluate vapoursynth script in-memory if ffms2 or lsmash exists
    ffmpeg_get_frame_count(source)
  }
}

fn extract_audio(input: &str, temp: &str, audio_params: Vec<String>) {
  let input_path = Path::new(&input);
  let temp_path = Path::new(&temp);
  av1an_core::ffmpeg::extract_audio(input_path, temp_path, &audio_params);
}

fn ffmpeg_get_frame_count(source: &str) -> usize {
  av1an_core::ffmpeg::ffmpeg_get_frame_count(Path::new(source))
}

fn segment(input: &str, temp: &str, segments: Vec<usize>) -> anyhow::Result<()> {
  let input = Path::new(&input);
  let temp = Path::new(&temp);
  av1an_core::split::segment(input, temp, &segments);
  Ok(())
}

fn write_scenes_to_file(
  scenes: Vec<usize>,
  frames: usize,
  scenes_path_string: &str,
) -> anyhow::Result<()> {
  let scene_path = PathBuf::from(scenes_path_string);

  av1an_core::split::write_scenes_to_file(&scenes, frames, &scene_path).unwrap();
  Ok(())
}

fn vmaf_auto_threads(workers: usize) -> usize {
  av1an_core::target_quality::vmaf_auto_threads(workers)
}

fn set_log(file: &str) -> anyhow::Result<()> {
  av1an_core::logger::set_log(file).unwrap();
  Ok(())
}

fn log(msg: &str) -> anyhow::Result<()> {
  av1an_core::logger::log(msg);
  Ok(())
}

fn compose_ffmpeg_pipe(params: Vec<String>) -> anyhow::Result<Vec<String>> {
  let res = av1an_core::compose_ffmpeg_pipe(params);
  Ok(res)
}

fn weighted_search(
  num1: f64,
  vmaf1: f64,
  num2: f64,
  vmaf2: f64,
  target: f64,
) -> anyhow::Result<usize> {
  Ok(av1an_core::target_quality::weighted_search(
    num1, vmaf1, num2, vmaf2, target,
  ))
}

pub fn get_percentile(scores: Vec<f64>, percent: f64) -> anyhow::Result<f64> {
  // pyo3 doesn't seem to support `mut` in function declarations, so this is necessary
  let mut scores = scores;
  Ok(av1an_core::get_percentile(&mut scores, percent))
}

pub fn read_weighted_vmaf(fl: String, percentile: f64) -> anyhow::Result<f64> {
  let file = PathBuf::from(fl);
  let val = av1an_core::read_weighted_vmaf(&file, percentile).unwrap();
  Ok(val)
}

pub fn init_progress_bar(len: u64) -> anyhow::Result<()> {
  av1an_core::progress_bar::init_progress_bar(len).unwrap();
  Ok(())
}

pub fn update_bar(inc: u64) -> anyhow::Result<()> {
  av1an_core::progress_bar::update_bar(inc).unwrap();
  Ok(())
}

pub fn finish_progress_bar() -> anyhow::Result<()> {
  av1an_core::progress_bar::finish_progress_bar().unwrap();
  Ok(())
}

pub fn plot_vmaf_score_file(scores_file_string: String, plot_path_string: String) {
  let scores_file = PathBuf::from(scores_file_string);
  let plot_path = PathBuf::from(plot_path_string);
  av1an_core::vmaf::plot_vmaf_score_file(&scores_file, &plot_path).unwrap()
}

pub fn validate_vmaf(model: &str) -> anyhow::Result<()> {
  av1an_core::vmaf::validate_vmaf(&model).unwrap();
  Ok(())
}

pub fn plot_vmaf(source: &str, output: &str) -> anyhow::Result<()> {
  let input = PathBuf::from(source);
  let out = PathBuf::from(output);
  av1an_core::vmaf::plot_vmaf(&input, &out).unwrap();
  Ok(())
}

pub fn interpolate_target_q(scores: Vec<(f64, u32)>, target: f64) -> anyhow::Result<(f64, f64)> {
  let q = av1an_core::target_quality::interpolate_target_q(scores.clone(), target).unwrap();

  let vmaf = av1an_core::target_quality::interpolate_target_vmaf(scores, q).unwrap();

  Ok((q, vmaf))
}

pub fn interpolate_target_vmaf(scores: Vec<(f64, u32)>, target: f64) -> anyhow::Result<f64> {
  Ok(av1an_core::target_quality::interpolate_target_vmaf(scores, target).unwrap())
}

pub fn log_probes(
  vmaf_cq_scores: Vec<(f64, u32)>,
  frames: u32,
  probing_rate: u32,
  name: String,
  target_q: u32,
  target_vmaf: f64,
  skip: String,
) -> anyhow::Result<()> {
  av1an_core::target_quality::log_probes(
    vmaf_cq_scores,
    frames,
    probing_rate,
    &name,
    target_q,
    target_vmaf,
    &skip,
  );
  Ok(())
}

pub fn av_scenechange_detect(
  input: &str,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  is_vs: bool,
) -> anyhow::Result<Vec<usize>> {
  if verbosity != Verbosity::Quiet {
    println!("Scene detection");
    av1an_core::progress_bar::init_progress_bar(total_frames as u64).unwrap();
  }

  let mut frames = av1an_scene_detection::av_scenechange::scene_detect(
    Path::new(input),
    if verbosity == Verbosity::Quiet {
      None
    } else {
      Some(Box::new(|frames, _keyframes| {
        let _ = av1an_core::progress_bar::set_pos(frames as u64);
      }))
    },
    min_scene_len,
    is_vs,
  )?;

  let _ = av1an_core::progress_bar::finish_progress_bar();

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
  target_quality: Option<TargetQuality>,
}

impl<'a> Queue<'a> {
  fn encoding_loop(self) -> Result<(), ()> {
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
            s.spawn(move |_| {
              while let Ok(mut chunk) = rx.recv() {
                if queue.encode_chunk(&mut chunk, consumer_idx).is_err() {
                  return Err(());
                }
              }
              Ok(())
            })
          })
          .collect();
        for consumer in consumers {
          consumer.join().unwrap().unwrap();
        }
      })
      .unwrap();

      if self.project.verbosity == Verbosity::Normal {
        finish_progress_bar().unwrap();
      } else if self.project.verbosity == Verbosity::Verbose {
        finish_multi_progress_bar().unwrap();
      }
    }

    Ok(())
  }

  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), String> {
    let st_time = Instant::now();

    let _ = log(format!("Enc: {}, {} fr", chunk.index, chunk.frames).as_str());

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
      for _try in 1..=MAX_TRIES {
        let res = self
          .project
          .create_pipes(chunk.clone(), current_pass, worker_id);
        if let Err(e) = res {
          eprintln!("{}", e);
          let _ = log(&e);
          if _try == MAX_TRIES {
            eprintln!("Encoder crashed {} times, shutting down thread.", MAX_TRIES);
            return Err(e);
          }
        } else {
          break;
        }
      }
    }

    let encoded_frames = self.frame_check_output(chunk, chunk.frames);

    if encoded_frames == chunk.frames {
      let progress_file = Path::new(&self.project.temp).join("done.json");
      let done_json = fs::read_to_string(&progress_file).unwrap();
      let mut done_json: DoneJson = serde_json::from_str(&done_json).unwrap();
      done_json.done.insert(chunk.name(), encoded_frames);

      let mut progress_file = File::create(&progress_file).unwrap();
      progress_file
        .write_all(serde_json::to_string(&done_json).unwrap().as_bytes())
        .unwrap();

      let enc_time = st_time.elapsed();

      let _ = log(
        format!(
          "Done: {} Fr: {}/{}",
          chunk.index, encoded_frames, chunk.frames
        )
        .as_str(),
      );
      let _ = log(
        format!(
          "Fps: {:.2} Time: {:?}",
          encoded_frames as f64 / enc_time.as_secs_f64(),
          enc_time
        )
        .as_str(),
      );
    }

    Ok(())
  }

  fn frame_check_output(&self, chunk: &Chunk, expected_frames: usize) -> usize {
    let actual_frames = frame_probe(&chunk.output_path());

    if actual_frames != expected_frames {
      let msg = format!(
        "Chunk #{}: {}/{} fr",
        chunk.index, actual_frames, expected_frames
      );
      let _ = log(&msg);
      println!(":: {}", msg);
    }

    actual_frames
  }
}

struct TargetQuality {
  vmaf_res: String,
  vmaf_filter: String,
  n_threads: usize,
  model: String,
  probing_rate: usize,
  probes: u32,
  target: f32,
  min_q: u32,
  max_q: u32,
  encoder: Encoder,
  ffmpeg_pipe: Vec<String>,
  temp: String,
  workers: usize,
  video_params: Vec<String>,
  probe_slow: bool,
}

impl TargetQuality {
  fn new(project: &Project) -> Self {
    Self {
      vmaf_res: project
        .vmaf_res
        .clone()
        .unwrap_or_else(|| String::with_capacity(0)),
      vmaf_filter: project
        .vmaf_filter
        .clone()
        .unwrap_or_else(|| String::with_capacity(0)),
      n_threads: project.n_threads.unwrap_or(0) as usize,
      model: project
        .vmaf_path
        .clone()
        .unwrap_or_else(|| String::with_capacity(0)),
      probes: project.probes,
      target: project.target_quality.unwrap(),
      min_q: project.min_q.unwrap(),
      max_q: project.max_q.unwrap(),
      encoder: project.encoder,
      ffmpeg_pipe: project.ffmpeg_pipe.clone(),
      temp: project.temp.clone(),
      workers: project.workers,
      video_params: project.video_params.clone(),
      probe_slow: project.probe_slow,
      probing_rate: adapt_probing_rate(project.probing_rate as usize, 20),
    }
  }

  fn per_shot_target_quality(&self, chunk: &Chunk) -> u32 {
    let mut vmaf_cq = vec![];
    let frames = chunk.frames;

    let mut q_list = vec![];

    // Make middle probe
    let middle_point = (self.min_q + self.max_q) / 2;
    q_list.push(middle_point);
    let last_q = middle_point;

    let mut score = read_weighted_vmaf(self.vmaf_probe(chunk, last_q.to_string()), 0.25).unwrap();
    vmaf_cq.push((score, last_q));

    // Initialize search boundary
    let mut vmaf_lower = score;
    let mut vmaf_upper = score;
    let mut vmaf_cq_lower = last_q;
    let mut vmaf_cq_upper = last_q;

    // Branch
    let next_q = if score < self.target as f64 {
      self.min_q
    } else {
      self.max_q
    };

    q_list.push(next_q);

    // Edge case check
    score = read_weighted_vmaf(self.vmaf_probe(chunk, next_q.to_string()), 0.25).unwrap();
    vmaf_cq.push((score, next_q));

    if (next_q == self.min_q && score < self.target as f64)
      || (next_q == self.max_q && score > self.target as f64)
    {
      av1an_core::target_quality::log_probes(
        vmaf_cq,
        frames as u32,
        self.probing_rate as u32,
        &chunk.name(),
        next_q,
        score,
        if score < self.target as f64 {
          "low"
        } else {
          "high"
        },
      );
      return next_q;
    }

    // Set boundary
    if score < self.target as f64 {
      vmaf_lower = score;
      vmaf_cq_lower = next_q;
    } else {
      vmaf_upper = score;
      vmaf_cq_upper = next_q;
    }

    // VMAF search
    for _ in 0..self.probes - 2 {
      let new_point = weighted_search(
        vmaf_cq_lower as f64,
        vmaf_lower,
        vmaf_cq_upper as f64,
        vmaf_upper,
        self.target as f64,
      )
      .unwrap();

      if vmaf_cq
        .iter()
        .map(|(_, x)| *x)
        .any(|x| x == new_point as u32)
      {
        break;
      }

      q_list.push(new_point as u32);
      score = read_weighted_vmaf(self.vmaf_probe(chunk, new_point.to_string()), 0.25).unwrap();
      vmaf_cq.push((score, new_point as u32));

      // Update boundary
      if score < self.target as f64 {
        vmaf_lower = score;
        vmaf_cq_lower = new_point as u32;
      } else {
        vmaf_upper = score;
        vmaf_cq_upper = new_point as u32;
      }
    }

    let (q, q_vmaf) = interpolate_target_q(vmaf_cq.clone(), self.target as f64).unwrap();
    log_probes(
      vmaf_cq,
      frames as u32,
      self.probing_rate as u32,
      chunk.name(),
      q as u32,
      q_vmaf,
      "".into(),
    )
    .unwrap();

    q as u32
  }

  fn vmaf_probe(&self, chunk: &Chunk, q: String) -> String {
    let n_threads = if self.n_threads == 0 {
      vmaf_auto_threads(self.workers)
    } else {
      self.n_threads
    };

    let cmd = self.encoder.probe_cmd(
      self.temp.clone(),
      &chunk.name(),
      q.clone(),
      self.ffmpeg_pipe.clone(),
      &self.probing_rate.to_string(),
      n_threads.to_string(),
      self.video_params.clone(),
      self.probe_slow,
    );

    let future = async {
      let mut ffmpeg_gen_pipe = tokio::process::Command::new(chunk.ffmpeg_gen_cmd[0].clone())
        .args(&chunk.ffmpeg_gen_cmd[1..])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

      let ffmpeg_gen_pipe_stdout: Stdio =
        ffmpeg_gen_pipe.stdout.take().unwrap().try_into().unwrap();

      let mut ffmpeg_pipe = tokio::process::Command::new(cmd.0[0].clone())
        .args(&cmd.0[1..])
        .stdin(ffmpeg_gen_pipe_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

      let ffmpeg_pipe_stdout: Stdio = ffmpeg_pipe.stdout.take().unwrap().try_into().unwrap();

      let mut pipe = tokio::process::Command::new(cmd.1[0].clone())
        .args(&cmd.1[1..])
        .stdin(ffmpeg_pipe_stdout)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

      process_pipe(
        &mut pipe,
        chunk.index,
        &mut [&mut ffmpeg_gen_pipe, &mut ffmpeg_pipe],
      )
      .await
      .unwrap();
    };

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    rt.block_on(future);

    let probe_name =
      Path::new(&chunk.temp)
        .join("split")
        .join(format!("v_{}{}.ivf", q, chunk.name()));
    let fl_path = Path::new(&chunk.temp)
      .join("split")
      .join(format!("{}.json", chunk.name()));

    let fl_path = fl_path.to_str().unwrap().to_owned();

    run_vmaf_on_chunk(
      probe_name.to_str().unwrap().to_owned(),
      chunk.ffmpeg_gen_cmd.clone(),
      fl_path.clone(),
      self.model.clone(),
      self.vmaf_res.clone(),
      self.probing_rate,
      self.vmaf_filter.clone(),
      self.n_threads,
    );

    fl_path
  }

  fn per_shot_target_quality_routine(&self, chunk: &mut Chunk) {
    chunk.per_shot_target_quality_cq = Some(self.per_shot_target_quality(chunk));
  }
}

async fn process_pipe(
  pipe: &mut tokio::process::Child,
  chunk_index: usize,
  utility: &mut [&mut tokio::process::Child],
) -> Result<(), String> {
  let mut encoder_history: VecDeque<String> = VecDeque::with_capacity(20);

  let mut reader = BufReader::new(pipe.stdout.take().unwrap()).lines();

  while let Some(line) = reader.next_line().await.unwrap() {
    encoder_history.push_back(line);
  }

  for util in utility {
    // On Windows, killing the process can fail with a permission denied error, so we don't
    // unwrap the result to prevent the program from crashing if killing the child process fails.
    let _ = util.kill().await;
  }

  let returncode = pipe.wait().await.unwrap();
  if let Some(code) = returncode.code() {
    if code != 0 && code != -2 {
      return Err(format!(
        "Encoder encountered an error: {}
Chunk: {}
{:?}",
        code, chunk_index, encoder_history
      ));
    }
  }

  Ok(())
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
struct Chunk {
  temp: String,
  index: usize,
  ffmpeg_gen_cmd: Vec<String>,
  output_ext: String,
  size: usize,
  frames: usize,
  per_shot_target_quality_cq: Option<u32>,
}

impl Chunk {
  fn name(&self) -> String {
    format!("{:05}", self.index)
  }

  fn output(&self) -> String {
    self.output_path()
  }

  fn output_path(&self) -> String {
    Path::new(&self.temp)
      .join("encode")
      .join(format!("{}.{}", self.name(), self.output_ext))
      .to_str()
      .unwrap()
      .to_owned()
  }
}

fn save_chunk_queue(temp: &str, chunk_queue: Vec<Chunk>) {
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
  pub mkvmerge: bool,
  pub output_ivf: bool,
  pub webm: bool,

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
  pub logging: String,
  pub resume: bool,
  pub keep: bool,

  pub vmaf: bool,
  pub vmaf_path: Option<String>,
  pub vmaf_res: Option<String>,

  pub concat: String,

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
    .filter_map(|param| {
      if valid_options.contains(param) {
        None
      } else {
        Some(param)
      }
    })
    .map(|s| s.to_string())
    .collect()
}

fn suggest_fix(wrong_arg: &str, arg_dictionary: &HashSet<String>) -> Option<String> {
  arg_dictionary
    .iter()
    .map(|arg| (arg, strsim::jaro_winkler(arg, wrong_arg)))
    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Less))
    .map(|(s, _)| (*s).to_owned())
}

fn read_chunk_queue(temp: &str) -> Vec<Chunk> {
  let contents = fs::read_to_string(Path::new(temp).join("chunks.json")).unwrap();

  serde_json::from_str(&contents).unwrap()
}

#[derive(Debug, Deserialize, Serialize)]
struct DoneJson {
  frames: usize,
  done: HashMap<String, usize>,
}

impl Project {
  fn read_queue_files(source_path: &Path) -> Vec<PathBuf> {
    let mut queue_files = fs::read_dir(&source_path)
      .unwrap()
      .map(|res| res.map(|e| e.path()))
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    queue_files.retain(|file| file.is_file());
    queue_files
      .retain(|file| matches!(Path::new(&file).extension().map(|x| x == "mkv"), Some(true)));
    av1an_core::concat::sort_files_by_filename(&mut queue_files);

    queue_files
  }
}

impl Project {
  fn create_pipes(&self, c: Chunk, current_pass: u8, worker_id: usize) -> Result<(), String> {
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

    let mut encoder_history: VecDeque<String> = VecDeque::with_capacity(20);

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    let returncode = rt.block_on(async {
      let mut ffmpeg_gen_pipe = tokio::process::Command::new(&c.ffmpeg_gen_cmd[0])
        .args(&c.ffmpeg_gen_cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

      let ffmpeg_gen_pipe_stdout: Stdio =
        ffmpeg_gen_pipe.stdout.take().unwrap().try_into().unwrap();

      let ffmpeg_pipe = compose_ffmpeg_pipe(self.ffmpeg_pipe.clone()).unwrap();
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

      while let Ok(read) = reader.read_until(b'\r', &mut buf).await {
        if read == 0 {
          break;
        }

        let line = std::str::from_utf8(&buf);

        if let Ok(line) = line {
          if self.verbosity == Verbosity::Verbose && !line.contains('\n') {
            update_mp_msg(worker_id, line.to_string()).unwrap();
          }
          if let Some(new) = self.encoder.match_line(line) {
            encoder_history.push_back(line.to_owned());

            if new > frame {
              if self.verbosity == Verbosity::Normal {
                update_bar((new - frame) as u64).unwrap();
              } else if self.verbosity == Verbosity::Verbose {
                update_mp_bar((new - frame) as u64).unwrap();
              }
              frame = new;
            }
          }
        }

        buf.clear();
      }

      let returncode = pipe.wait_with_output().await.unwrap();

      let _ = ffmpeg_gen_pipe.kill().await;
      let _ = ffmpeg_pipe.kill().await;

      returncode.status
    });

    if let Some(code) = returncode.code() {
      // -2 is Ctrl+C for aom
      if code != 0 && code != -2 {
        return Err(format!(
          "Encoder encountered an error: {}
Chunk: {}
{}",
          returncode,
          c.index,
          encoder_history.iter().join("\n")
        ));
      }
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
      ffmpeg_get_frame_count(&self.input)
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
          .map(|s| s.to_owned())
      })
      .collect::<HashSet<String>>()
  }

  // TODO remove all of these extra allocations
  fn validate_inputs(&self) {
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
      .map(|s| s.to_owned())
      .collect();

    let valid_params = self.valid_encoder_params();

    let mut invalid_param_found = false;
    for wrong_param in invalid_params(video_params.as_slice(), &valid_params) {
      if let Some(suggestion) = suggest_fix(&wrong_param, &valid_params) {
        println!(
          "'{}' isn't a valid parameter for {}. Did you mean '{}'?",
          wrong_param, self.encoder, suggestion,
        );
        invalid_param_found = true;
      }
    }

    if invalid_param_found {
      panic!("To continue anyway, run Av1an with --force");
    }
  }

  pub fn startup_check(&mut self) -> anyhow::Result<()> {
    if matches!(
      self.encoder,
      Encoder::rav1e | Encoder::aom | Encoder::svt_av1 | Encoder::vpx
    ) && self.output_ivf
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

    let _ = log(&get_ffmpeg_info());

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

    let encoder_bin = self.encoder.encoder_bin();
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

    self.validate_inputs();

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
      ChunkMethod::Segment => self.create_video_queue_segment(splits),
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

    let scene_file = Path::new(&self.temp).join("scenes.json");

    let mut scenes = if self.resume {
      av1an_core::split::read_scenes_from_file(scene_file.as_path())
        .unwrap()
        .0
    } else {
      self.calc_split_locations()
    };
    let _ = log(&format!("SC: Found {} scenes", scenes.len() + 1));
    if let Some(split_len) = self.extra_splits_len {
      let _ = log(&format!(
        "SC: Applying extra splits every {} frames",
        split_len
      ));
      scenes = extra_splits(scenes, self.frames, split_len);
      let _ = log(&format!("SC: Now at {} scenes", scenes.len() + 1));
    }

    self.write_scenes_to_file(scenes.clone(), scene_file.as_path().to_str().unwrap());

    scenes
  }

  fn write_scenes_to_file(&self, scenes: Vec<usize>, path: &str) {
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
      ..Default::default()
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
      ..Default::default()
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

  fn create_video_queue_segment(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    let _ = log("Split video");
    segment(&self.input, &self.temp, splits).unwrap();
    let _ = log("Split done");

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
    let keyframes = get_keyframes(&self.input).unwrap();

    let mut splits = vec![0];
    splits.extend(split_locations);
    splits.push(self.get_frames());

    let segments_set: HashSet<(usize, usize)> = splits
      .iter()
      .zip(splits.iter().skip(1))
      .map(|(start, end)| (*start, *end))
      .collect();

    let to_split: Vec<usize> = keyframes
      .iter()
      .filter(|kf| splits.contains(kf))
      .copied()
      .collect();

    let _ = log("Segmenting video");
    segment(
      &self.input,
      &self.temp,
      to_split[1..].iter().copied().collect(),
    )
    .unwrap();
    let _ = log("Segment done");

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
      ..Default::default()
    }
  }

  fn load_or_gen_chunk_queue(&mut self, splits: Vec<usize>) -> Vec<Chunk> {
    if self.resume {
      let mut chunks = read_chunk_queue(&self.temp);

      let done_path = Path::new(&self.temp).join("done.json");

      let done_contents = fs::read_to_string(&done_path).unwrap();
      let done: DoneJson = serde_json::from_str(&done_contents).unwrap();

      // only keep the chunks that are not done
      chunks.retain(|chunk| !done.done.contains_key(&chunk.name()));

      chunks
    } else {
      let chunks = self.create_encoding_queue(splits);
      save_chunk_queue(&self.temp, chunks.clone());
      chunks
    }
  }

  pub fn encode_file(&mut self) {
    let _ = log(format!("File hash: {}", hash_path(&self.input)).as_str());

    let done_path = Path::new(&self.temp).join("done.json");

    self.resume = self.resume && done_path.exists();

    if !self.resume && Path::new(&self.temp).is_dir() {
      fs::remove_dir_all(&self.temp).unwrap();
    }

    let _ = match fs::create_dir_all(Path::new(&self.temp).join("split")) {
      Ok(_) => {}
      Err(e) => match e.kind() {
        io::ErrorKind::AlreadyExists => {}
        _ => panic!("{}", e),
      },
    };
    let _ = match fs::create_dir_all(Path::new(&self.temp).join("encode")) {
      Ok(_) => {}
      Err(e) => match e.kind() {
        io::ErrorKind::AlreadyExists => {}
        _ => panic!("{}", e),
      },
    };

    set_log(&self.logging).unwrap();

    let splits = self.split_routine();

    let chunk_queue = self.load_or_gen_chunk_queue(splits);

    let done_path = Path::new(&self.temp).join("done.json");

    let mut initial_frames: usize = 0;

    if self.resume && done_path.exists() {
      let _ = log("Resuming...");

      let done: DoneJson = serde_json::from_str(&fs::read_to_string(&done_path).unwrap()).unwrap();
      initial_frames = done.done.iter().map(|(_, frames)| frames).sum();
      let _ = log(format!("Resumed with {} encoded clips done", done.done.len()).as_str());
    } else {
      let total = self.get_frames();
      let mut done_file = fs::File::create(&done_path).unwrap();
      done_file
        .write_all(
          serde_json::to_string(&DoneJson {
            frames: total,
            done: HashMap::new(),
          })
          .unwrap()
          .as_bytes(),
        )
        .unwrap();
    }

    if !self.resume {
      extract_audio(&self.input, &self.temp, self.audio_params.clone());
    }

    if self.workers == 0 {
      self.workers = determine_workers(self.encoder).unwrap() as usize;
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
      init_progress_bar((self.frames - initial_frames) as u64).unwrap();
    } else if self.verbosity == Verbosity::Verbose {
      init_multi_progress_bar((self.frames - initial_frames) as u64, self.workers).unwrap();
    }

    // hack to avoid borrow checker errors
    let concat = self.concat.clone();
    let temp = self.temp.clone();
    let input = self.input.clone();
    let output_file = self.output_file.clone();
    let encoder = self.encoder;
    let vmaf = self.vmaf;
    let keep = self.keep;

    let queue = Queue {
      chunk_queue,
      project: &self,
      target_quality: if self.target_quality.is_some() {
        Some(TargetQuality::new(&self))
      } else {
        None
      },
    };

    queue.encoding_loop().unwrap();

    let _ = log("Concatenating");

    // TODO refactor into Concatenate trait
    match concat.as_str() {
      "ivf" => {
        av1an_core::concat::concat_ivf(&Path::new(&temp).join("encode"), Path::new(&output_file))
          .unwrap();
      }
      "mkvmerge" => {
        av1an_core::concat::concatenate_mkvmerge(temp.clone(), output_file.clone()).unwrap()
      }
      "ffmpeg" => {
        av1an_core::ffmpeg::concatenate_ffmpeg(temp.clone(), output_file.clone(), encoder);
      }
      _ => unreachable!(),
    }

    if vmaf {
      plot_vmaf(&input, &output_file).unwrap();
    }

    if !keep {
      fs::remove_dir_all(temp).unwrap();
    }
  }
}

fn run_vmaf_on_chunk(
  encoded: String,
  pipe_cmd: Vec<String>,
  stat_file: String,
  model: String,
  res: String,
  sample_rate: usize,
  vmaf_filter: String,
  threads: usize,
) {
  let encoded = PathBuf::from(encoded);
  let stat_file = PathBuf::from(stat_file);

  av1an_core::vmaf::run_vmaf_on_chunk(
    &encoded,
    &pipe_cmd,
    &stat_file,
    &model,
    &res,
    sample_rate,
    &vmaf_filter,
    threads,
  )
  .unwrap()
}
