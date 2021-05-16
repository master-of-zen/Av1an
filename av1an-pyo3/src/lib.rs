use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use av1an_core::{ChunkMethod, Encoder};

use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;

#[pyfunction]
fn adapt_probing_rate(_frames: usize, rate: usize) -> usize {
  av1an_core::adapt_probing_rate(rate)
}

#[pyfunction]
fn hash_path(path: &str) -> PyResult<String> {
  let mut s = DefaultHasher::new();
  path.hash(&mut s);
  let hs = s.finish().to_string();
  let out = hs[0..7].to_string();
  Ok(out)
}

/// Creates vs pipe file
#[pyfunction]
fn create_vs_file(temp: &str, source: &str, chunk_method: &str) -> PyResult<String> {
  // only for python code, remove if being called by rust
  let temp = Path::new(temp);
  let source = Path::new(source);
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

/// A Python module implemented in Rust.
#[pymodule]
fn av1an_pyo3(_py: Python, m: &PyModule) -> PyResult<()> {
  m.add_function(wrap_pyfunction!(get_ffmpeg_info, m)?)?;
  m.add_function(wrap_pyfunction!(determine_workers, m)?)?;
  m.add_function(wrap_pyfunction!(create_vs_file, m)?)?;
  m.add_function(wrap_pyfunction!(hash_path, m)?)?;
  m.add_function(wrap_pyfunction!(adapt_probing_rate, m)?)?;
  m.add_function(wrap_pyfunction!(frame_probe_vspipe, m)?)?;

  Ok(())
}
