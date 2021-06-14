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
  encoder: String,
  threads: String,
  q: String,
) -> PyResult<Vec<String>> {
  let encoder = Encoder::from_str(&encoder).map_err(|_| {
    pyo3::exceptions::PyTypeError::new_err(format!("Unknown or unsupported encoder '{}'", encoder))
  })?;

  Ok(av1an_core::target_quality::construct_target_quality_command(encoder, threads, q))
}

#[pyfunction]
fn construct_target_quality_slow_command(encoder: String, q: String) -> PyResult<Vec<String>> {
  let encoder = Encoder::from_str(&encoder).map_err(|_| {
    pyo3::exceptions::PyTypeError::new_err(format!("Unknown or unsupported encoder '{}'", encoder))
  })?;

  Ok(av1an_core::target_quality::construct_target_quality_slow_command(encoder, q))
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

#[pymodule]
fn av1an_pyo3(_py: Python, m: &PyModule) -> PyResult<()> {
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
  m.add_function(wrap_pyfunction!(construct_target_quality_slow_command, m)?)?;
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

  Ok(())
}
