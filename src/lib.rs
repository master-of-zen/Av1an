use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use std::process::{Command, Stdio};
use std::{
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
};

/// Formats the sum of two numbers as string.
#[pyfunction]
fn get_ffmpeg_info() -> PyResult<String> {
  let mut cmd = Command::new("ffmpeg");

  cmd.stderr(Stdio::piped());

  let output = String::from_utf8(cmd.output().unwrap().stderr).unwrap();

  Ok(output)
}

#[pyfunction]
fn hash_path(path: String) -> PyResult<String> {
  let mut s = DefaultHasher::new();
  path.hash(&mut s);
  let hs = s.finish().to_string();
  let out = hs[0..7].to_string();
  Ok(out)
}

/// A Python module implemented in Rust.
#[pymodule]
fn av1an(_py: Python, m: &PyModule) -> PyResult<()> {
  m.add_function(wrap_pyfunction!(get_ffmpeg_info, m)?)?;
  m.add_function(wrap_pyfunction!(hash_path, m)?)?;

  Ok(())
}
