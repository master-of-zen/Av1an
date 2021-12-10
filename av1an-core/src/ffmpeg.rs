use crate::{into_array, into_vec};
use ffmpeg_next::format::{input, Pixel};
use ffmpeg_next::media::Type as MediaType;
use ffmpeg_next::Error::StreamNotFound;
use path_abs::{PathAbs, PathInfo};
use std::{
  ffi::OsStr,
  path::Path,
  process::{Command, Stdio},
};

pub fn compose_ffmpeg_pipe<S: Into<String>>(
  params: impl IntoIterator<Item = S>,
  pix_format: Pixel,
) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params.into_iter().map(Into::into));

  p.extend(into_array![
    "-pix_fmt",
    pix_format.descriptor().unwrap().name(),
    "-strict",
    "-1",
    "-f",
    "yuv4mpegpipe",
    "-"
  ]);

  p
}

/// Get frame count using FFmpeg
pub fn num_frames(source: &Path) -> Result<usize, ffmpeg_next::Error> {
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

pub fn get_pixel_format(source: &Path) -> Result<Pixel, ffmpeg_next::Error> {
  let ictx = ffmpeg_next::format::input(&source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = input.codec().decoder().video()?;

  Ok(decoder.format())
}

/// Returns vec of all keyframes
pub fn get_keyframes(source: &Path) -> Result<Vec<usize>, ffmpeg_next::Error> {
  let mut ictx = input(&source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
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
    return Ok(vec![0]);
  };

  Ok(kfs)
}

/// Returns true if input file have audio in it
pub fn has_audio(file: &Path) -> bool {
  let ictx = input(&file).unwrap();
  ictx.streams().best(MediaType::Audio).is_some()
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

    encode_audio.args([
      "-map_metadata",
      "0",
      "-vn",
      "-map",
      "0",
      "-map",
      "-0:a",
      "-c",
      "copy",
      "-map",
      "0:a",
    ]);

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

/// Escapes paths in ffmpeg filters if on windows
pub fn escape_path_in_filter(path: impl AsRef<Path>) -> String {
  if cfg!(windows) {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      // This is needed because of how FFmpeg handles absolute file paths on Windows.
      // https://stackoverflow.com/questions/60440793/how-can-i-use-windows-absolute-paths-with-the-movie-filter-on-ffmpeg
      .replace('\\', "/")
      .replace(':', r"\\:")
  } else {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }
}
