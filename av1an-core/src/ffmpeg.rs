use crate::{into_vec, regex};
use ffmpeg_next::format::input;
use ffmpeg_next::media::Type as MediaType;
use ffmpeg_next::picture::Type as PictureType;
use ffmpeg_next::Error::StreamNotFound;
use path_abs::{PathAbs, PathInfo};
use std::{
  ffi::OsStr,
  path::Path,
  process::{Command, Stdio},
};

pub fn compose_ffmpeg_pipe(params: Vec<String>) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params);

  p
}
/// Get frame count. Direct counting of frame count using ffmpeg
pub fn num_frames(source: &Path) -> anyhow::Result<usize> {
  let mut ictx = input(&source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let video_stream_index = input.index();

  Ok(
    ictx
      .packets()
      .filter(|(stream, _)| stream.index() == video_stream_index)
      .count(),
  )
}

/// Returns vec of all keyframes
pub fn get_keyframes<P: AsRef<Path>>(source: P) -> Vec<usize> {
  let mut ictx = input(&source).unwrap();
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)
    .unwrap();
  let video_stream_index = input.index();

  let kfs = ictx
    .packets()
    .filter(|(stream, _)| stream.index() == video_stream_index)
    .map(|(_, packet)| packet)
    .enumerate()
    .filter(|(_, packet)| packet.is_key())
    .map(|(i, _)| i)
    .collect::<Vec<_>>();

  if kfs.is_empty() {
    return vec![0];
  };

  kfs
}

/// Returns true if input file have audio in it
pub fn has_audio(file: &Path) -> bool {
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  cmd.args(&["-hide_banner", "-i", file.to_str().unwrap()]);

  let out = cmd.output().unwrap();

  let output = String::from_utf8(out.stderr).unwrap();

  regex!(r".*Stream.+(Audio)").is_match(&output)
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
    encode_audio.args(["-map_metadata", "0", "-dn", "-vn", "-sn"]);

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

/// Returns list of frame types of the video
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

/// Escapes paths in ffmpeg filters if on windows
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

/// Check for `FFmpeg`
pub fn get_ffmpeg_info() -> String {
  let mut cmd = Command::new("ffmpeg");
  cmd.stderr(Stdio::piped());
  String::from_utf8(cmd.output().unwrap().stderr).unwrap()
}
