use once_cell::sync::Lazy;

use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use av1an_core::vapoursynth;
use av1an_core::{ChunkMethod, Encoder};
use regex::Regex;

use serde::{Deserialize, Serialize};

use std::cmp;
use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::Write;
use std::iter;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::usize;
use std::{collections::hash_map::DefaultHasher, path::PathBuf};

use dict_derive::FromPyObject;

#[pyfunction]
fn adapt_probing_rate(rate: usize, _frames: usize) -> usize {
  av1an_core::adapt_probing_rate(rate)
}

#[pyfunction]
fn get_keyframes(source: &str) -> PyResult<Vec<usize>> {
  let pt = Path::new(source);
  let kf = av1an_core::ffmpeg::get_keyframes(pt);
  Ok(kf)
}

#[pyfunction]
fn hash_path(path: &str) -> String {
  let mut s = DefaultHasher::new();
  path.hash(&mut s);
  format!("{:x}", s.finish())[..7].to_string()
}

#[pyfunction]
fn construct_target_quality_command(
  encoder: &str,
  threads: &str,
  q: &str,
) -> PyResult<Vec<String>> {
  let encoder = av1an_encoder_constructor::Encoder::from_str(encoder).map_err(|_| {
    pyo3::exceptions::PyTypeError::new_err(format!("Unknown or unsupported encoder '{}'", encoder))
  })?;

  Ok(
    encoder
      .construct_target_quality_command(threads.parse().unwrap(), q.to_string())
      .iter()
      .map(|s| s.to_string())
      .collect(),
  )
}

/// Creates vs pipe file
#[pyfunction]
fn create_vs_file(temp: &str, source: &str, chunk_method: &str) -> PyResult<String> {
  // only for python code, remove if being called by rust
  let temp = Path::new(temp);
  let source = Path::new(source).canonicalize()?;
  let chunk_method = ChunkMethod::from_str(chunk_method)
    // TODO implement this in the FromStr implementation itself
    .map_err(|_| pyo3::exceptions::PyTypeError::new_err("Invalid chunk method"))?;
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
      _ =>
        return Err(pyo3::exceptions::PyTypeError::new_err(
          "Can only use vapoursynth chunk methods"
        )),
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

#[pyfunction]
fn get_ffmpeg_info() -> String {
  av1an_core::get_ffmpeg_info()
}

#[pyfunction]
fn get_frame_types(file: String) -> Vec<String> {
  let input_file = Path::new(&file);

  av1an_core::ffmpeg::get_frame_types(input_file)
}

#[pyfunction]
fn determine_workers(encoder: &str) -> PyResult<u64> {
  Ok(av1an_core::determine_workers(
    Encoder::from_str(encoder).map_err(|_| {
      pyo3::exceptions::PyTypeError::new_err(format!(
        "Unknown or unsupported encoder '{}'",
        encoder
      ))
    })?,
  ))
}

#[pyfunction]
fn frame_probe_vspipe(source: &str, py: Python) -> PyResult<usize> {
  let frames = py.allow_threads(|| av1an_core::vapoursynth::num_frames(Path::new(source)));
  frames.map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("{}", e)))
}

#[pyfunction]
fn frame_probe(source: &str, py: Python) -> PyResult<usize> {
  if is_vapoursynth(source) {
    frame_probe_vspipe(source, py)
  } else {
    // TODO evaluate vapoursynth script in-memory if ffms2 or lsmash exists
    Ok(ffmpeg_get_frame_count(source))
  }
}

#[pyfunction]
fn extract_audio(input: &str, temp: &str, audio_params: Vec<String>) {
  let input_path = Path::new(&input);
  let temp_path = Path::new(&temp);
  av1an_core::ffmpeg::extract_audio(input_path, temp_path, &audio_params);
}

#[pyfunction]
fn ffmpeg_get_frame_count(source: &str) -> usize {
  av1an_core::ffmpeg::ffmpeg_get_frame_count(Path::new(source))
}

#[pyfunction]
fn concatenate_ivf(input: &str, output: &str) -> PyResult<()> {
  av1an_core::concat::concat_ivf(Path::new(input), Path::new(output))
    .map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("{}", e)))
}

#[pyfunction]
fn concatenate_ffmpeg(temp: String, output: String, encoder: String) -> PyResult<()> {
  let encoder = Encoder::from_str(&encoder).map_err(|_| {
    pyo3::exceptions::PyTypeError::new_err(format!("Unknown or unsupported encoder '{}'", encoder))
  })?;

  let temp_path = Path::new(&temp);
  let output_path = Path::new(&output);

  av1an_core::ffmpeg::concatenate_ffmpeg(temp_path, output_path, encoder);
  Ok(())
}

#[pyfunction]
fn extra_splits(split_locations: Vec<usize>, total_frames: usize, split_size: usize) -> Vec<usize> {
  av1an_core::split::extra_splits(split_locations, total_frames, split_size)
}

#[pyfunction]
fn segment(input: &str, temp: &str, segments: Vec<usize>) -> PyResult<()> {
  let input = Path::new(&input);
  let temp = Path::new(&temp);
  av1an_core::split::segment(input, temp, &segments);
  Ok(())
}

#[pyfunction]
fn process_inputs(input: Vec<String>) -> Vec<String> {
  let path_bufs: Vec<PathBuf> = input
    .into_iter()
    .map(|x| PathBuf::from_str(x.as_str()).unwrap())
    .collect();

  let processed = av1an_core::file_validation::process_inputs(&path_bufs);

  let out: Vec<String> = processed
    .iter()
    .map(|x| x.as_path().to_str().unwrap().to_string())
    .collect();

  out
}

#[pyfunction]
fn write_scenes_to_file(
  scenes: Vec<usize>,
  frames: usize,
  scenes_path_string: &str,
) -> PyResult<()> {
  let scene_path = PathBuf::from(scenes_path_string);

  av1an_core::split::write_scenes_to_file(&scenes, frames, &scene_path).unwrap();
  Ok(())
}

#[pyfunction]
fn read_scenes_from_file(scenes_path_string: &str) -> (Vec<usize>, usize) {
  let scene_path = PathBuf::from(scenes_path_string);

  av1an_core::split::read_scenes_from_file(&scene_path).unwrap()
}

#[pyfunction]
fn parse_args() -> String {
  av1an_cli::parse_args()
}

#[pyfunction]
fn default_args() -> String {
  av1an_cli::default_args()
}

#[pyfunction]
fn vmaf_auto_threads(workers: usize) -> usize {
  av1an_core::target_quality::vmaf_auto_threads(workers)
}

#[pyfunction]
fn set_log(file: &str) -> PyResult<()> {
  av1an_core::logger::set_log(file).unwrap();
  Ok(())
}

#[pyfunction]
fn log(msg: &str) -> PyResult<()> {
  av1an_core::logger::log(msg);
  Ok(())
}

#[pyfunction]
fn get_default_pass(encoder: &str) -> PyResult<usize> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .get_default_pass(),
  )
}

#[pyfunction]
fn get_default_cq_range(encoder: &str) -> PyResult<(usize, usize)> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .get_default_cq_range(),
  )
}

#[pyfunction]
fn get_default_arguments(encoder: &str) -> PyResult<Vec<String>> {
  let encoder = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  let default_arguments = encoder.get_default_arguments();

  Ok(
    default_arguments
      .iter()
      .map(|&s| s.to_string())
      .collect::<Vec<String>>(),
  )
}

#[pyfunction]
fn help_command(encoder: &str) -> PyResult<Vec<String>> {
  let encoder = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  let help_command = encoder.help_command();

  Ok(
    help_command
      .iter()
      .map(|&s| s.to_string())
      .collect::<Vec<String>>(),
  )
}

#[pyfunction]
fn encoder_bin(encoder: &str) -> PyResult<String> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .encoder_bin()
      .into(),
  )
}

#[pyfunction]
fn output_extension(encoder: &str) -> PyResult<String> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .output_extension()
      .into(),
  )
}

#[pyfunction]
fn compose_ffmpeg_pipe(params: Vec<String>) -> PyResult<Vec<String>> {
  let res = av1an_encoder_constructor::compose_ffmpeg_pipe(params);
  Ok(res)
}

#[pyfunction]
fn compose_1_1_pass(encoder: String, params: Vec<String>, output: String) -> PyResult<Vec<String>> {
  let enc = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  Ok(enc.compose_1_1_pass(params, output))
}

#[pyfunction]
fn compose_1_2_pass(encoder: String, params: Vec<String>, fpf: String) -> PyResult<Vec<String>> {
  let enc = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  Ok(enc.compose_1_2_pass(params, fpf))
}

#[pyfunction]
fn compose_2_2_pass(
  encoder: String,
  params: Vec<String>,
  fpf: String,
  output: String,
) -> PyResult<Vec<String>> {
  let enc = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  Ok(enc.compose_2_2_pass(params, fpf, output))
}

#[pyfunction]
fn find_aom_keyframes(fl: String, min_kf_length: usize) -> Vec<usize> {
  let file = PathBuf::from(fl);
  av1an_scene_detection::aom_kf::find_aom_keyframes(file, min_kf_length)
}

#[pyfunction]
fn man_command(encoder: String, params: Vec<String>, q: usize) -> Vec<String> {
  let enc = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();

  enc.man_command(params, q)
}

#[pyfunction]
fn match_line(encoder: &str, line: &str) -> PyResult<usize> {
  let enc = av1an_encoder_constructor::Encoder::from_str(encoder).unwrap();

  Ok(enc.match_line(line).unwrap())
}

#[pyfunction]
fn weighted_search(num1: f64, vmaf1: f64, num2: f64, vmaf2: f64, target: f64) -> PyResult<usize> {
  Ok(av1an_core::target_quality::weighted_search(
    num1, vmaf1, num2, vmaf2, target,
  ))
}

#[pyfunction]
fn probe_cmd(
  encoder: String,
  temp: String,
  name: String,
  q: String,
  ffmpeg_pipe: Vec<String>,
  probing_rate: String,
  n_threads: String,
  video_params: Vec<String>,
  probe_slow: bool,
) -> PyResult<(Vec<String>, Vec<String>)> {
  let encoder = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  Ok(encoder.probe_cmd(
    temp,
    name,
    q,
    ffmpeg_pipe,
    probing_rate,
    n_threads,
    video_params,
    probe_slow,
  ))
}

#[pyfunction]
pub fn get_percentile(scores: Vec<f64>, percent: f64) -> PyResult<f64> {
  // pyo3 doesn't seem to support `mut` in function declarations, so this is necessary
  let mut scores = scores;
  Ok(av1an_core::get_percentile(&mut scores, percent))
}

#[pyfunction]
pub fn read_weighted_vmaf(fl: String, percentile: f64) -> PyResult<f64> {
  let file = PathBuf::from(fl);
  let val = av1an_core::read_weighted_vmaf(&file, percentile).unwrap();
  Ok(val)
}

#[pyfunction]
pub fn init_progress_bar(len: u64) -> PyResult<()> {
  av1an_core::progress_bar::init_progress_bar(len).unwrap();
  Ok(())
}

#[pyfunction]
pub fn update_bar(inc: u64) -> PyResult<()> {
  av1an_core::progress_bar::update_bar(inc).unwrap();
  Ok(())
}

#[pyfunction]
pub fn finish_progress_bar() -> PyResult<()> {
  av1an_core::progress_bar::finish_progress_bar().unwrap();
  Ok(())
}

#[pyfunction]
pub fn plot_vmaf_score_file(scores_file_string: String, plot_path_string: String) {
  let scores_file = PathBuf::from(scores_file_string);
  let plot_path = PathBuf::from(plot_path_string);
  av1an_core::vmaf::plot_vmaf_score_file(&scores_file, &plot_path).unwrap()
}

#[pyfunction]
pub fn validate_vmaf(model: &str) -> PyResult<()> {
  av1an_core::vmaf::validate_vmaf(&model).unwrap();
  Ok(())
}

#[pyfunction]
pub fn plot_vmaf(source: &str, output: &str) -> PyResult<()> {
  let input = PathBuf::from(source);
  let out = PathBuf::from(output);
  av1an_core::vmaf::plot_vmaf(&input, &out).unwrap();
  Ok(())
}

#[pyfunction]
pub fn interpolate_target_q(scores: Vec<(f64, u32)>, target: f64) -> PyResult<(f64, f64)> {
  let q = av1an_core::target_quality::interpolate_target_q(scores.clone(), target).unwrap();

  let vmaf = av1an_core::target_quality::interpolate_target_vmaf(scores, q).unwrap();

  Ok((q, vmaf))
}

#[pyfunction]
pub fn interpolate_target_vmaf(scores: Vec<(f64, u32)>, target: f64) -> PyResult<f64> {
  Ok(av1an_core::target_quality::interpolate_target_vmaf(scores, target).unwrap())
}

#[pyfunction]
pub fn log_probes(
  vmaf_cq_scores: Vec<(f64, u32)>,
  frames: u32,
  probing_rate: u32,
  name: String,
  target_q: u32,
  target_vmaf: f64,
  skip: String,
) -> PyResult<()> {
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

#[pyfunction]
pub fn av_scenechange_detect(
  input: &str,
  total_frames: usize,
  min_scene_len: usize,
  quiet: bool,
  is_vs: bool,
) -> PyResult<Vec<usize>> {
  if !quiet {
    println!("Scene detection");
    av1an_core::progress_bar::init_progress_bar(total_frames as u64).unwrap();
  }

  let mut frames = av1an_scene_detection::av_scenechange::scene_detect(
    Path::new(input),
    if quiet {
      None
    } else {
      Some(Box::new(|frames, _keyframes| {
        let _ = av1an_core::progress_bar::set_pos(frames as u64);
      }))
    },
    min_scene_len,
    is_vs,
  )
  .map_err(|e| {
    pyo3::exceptions::PyChildProcessError::new_err(format!(
      "Error in av-scenechange detection: {}",
      e
    ))
  });

  let _ = av1an_core::progress_bar::finish_progress_bar();

  if let Ok(ref mut frames) = frames {
    if frames[0] == 0 {
      // TODO refactor the chunk creation to not require this
      // Currently, this is required for compatibility with create_video_queue_vs
      frames.remove(0);
    }
  }

  frames
}

#[pyfunction]
fn is_vapoursynth(s: &str) -> bool {
  [".vpy", ".py"].iter().any(|ext| s.ends_with(ext))
}

#[pyclass(dict)]
#[derive(Default, Debug, Serialize, Deserialize, Clone)]
struct Chunk {
  #[pyo3(get, set)]
  temp: String,
  #[pyo3(get, set)]
  index: usize,
  #[pyo3(get, set)]
  ffmpeg_gen_cmd: Vec<String>,
  #[pyo3(get, set)]
  output_ext: String,
  #[pyo3(get, set)]
  size: usize,
  #[pyo3(get, set)]
  frames: usize,
  #[pyo3(get, set)]
  per_shot_target_quality_cq: Option<u32>,
}

#[pymethods]
impl Chunk {
  #[new]
  fn new(
    temp: String,
    index: usize,
    ffmpeg_gen_cmd: Vec<String>,
    output_ext: String,
    size: usize,
    frames: usize,
  ) -> Self {
    Chunk {
      temp,
      index,
      ffmpeg_gen_cmd,
      output_ext,
      size,
      frames,
      ..Default::default()
    }
  }

  #[getter]
  fn name(&self) -> String {
    format!("{:05}", self.index)
  }

  #[getter]
  fn output(&self) -> String {
    self.output_path()
  }

  #[getter]
  fn output_path(&self) -> String {
    Path::new(&self.temp)
      .join("encode")
      .join(format!("{}.{}", self.name(), self.output_ext))
      .to_string_lossy()
      .to_string()
  }
}

#[pyfunction]
fn save_chunk_queue(temp: &str, chunk_queue: Vec<Chunk>) {
  let mut file = fs::File::create(Path::new(temp).join("chunks.json")).unwrap();

  file
    .write_all(serde_json::to_string(&chunk_queue).unwrap().as_bytes())
    .unwrap();
}

#[pyclass(dict)]
#[derive(Default, FromPyObject)]
struct Project {
  #[pyo3(get, set)]
  frames: usize,
  #[pyo3(get, set)]
  is_vs: bool,

  #[pyo3(get, set)]
  input: String,
  #[pyo3(get, set)]
  temp: String,
  #[pyo3(get, set)]
  output_file: String,
  #[pyo3(get, set)]
  mkvmerge: bool,
  #[pyo3(get, set)]
  output_ivf: bool,
  #[pyo3(get, set)]
  webm: bool,

  #[pyo3(get, set)]
  chunk_method: Option<String>,
  #[pyo3(get, set)]
  scenes: Option<String>,
  #[pyo3(get, set)]
  split_method: String,
  #[pyo3(get, set)]
  extra_split: usize,
  #[pyo3(get, set)]
  min_scene_len: usize,

  #[pyo3(get, set)]
  passes: u8,
  #[pyo3(get, set)]
  video_params: Vec<String>,
  #[pyo3(get, set)]
  encoder: String,
  #[pyo3(get, set)]
  workers: usize,

  // FFmpeg params
  #[pyo3(get, set)]
  ffmpeg_pipe: Vec<String>,
  #[pyo3(get, set)]
  ffmpeg: Vec<String>,
  #[pyo3(get, set)]
  audio_params: Vec<String>,
  #[pyo3(get, set)]
  pix_format: String,

  #[pyo3(get, set)]
  quiet: bool,
  #[pyo3(get, set)]
  logging: String,
  #[pyo3(get, set)]
  resume: bool,
  #[pyo3(get, set)]
  keep: bool,
  #[pyo3(get, set)]
  force: bool,

  #[pyo3(get, set)]
  vmaf: bool,
  #[pyo3(get, set)]
  vmaf_path: Option<String>,
  #[pyo3(get, set)]
  vmaf_res: Option<String>,

  #[pyo3(get, set)]
  concat: String,

  #[pyo3(get, set)]
  target_quality: Option<f32>,
  #[pyo3(get, set)]
  target_quality_method: Option<String>,
  #[pyo3(get, set)]
  probes: u32,
  #[pyo3(get, set)]
  probe_slow: bool,
  #[pyo3(get, set)]
  min_q: Option<u32>,
  #[pyo3(get, set)]
  max_q: Option<u32>,
  #[pyo3(get, set)]
  vmaf_plots: Option<bool>,
  #[pyo3(get, set)]
  probing_rate: u32,
  #[pyo3(get, set)]
  n_threads: Option<u32>,
  #[pyo3(get, set)]
  vmaf_filter: Option<String>,
}

static HELP_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+(-\w+|(?:--\w+(?:-\w+)*))").unwrap());

// TODO refactor to make types generic
fn invalid_params<'a>(params: &[String], valid_options: &HashSet<String>) -> Vec<String> {
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

#[pyfunction]
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
    queue_files.retain(
      |file| match Path::new(&file).extension().map(|x| x == "mkv") {
        Some(true) => true,
        _ => false,
      },
    );
    av1an_core::concat::sort_files_by_filename(&mut queue_files);

    queue_files
  }
}

#[pymethods]
impl Project {
  #[new]
  fn new(project: Project) -> Self {
    project
  }

  fn get_frames(&mut self, py: Python) -> usize {
    if self.frames != 0 {
      return self.frames;
    }

    self.frames = if self.is_vs {
      py.allow_threads(|| vapoursynth::num_frames(Path::new(&self.input)).unwrap())
    } else {
      if ["vs_ffms2", "vs_lsmash"].contains(&self.chunk_method.as_ref().unwrap().as_str()) {
        let vs = if self.is_vs {
          self.input.clone()
        } else {
          create_vs_file(
            &self.temp,
            &self.input,
            &self.chunk_method.as_ref().unwrap(),
          )
          .unwrap()
        };
        let fr = py.allow_threads(|| vapoursynth::num_frames(Path::new(&vs)).unwrap());
        if fr > 0 {
          fr
        } else {
          panic!("vapoursynth reported 0 frames")
        }
      } else {
        ffmpeg_get_frame_count(&self.input)
      }
    };

    self.frames
  }

  fn select_best_chunking_method(&mut self, py: Python) {
    // You have to wrap vapoursynth calls with `allow_threads`, otherwise
    // a fatal Python interpreter error occurs relating to the GIL state.
    let chunk_method = py.allow_threads(|| av1an_core::vapoursynth::select_chunk_method().unwrap());

    self.chunk_method = Some(chunk_method.to_string());
  }

  /// returns a list of valid parameters
  #[must_use]
  fn valid_encoder_params(&self) -> HashSet<String> {
    let help = help_command(&self.encoder).unwrap();

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

  fn startup_check(&mut self, py: Python) -> PyResult<()> {
    if ["rav1e", "aom", "svt_av1", "vpx"].contains(&self.encoder.as_str()) && self.output_ivf {
      panic!(".ivf only supports VP8, VP9, and AV1");
    }

    if self.chunk_method.is_none() {
      self.select_best_chunking_method(py);
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

    if let Ok((min, max)) = get_default_cq_range(&self.encoder) {
      match self.min_q {
        None => {
          self.min_q = Some(min as u32);
        }
        Some(min_q) => assert!(min_q > 1),
      }
      if let None = self.max_q {
        self.max_q = Some(max as u32);
      }
    }

    let encoder_bin = encoder_bin(&self.encoder).unwrap();
    let settings_valid = which::which(&encoder_bin).is_ok();

    if !settings_valid {
      panic!(
        "Encoder {} not found. Is it installed in the system path?",
        encoder_bin
      );
    }

    if self.video_params.is_empty() {
      self.video_params = get_default_arguments(&self.encoder).unwrap();
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

  fn create_encoding_queue(&mut self, splits: Vec<usize>, py: Python) -> Vec<Chunk> {
    let mut chunks = match self.chunk_method.as_ref().unwrap().as_str() {
      "vs_ffms2" | "vs_lsmash" => self.create_video_queue_vs(splits, py),
      "hybrid" => self.create_video_queue_hybrid(splits, py),
      "select" => self.create_video_queue_select(splits, py),
      "segment" => self.create_video_queue_segment(splits, py),
      _ => unreachable!(),
    };

    chunks.sort_unstable_by_key(|chunk| Reverse(chunk.size));

    chunks
  }

  fn calc_split_locations(&self) -> Vec<usize> {
    match self.split_method.as_str() {
      "av-scenechange" => av_scenechange_detect(
        &self.input,
        self.frames,
        self.min_scene_len,
        self.quiet,
        self.is_vs,
      )
      .unwrap(),
      "none" => Vec::with_capacity(0),
      _ => unreachable!(),
    }
  }

  // If we are not resuming, then do scene detection. Otherwise: get scenes from
  // scenes.json and return that.
  fn split_routine(&mut self, py: Python) -> Vec<usize> {
    // TODO make self.frames impossible to misuse
    let _ = self.get_frames(py);

    let scene_file = Path::new(&self.temp).join("scenes.json");

    let scenes = if self.resume {
      av1an_core::split::read_scenes_from_file(scene_file.as_path())
        .unwrap()
        .0
    } else {
      self.calc_split_locations()
    };

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
    let output_ext = output_extension(&self.encoder).unwrap();
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

    let output_ext = output_extension(&self.encoder).unwrap();

    Chunk {
      temp: self.temp.clone(),
      index,
      ffmpeg_gen_cmd: vspipe_cmd_gen,
      output_ext,
      // use the number of frames to prioritize which chunks encode first, since we don't have file size
      size: frames,
      frames,
      ..Default::default()
    }
  }

  fn create_video_queue_vs(&mut self, splits: Vec<usize>, py: Python) -> Vec<Chunk> {
    let last_frame = self.get_frames(py);

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
      create_vs_file(
        &self.temp,
        &self.input,
        &self.chunk_method.as_ref().unwrap(),
      )
      .unwrap()
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

  fn create_video_queue_select(&mut self, splits: Vec<usize>, py: Python) -> Vec<Chunk> {
    let last_frame = self.get_frames(py);

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

  fn create_video_queue_segment(&mut self, splits: Vec<usize>, py: Python) -> Vec<Chunk> {
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
      .map(|(index, file)| {
        self.create_chunk_from_segment(index, file.as_path().to_str().unwrap(), py)
      })
      .collect();

    chunk_queue
  }

  fn create_video_queue_hybrid(&mut self, split_locations: Vec<usize>, py: Python) -> Vec<Chunk> {
    let keyframes = get_keyframes(&self.input).unwrap();

    let mut splits = vec![0];
    splits.extend(split_locations);
    splits.push(self.get_frames(py));

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

  fn create_chunk_from_segment(&mut self, index: usize, file: &str, py: Python) -> Chunk {
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

    let output_ext = output_extension(&self.encoder).unwrap();
    let file_size = File::open(file).unwrap().metadata().unwrap().len();

    Chunk {
      temp: self.temp.clone(),
      frames: self.get_frames(py),
      ffmpeg_gen_cmd,
      output_ext,
      index,
      size: file_size as usize,
      ..Default::default()
    }
  }

  fn load_or_gen_chunk_queue(&mut self, splits: Vec<usize>, py: Python) -> Vec<Chunk> {
    if self.resume {
      let mut chunks = read_chunk_queue(&self.temp);

      let done_path = Path::new(&self.temp).join("done.json");

      let done_contents = fs::read_to_string(&done_path).unwrap();
      let done: DoneJson = serde_json::from_str(&done_contents).unwrap();

      // only keep the chunks that are not done
      chunks.retain(|chunk| !done.done.contains_key(&chunk.name()));

      chunks
    } else {
      let chunks = self.create_encoding_queue(splits, py);
      save_chunk_queue(&self.temp, chunks.clone());
      chunks
    }
  }

  fn encode_file(mut _self: PyRefMut<Self>, py: Python) {
    let _ = log(format!("File hash: {}", hash_path(&_self.input)).as_str());

    let done_path = Path::new(&_self.temp).join("done.json");

    _self.resume = _self.resume && done_path.exists();

    if !_self.resume && Path::new(&_self.temp).is_dir() {
      fs::remove_dir_all(&_self.temp).unwrap();
    }

    let _ = match fs::create_dir_all(Path::new(&_self.temp).join("split")) {
      Ok(_) => {}
      Err(e) => match e.kind() {
        io::ErrorKind::AlreadyExists => {}
        _ => panic!("{}", e),
      },
    };
    let _ = match fs::create_dir_all(Path::new(&_self.temp).join("encode")) {
      Ok(_) => {}
      Err(e) => match e.kind() {
        io::ErrorKind::AlreadyExists => {}
        _ => panic!("{}", e),
      },
    };

    set_log(&_self.logging).unwrap();

    let splits = _self.split_routine(py);

    let chunk_queue = _self.load_or_gen_chunk_queue(splits, py);

    let done_path = Path::new(&_self.temp).join("done.json");

    let mut initial_frames: usize = 0;

    if _self.resume && done_path.exists() {
      let _ = log("Resuming...");

      let done: DoneJson = serde_json::from_str(&fs::read_to_string(&done_path).unwrap()).unwrap();
      initial_frames = done.done.iter().map(|(_, frames)| frames).sum();
      let _ = log(format!("Resmued with {} encoded clips done", done.done.len()).as_str());
    } else {
      let total = _self.get_frames(py);
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

    if !_self.resume {
      extract_audio(&_self.input, &_self.temp, _self.audio_params.clone());
    }

    if _self.workers == 0 {
      _self.workers = determine_workers(&_self.encoder).unwrap() as usize;
    }
    _self.workers = cmp::min(_self.workers, chunk_queue.len());
    println!(
      "Queue: {} Workers: {} Passes: {}\nParams: {}",
      chunk_queue.len(),
      _self.workers,
      _self.passes,
      _self.video_params.join(" ")
    );

    init_progress_bar((_self.frames - initial_frames) as u64).unwrap();

    Python::with_gil(|py| -> PyResult<()> {
      let av1an = PyModule::import(py, "av1an")?;

      // hack to avoid borrow checker errors
      let concat = _self.concat.clone();
      let temp = _self.temp.clone();
      let input = _self.input.clone();
      let output_file = _self.output_file.clone();
      let encoder = _self.encoder.clone();
      let vmaf = _self.vmaf;
      let keep = _self.keep;

      let queue = av1an.getattr("Queue")?.call1((_self, chunk_queue))?;
      queue.call_method0("encoding_loop")?;
      let status: String = queue.getattr("status")?.extract()?;

      if status.eq_ignore_ascii_case("fatal") {
        let msg = "FATAL Encoding process encountered fatal error, shutting down";
        log(msg)?;
        panic!("\n::{}", msg);
      }

      let _ = log("Concatenating");

      // TODO refactor into Concatenate trait
      match concat.as_str() {
        "ivf" => {
          av1an_core::concat::concat_ivf(&Path::new(&temp).join("encode"), Path::new(&output_file))
            .unwrap();
        }
        "mkvmerge" => {
          av1an.call_method1("concatenate_mkvmerge", (temp.clone(), output_file.clone()))?;
        }
        "ffmpeg" => {
          av1an_core::ffmpeg::concatenate_ffmpeg(
            temp.clone(),
            output_file.clone(),
            Encoder::from_str(&encoder).unwrap(),
          );
        }
        _ => unreachable!(),
      }

      if vmaf {
        plot_vmaf(&input, &output_file)?;
      }

      if !keep {
        fs::remove_dir_all(temp).unwrap();
      }

      Ok(())
    })
    .unwrap();
  }
}

#[pymodule]
fn av1an_pyo3(_py: Python, m: &PyModule) -> PyResult<()> {
  m.add_function(wrap_pyfunction!(init_progress_bar, m)?)?;
  m.add_function(wrap_pyfunction!(update_bar, m)?)?;
  m.add_function(wrap_pyfunction!(finish_progress_bar, m)?)?;
  m.add_function(wrap_pyfunction!(get_ffmpeg_info, m)?)?;
  m.add_function(wrap_pyfunction!(determine_workers, m)?)?;
  m.add_function(wrap_pyfunction!(create_vs_file, m)?)?;
  m.add_function(wrap_pyfunction!(hash_path, m)?)?;
  m.add_function(wrap_pyfunction!(adapt_probing_rate, m)?)?;
  m.add_function(wrap_pyfunction!(frame_probe_vspipe, m)?)?;
  m.add_function(wrap_pyfunction!(ffmpeg_get_frame_count, m)?)?;
  m.add_function(wrap_pyfunction!(get_keyframes, m)?)?;
  m.add_function(wrap_pyfunction!(concatenate_ivf, m)?)?;
  m.add_function(wrap_pyfunction!(construct_target_quality_command, m)?)?;
  m.add_function(wrap_pyfunction!(concatenate_ffmpeg, m)?)?;
  m.add_function(wrap_pyfunction!(extract_audio, m)?)?;
  m.add_function(wrap_pyfunction!(get_frame_types, m)?)?;
  m.add_function(wrap_pyfunction!(extra_splits, m)?)?;
  m.add_function(wrap_pyfunction!(segment, m)?)?;
  m.add_function(wrap_pyfunction!(process_inputs, m)?)?;
  m.add_function(wrap_pyfunction!(write_scenes_to_file, m)?)?;
  m.add_function(wrap_pyfunction!(read_scenes_from_file, m)?)?;
  m.add_function(wrap_pyfunction!(parse_args, m)?)?;
  m.add_function(wrap_pyfunction!(default_args, m)?)?;
  m.add_function(wrap_pyfunction!(vmaf_auto_threads, m)?)?;
  m.add_function(wrap_pyfunction!(set_log, m)?)?;
  m.add_function(wrap_pyfunction!(log, m)?)?;
  m.add_function(wrap_pyfunction!(get_default_pass, m)?)?;
  m.add_function(wrap_pyfunction!(get_default_cq_range, m)?)?;
  m.add_function(wrap_pyfunction!(get_default_arguments, m)?)?;
  m.add_function(wrap_pyfunction!(help_command, m)?)?;
  m.add_function(wrap_pyfunction!(encoder_bin, m)?)?;
  m.add_function(wrap_pyfunction!(output_extension, m)?)?;
  m.add_function(wrap_pyfunction!(compose_ffmpeg_pipe, m)?)?;
  m.add_function(wrap_pyfunction!(compose_1_1_pass, m)?)?;
  m.add_function(wrap_pyfunction!(compose_1_2_pass, m)?)?;
  m.add_function(wrap_pyfunction!(compose_2_2_pass, m)?)?;
  m.add_function(wrap_pyfunction!(find_aom_keyframes, m)?)?;
  m.add_function(wrap_pyfunction!(man_command, m)?)?;
  m.add_function(wrap_pyfunction!(match_line, m)?)?;
  m.add_function(wrap_pyfunction!(weighted_search, m)?)?;
  m.add_function(wrap_pyfunction!(probe_cmd, m)?)?;
  m.add_function(wrap_pyfunction!(get_percentile, m)?)?;
  m.add_function(wrap_pyfunction!(read_weighted_vmaf, m)?)?;
  m.add_function(wrap_pyfunction!(plot_vmaf_score_file, m)?)?;
  m.add_function(wrap_pyfunction!(validate_vmaf, m)?)?;
  m.add_function(wrap_pyfunction!(plot_vmaf, m)?)?;
  m.add_function(wrap_pyfunction!(interpolate_target_q, m)?)?;
  m.add_function(wrap_pyfunction!(interpolate_target_vmaf, m)?)?;
  m.add_function(wrap_pyfunction!(log_probes, m)?)?;
  m.add_function(wrap_pyfunction!(av_scenechange_detect, m)?)?;
  m.add_function(wrap_pyfunction!(is_vapoursynth, m)?)?;
  m.add_function(wrap_pyfunction!(save_chunk_queue, m)?)?;
  m.add_function(wrap_pyfunction!(read_chunk_queue, m)?)?;
  m.add_function(wrap_pyfunction!(frame_probe, m)?)?;

  m.add_class::<Project>()?;
  m.add_class::<Chunk>()?;

  Ok(())
}
