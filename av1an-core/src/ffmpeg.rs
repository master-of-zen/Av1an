use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Get frame count. Direct counting of frame count using ffmpeg
pub fn ffmpeg_get_frame_count(source: &Path) -> usize {
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

  let out = cmd.output().unwrap();

  assert!(out.status.success());

  let re = Regex::new(r".*frame=\s*([0-9]+)\s").unwrap();
  let output = String::from_utf8(out.stderr).unwrap();

  let cap = re.captures(&output).unwrap();

  let frame_count = cap[cap.len() - 1].parse::<usize>().unwrap();
  frame_count
}

/// Returns vec of all keyframes
pub fn get_keyframes(source: &Path) -> Vec<usize> {
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  cmd.args(&[
    "-hide_banner",
    "-i",
    source.to_str().unwrap(),
    "-vf",
    r"select=eq(pict_type\,PICT_TYPE_I)",
    "-f",
    "null",
    "-loglevel",
    "debug",
    "-",
  ]);

  let out = cmd.output().unwrap();
  assert!(out.status.success());

  let re = Regex::new(r".*n:([0-9]+)\.[0-9]+ pts:.+key:1").unwrap();
  let output = String::from_utf8(out.stderr).unwrap();
  let mut kfs: Vec<usize> = vec![];
  for found in re.captures_iter(&output) {
    kfs.push(found.get(1).unwrap().as_str().parse::<usize>().unwrap());
  }

  if kfs.is_empty() {
    return vec![0];
  };

  kfs
}
