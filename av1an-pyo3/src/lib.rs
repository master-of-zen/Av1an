use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use av1an_core::{ChunkMethod, Encoder};

use chrono::Utc;
use once_cell::sync::OnceCell;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::usize;
use std::{collections::hash_map::DefaultHasher, path::PathBuf};

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
  let encoder = av1an_encoder_constructor::Encoder::from_str(&encoder).map_err(|_| {
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
fn frame_probe_vspipe(source: &str) -> PyResult<usize> {
  av1an_core::vapoursynth::frame_probe_vspipe(Path::new(source))
    .map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("{}", e)))
}

#[pyfunction]
fn extract_audio(input: String, temp: String, audio_params: Vec<String>) {
  let input_path = Path::new(&input);
  let temp_path = Path::new(&temp);
  av1an_core::ffmpeg::extract_audio(input_path, temp_path, audio_params);
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
fn segment(input: String, temp: String, segments: Vec<usize>) -> PyResult<()> {
  let input = Path::new(&input);
  let temp = Path::new(&temp);
  av1an_core::split::segment(input, temp, segments);
  Ok(())
}

#[pyfunction]
fn process_inputs(input: Vec<String>) -> Vec<String> {
  let path_bufs: Vec<PathBuf> = input
    .into_iter()
    .map(|x| PathBuf::from_str(x.as_str()).unwrap())
    .collect();

  let processed = av1an_core::file_validation::process_inputs(path_bufs);

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
  scenes_path_string: String,
) -> PyResult<()> {
  let scene_path = PathBuf::from(scenes_path_string);

  av1an_core::split::write_scenes_to_file(scenes, frames, scene_path).unwrap();
  Ok(())
}

#[pyfunction]
fn read_scenes_from_file(scenes_path_string: String) -> (Vec<usize>, usize) {
  let scene_path = PathBuf::from(scenes_path_string);

  av1an_core::split::read_scenes_from_file(scene_path).unwrap()
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

static LOG_HANDLE: OnceCell<File> = OnceCell::new();

#[pyfunction]
fn set_log(file: &str) -> PyResult<()> {
  LOG_HANDLE
    .set(File::create(file).map_err(|e| {
      pyo3::exceptions::PyOSError::new_err(format!("Failed to create file {:?}: {}", file, e))
    })?)
    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Failed to set the global log handle"))
}

#[pyfunction]
fn log(msg: &str) {
  if let Some(mut file) = LOG_HANDLE.get() {
    file
      .write_all(format!("[{}] {}\n", Utc::now().to_rfc2822(), msg).as_bytes())
      .unwrap();
  }
}

#[pyfunction]
fn get_default_pass(encoder: String) -> PyResult<usize> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .get_default_pass(),
  )
}

#[pyfunction]
fn get_default_cq_range(encoder: String) -> PyResult<(usize, usize)> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .get_default_cq_range(),
  )
}

#[pyfunction]
fn get_default_arguments(encoder: String) -> PyResult<Vec<String>> {
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
fn help_command(encoder: String) -> PyResult<Vec<String>> {
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
fn encoder_bin(encoder: String) -> PyResult<String> {
  Ok(
    av1an_encoder_constructor::Encoder::from_str(&encoder)
      .unwrap()
      .encoder_bin()
      .into(),
  )
}

#[pyfunction]
fn output_extension(encoder: String) -> PyResult<String> {
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
  let enc = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();

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
) -> PyResult<(Vec<String>, Vec<String>)> {
  let encoder = av1an_encoder_constructor::Encoder::from_str(&encoder).unwrap();
  Ok(encoder.probe_cmd(temp, name, q, ffmpeg_pipe, probing_rate, n_threads))
}

#[pyfunction]
pub fn get_percentile(scores: Vec<f64>, percent: f64) -> PyResult<f64> {
  Ok(av1an_core::get_percentile(scores, percent))
}

#[pyfunction]
pub fn read_weighted_vmaf(fl: String, percentile: f64) -> PyResult<f64> {
  let file = PathBuf::from(fl);
  let val = av1an_core::read_weighted_vmaf(file, percentile).unwrap();
  Ok(val)
}
const INDICATIF_PROGRESS_TEMPLATE: &str =
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps}, eta {eta})";

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

#[pyfunction]
pub fn init_progress_bar(len: u64) {
  PROGRESS_BAR.get_or_init(|| {
    let bar = ProgressBar::new(len);
    bar.set_style(
      ProgressStyle::default_bar()
        .template(INDICATIF_PROGRESS_TEMPLATE)
        .progress_chars("#>-"),
    );
    bar.enable_steady_tick(100);
    bar
  });
}

#[pyfunction]
pub fn update_bar(inc: u64) {
  PROGRESS_BAR
    .get()
    .expect("The progress bar was not initialized!")
    .inc(inc)
}

#[pyfunction]
pub fn finish_progress_bar() {
  PROGRESS_BAR
    .get()
    .expect("The progress bar was not initialized!")
    .finish();
}

#[pyfunction]
pub fn plot_vmaf_score_file(scores_file_string: String, plot_path_string: String) {
  let scores_file = PathBuf::from(scores_file_string);
  let plot_path = PathBuf::from(plot_path_string);
  av1an_core::vmaf::plot_vmaf_score_file(scores_file, plot_path).unwrap()
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

  Ok(())
}
