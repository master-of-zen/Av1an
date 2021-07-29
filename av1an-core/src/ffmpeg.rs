use path_abs::{PathAbs, PathInfo};
use regex::Regex;
use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

/// Get frame count. Direct counting of frame count using ffmpeg
pub fn get_frame_count(source: impl AsRef<Path>) -> usize {
  let source = source.as_ref();

  let mut cmd = Command::new("ffmpeg");
  cmd.args(&[
    "-hide_banner",
    "-i",
    source.to_str().unwrap(),
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

  cap[cap.len() - 1].parse::<usize>().unwrap()
}

/// Returns vec of all keyframes
pub fn get_keyframes<P: AsRef<Path>>(source: P) -> Vec<usize> {
  let source = source.as_ref();
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

pub fn has_audio(file: &Path) -> bool {
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let re = Regex::new(r".*Stream.+(Audio)").unwrap();

  cmd.args(&["-hide_banner", "-i", file.to_str().unwrap()]);

  let out = cmd.output().unwrap();

  let output = String::from_utf8(out.stderr).unwrap();

  re.is_match(&output)
}

/// Encodes the audio using FFmpeg, blocking the current thread.
///
/// This function returns `true` if the audio exists and the audio
/// successfully encoded, or `false` otherwise.
#[must_use]
pub fn encode_audio<S: AsRef<OsStr>>(
  input: impl AsRef<Path>,
  temp: impl AsRef<Path>,
  audio_params: &[S],
) -> bool {
  let input = input.as_ref();
  let temp = temp.as_ref();

  if has_audio(input) {
    let audio_file = Path::new(temp).join("audio.mkv");
    let mut encode_audio = Command::new("ffmpeg");

    encode_audio.stdout(Stdio::piped());
    encode_audio.stderr(Stdio::piped());

    encode_audio.args(["-y", "-hide_banner", "-loglevel", "error", "-i"]);
    encode_audio.arg(input);
    encode_audio.args(["-map_metadata", "-1", "-dn", "-vn"]);

    encode_audio.args(audio_params);
    encode_audio.arg(audio_file);

    let output = encode_audio.output().unwrap();

    if !output.status.success() {
      warn!(
        "FFmpeg failed to encode audio!\n{:#?}\nParams: {:?}",
        output, encode_audio
      );
      return false;
    }

    true
  } else {
    false
  }
}

pub fn get_frame_types(file: &Path) -> Vec<String> {
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let args = [
    "ffmpeg",
    "-hide_banner",
    "-i",
    file.to_str().unwrap(),
    "-vf",
    "showinfo",
    "-f",
    "null",
    "-loglevel",
    "debug",
    "-",
  ];

  cmd.args(args);

  let out = cmd.output().unwrap();

  assert!(out.status.success());

  let output = String::from_utf8(out.stderr).unwrap();

  let str_vec = output.split('\n').collect::<Vec<_>>();

  let string_vec: Vec<String> = str_vec.iter().map(|s| (*s).to_string()).collect();

  string_vec
}

pub fn escape_path_in_filter(path: impl AsRef<Path>) -> String {
  if cfg!(target_os = "windows") {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      // This is needed because of how FFmpeg handles absolute file paths on Windows.
      // https://stackoverflow.com/questions/60440793/how-can-i-use-windows-absolute-paths-with-the-movie-filter-on-ffmpeg
      .replace(r"\", "/")
      .replace(":", r"\\:")
  } else {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }
}
