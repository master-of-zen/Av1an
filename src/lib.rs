use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use std::process::{Command, Stdio};

/// Formats the sum of two numbers as string.
#[pyfunction]
fn get_ffmpeg_info() -> PyResult<String> {
    let mut cmd = Command::new("ffmpeg");

    cmd.stderr(Stdio::piped());

    let output = String::from_utf8(cmd.output().unwrap().stderr).unwrap();

    Ok(output)
}

/// A Python module implemented in Rust.
#[pymodule]
fn av1an_rust(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_ffmpeg_info, m)?)?;

    Ok(())
}
