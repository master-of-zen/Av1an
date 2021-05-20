use failure::Error;
use regex::Regex;
use std::fs::{read_dir, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Encoder;

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

  cap[cap.len() - 1].parse::<usize>().unwrap()
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

pub fn write_concat_file(temp_folder: &Path) {
  let concat_file = &temp_folder.join("concat");
  let encode_folder = &temp_folder.join("encode");
  let files = read_dir(encode_folder).unwrap();

  let mut fls = vec![];

  for i in files {
    fls.push(i.unwrap());
  }

  let mut contents = String::new();

  for i in fls {
    contents.push_str(format!("file {}\n", i.path().display()).as_str());
  }

  let mut file = File::create(concat_file).unwrap();
  file.write_all(contents.as_bytes());
}

pub fn have_audio(file: &Path) -> bool {
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let re = Regex::new(r".*Stream.+(Audio)").unwrap();

  cmd.args(&["-hide_banner", "-i", file.to_str().unwrap()]);

  let out = cmd.output().unwrap();

  let output = String::from_utf8(out.stderr).unwrap();

  re.is_match(&output)
}

/// Extracting audio
pub fn extract_audio(input: &Path, temp: &Path, audio_params: Vec<String>) {
  let have_audio = have_audio(input);

  if have_audio {
    let audio_file = Path::new(temp).join("audio.mkv");
    let mut process_audio = Command::new("ffmpeg");

    process_audio.stdout(Stdio::piped());
    process_audio.stderr(Stdio::piped());

    process_audio.args([
      "-y",
      "-hide_banner",
      "-loglevel",
      "error",
      "-i",
      input.to_str().unwrap(),
      "-map_metadata",
      "-1",
      "-dn",
      "-vn",
    ]);

    let audio_args: Vec<&str> = audio_params.iter().map(|x| &**x).collect();
    process_audio.args(audio_args);
    process_audio.arg(audio_file.to_str().unwrap());

    let out = process_audio.output().unwrap();
    assert!(out.status.success());
  }
}

/// Concatenates using ffmpeg
pub fn concatenate_ffmpeg(temp: &Path, output: &Path, encoder: Encoder) {
  let out = Path::new(&output);
  let concat = &temp.join("concat");
  let concat_file = concat.to_str().unwrap();

  write_concat_file(&temp);

  let audio_file = Path::new(&temp).join("audio.mkv");

  let mut audio_cmd = vec![];

  if audio_file.exists() {
    if audio_file.metadata().unwrap().len() > 1000 {
      audio_cmd = vec!["-i", audio_file.to_str().unwrap(), "-c", "copy"];
    }
  }

  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  match encoder {
    Encoder::x265 => cmd
      .args(&[
        "-y",
        "-fflags",
        "+genpts",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &concat_file,
      ])
      .args(audio_cmd)
      .args(&[
        "-c",
        "copy",
        "-movflags",
        "frag_keyframe+empty_moov",
        "-map",
        "0",
        "-f",
        "mp4",
        output.to_str().unwrap(),
      ]),

    _ => cmd
      .args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &concat_file,
      ])
      .args(audio_cmd)
      .args(["-c", "copy", "-sn", "-map", "0", output.to_str().unwrap()]),
  };
  let out = cmd.output().unwrap();

  assert!(out.status.success());
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

  let str_vec = output.split("\n").collect::<Vec<_>>();

  let string_vec: Vec<String> = str_vec.iter().map(|x| x.to_string()).collect();

  string_vec
}
