use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use regex::Regex;
use std::{
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
  path::Path,
};
use std::{
  io::{self, stderr, Write},
  os::unix::prelude::CommandExt,
};
use std::{
  process::{Command, Stdio},
  str,
};

#[pyfunction]
/// Get frame count. Direct counting of frame count using ffmpeg
fn ffmpeg_get_frame_count(source: String) -> PyResult<usize> {
  let source_path = Path::new(&source);

  let mut cmd = Command::new("ffmpeg");
  cmd.args(&[
    "-hide_banner",
    "-i",
    source_path.to_str().unwrap(),
    "-map",
    "0:v:0",
    "-c",
    "copy",
    "-f",
    "null",
    "-",
  ]);

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;
  assert!(out.status.success());

  let re = Regex::new(r".*frame=\s*([0-9]+)\s").unwrap();
  let output = String::from_utf8(out.stderr).unwrap();

  // io::stdout().write_all(output.as_bytes()).unwrap();
  // dbg!(&output);
  let cap = re.captures(&output).unwrap();

  let frame_count = cap[cap.len() - 1].parse::<usize>().unwrap();
  Ok(frame_count)
}

#[pyfunction]
fn get_ffmpeg_info() -> PyResult<String> {
  let mut cmd = Command::new("ffmpeg");

  cmd.stderr(Stdio::piped());

  let output = String::from_utf8(cmd.output().unwrap().stderr).unwrap();

  Ok(output)
}

#[pyfunction]
fn adapt_probing_rate(_frames: usize, rate: usize) -> PyResult<usize> {
  let new_rate = match rate {
    1..=4 => rate,
    _ => 4,
  } as usize;
  Ok(new_rate)
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
  m.add_function(wrap_pyfunction!(adapt_probing_rate, m)?)?;
  m.add_function(wrap_pyfunction!(ffmpeg_get_frame_count, m)?)?;

  Ok(())
}
