use std::fmt::{Display, Write as FmtWrite};
use std::fs::{self, DirEntry, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use av_format::buffer::AccReader;
use av_format::demuxer::{Context as DemuxerContext, Event};
use av_format::muxer::{Context as MuxerContext, Writer};
use av_ivf::demuxer::IvfDemuxer;
use av_ivf::muxer::IvfMuxer;
use path_abs::{PathAbs, PathInfo};
use serde::{Deserialize, Serialize};

use crate::encoder::Encoder;
use crate::util::read_in_dir;

#[derive(
  PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr,
)]
pub enum ConcatMethod {
  #[strum(serialize = "mkvmerge")]
  MKVMerge,
  #[strum(serialize = "ffmpeg")]
  FFmpeg,
  #[strum(serialize = "ivf")]
  Ivf,
}

impl Display for ConcatMethod {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(<&'static str>::from(self))
  }
}

pub fn sort_files_by_filename(files: &mut [PathBuf]) {
  files.sort_unstable_by_key(|x| {
    // If the temp directory follows the expected format of 00000.ivf, 00001.ivf, etc.,
    // then these unwraps will not fail
    x.file_stem()
      .unwrap()
      .to_str()
      .unwrap()
      .parse::<u32>()
      .unwrap()
  });
}

pub fn ivf(input: &Path, out: &Path) -> anyhow::Result<()> {
  let mut files: Vec<PathBuf> = read_in_dir(input)?.collect();

  sort_files_by_filename(&mut files);

  assert!(!files.is_empty());

  let output = File::create(out)?;

  let mut muxer = MuxerContext::new(IvfMuxer::new(), Writer::new(output));

  let global_info = {
    let acc = AccReader::new(std::fs::File::open(&files[0]).unwrap());
    let mut demuxer = DemuxerContext::new(IvfDemuxer::new(), acc);

    demuxer.read_headers().unwrap();

    // attempt to set the duration correctly
    let duration = demuxer.info.duration.unwrap_or(0)
      + files
        .iter()
        .skip(1)
        .filter_map(|file| {
          let acc = AccReader::new(std::fs::File::open(file).unwrap());
          let mut demuxer = DemuxerContext::new(IvfDemuxer::new(), acc);

          demuxer.read_headers().unwrap();
          demuxer.info.duration
        })
        .sum::<u64>();

    let mut info = demuxer.info;
    info.duration = Some(duration);
    info
  };

  muxer.set_global_info(global_info)?;

  muxer.configure()?;
  muxer.write_header()?;

  let mut pos_offset: usize = 0;
  for file in &files {
    let mut last_pos: usize = 0;
    let input = std::fs::File::open(file)?;

    let acc = AccReader::new(input);

    let mut demuxer = DemuxerContext::new(IvfDemuxer::new(), acc);
    demuxer.read_headers()?;

    trace!("global info: {:#?}", demuxer.info);

    loop {
      match demuxer.read_event() {
        Ok(event) => match event {
          Event::MoreDataNeeded(sz) => panic!("needed more data: {sz} bytes"),
          Event::NewStream(s) => panic!("new stream: {s:?}"),
          Event::NewPacket(mut packet) => {
            if let Some(p) = packet.pos.as_mut() {
              last_pos = *p;
              *p += pos_offset;
            }

            trace!("received packet with pos: {:?}", packet.pos);
            muxer.write_packet(Arc::new(packet))?;
          }
          Event::Continue => continue,
          Event::Eof => {
            trace!("EOF received.");
            break;
          }
          _ => unimplemented!(),
        },
        Err(e) => {
          error!("{:?}", e);
          break;
        }
      }
    }
    pos_offset += last_pos + 1;
  }

  muxer.write_trailer()?;

  Ok(())
}

fn read_encoded_chunks(encode_dir: &Path) -> anyhow::Result<Vec<DirEntry>> {
  Ok(
    fs::read_dir(encode_dir)
      .with_context(|| format!("Failed to read encoded chunks from {:?}", &encode_dir))?
      .collect::<Result<Vec<_>, _>>()?,
  )
}

pub fn mkvmerge(
  temp_dir: &Path,
  output: &Path,
  encoder: Encoder,
  num_chunks: usize,
) -> anyhow::Result<()> {
  // mkvmerge does not accept UNC paths on Windows
  #[cfg(windows)]
  fn fix_path<P: AsRef<Path>>(p: P) -> String {
    const UNC_PREFIX: &str = r#"\\?\"#;

    let p = p.as_ref().display().to_string();
    if let Some(path) = p.strip_prefix(UNC_PREFIX) {
      if let Some(p2) = path.strip_prefix("UNC") {
        format!("\\{}", p2)
      } else {
        path.to_string()
      }
    } else {
      p
    }
  }

  #[cfg(not(windows))]
  fn fix_path<P: AsRef<Path>>(p: P) -> String {
    p.as_ref().display().to_string()
  }

  let mut audio_file = PathBuf::from(&temp_dir);
  audio_file.push("audio.mkv");
  let audio_file = PathAbs::new(&audio_file)?;
  let audio_file = if audio_file.as_path().exists() {
    Some(fix_path(audio_file))
  } else {
    None
  };

  let mut encode_dir = PathBuf::from(temp_dir);
  encode_dir.push("encode");

  let output = PathAbs::new(output)?;

  assert!(num_chunks != 0);

  let options_path = PathBuf::from(&temp_dir).join("options.json");
  let options_json_contents = mkvmerge_options_json(
    num_chunks,
    encoder,
    &fix_path(output.to_str().unwrap()),
    audio_file.as_deref(),
  );

  let mut options_json = File::create(options_path)?;
  options_json.write_all(options_json_contents.as_bytes())?;

  let mut cmd = Command::new("mkvmerge");
  cmd.current_dir(&encode_dir);
  cmd.arg("@../options.json");

  let out = cmd
    .output()
    .with_context(|| "Failed to execute mkvmerge command for concatenation")?;

  if !out.status.success() {
    // TODO: make an EncoderCrash-like struct, but without all the other fields so it
    // can be used in a more broad scope than just for the pipe/encoder
    error!(
      "mkvmerge concatenation failed with output: {:#?}\ncommand: {:?}",
      out, cmd
    );
    return Err(anyhow!("mkvmerge concatenation failed"));
  }

  Ok(())
}

/// Create mkvmerge options.json
pub fn mkvmerge_options_json(
  num: usize,
  encoder: Encoder,
  output: &str,
  audio: Option<&str>,
) -> String {
  let mut file_string = String::with_capacity(64 + 12 * num);
  write!(file_string, "[\"-o\", {output:?}").unwrap();
  if let Some(audio) = audio {
    write!(file_string, ", {audio:?}").unwrap();
  }
  file_string.push_str(", \"[\"");
  for i in 0..num {
    write!(file_string, ", \"{i:05}.{}\"", encoder.output_extension()).unwrap();
  }
  file_string.push_str(",\"]\"]");

  file_string
}

/// Concatenates using ffmpeg (does not work with x265)
pub fn ffmpeg(temp: &Path, output: &Path) -> anyhow::Result<()> {
  fn write_concat_file(temp_folder: &Path) -> anyhow::Result<()> {
    let concat_file = temp_folder.join("concat");
    let encode_folder = temp_folder.join("encode");

    let mut files = read_encoded_chunks(&encode_folder)?;

    files.sort_by_key(DirEntry::path);

    let mut contents = String::with_capacity(24 * files.len());

    for i in files {
      writeln!(
        contents,
        "file {}",
        format!("{}", i.path().display())
          .replace('\\', r"\\")
          .replace(' ', r"\ ")
          .replace('\'', r"\'")
      )?;
    }

    let mut file = File::create(concat_file)?;
    file.write_all(contents.as_bytes())?;

    Ok(())
  }

  let temp = PathAbs::new(temp)?;
  let temp = temp.as_path();

  let concat = temp.join("concat");
  let concat_file = concat.to_str().unwrap();

  write_concat_file(temp)?;

  let audio_file = {
    let file = temp.join("audio.mkv");
    if file.exists() && file.metadata().unwrap().len() > 1000 {
      Some(file)
    } else {
      None
    }
  };

  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  if let Some(file) = audio_file {
    cmd
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
        concat_file,
        "-i",
      ])
      .arg(file)
      .args(["-map", "0", "-map", "1", "-c", "copy"])
      .arg(output);
  } else {
    cmd
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
        concat_file,
      ])
      .args(["-map", "0", "-c", "copy"])
      .arg(output);
  }

  debug!("FFmpeg concat command: {:?}", cmd);

  let out = cmd
    .output()
    .with_context(|| "Failed to execute FFmpeg command for concatenation")?;

  if !out.status.success() {
    error!(
      "FFmpeg concatenation failed with output: {:#?}\ncommand: {:?}",
      out, cmd
    );
    return Err(anyhow!("FFmpeg concatenation failed"));
  }

  Ok(())
}
