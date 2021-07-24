use av_format::buffer::AccReader;
use av_format::demuxer::Context as DemuxerContext;
use av_format::demuxer::Event;
use av_format::muxer::Context as MuxerContext;
use av_ivf::demuxer::IvfDemuxer;
use av_ivf::muxer::IvfMuxer;
use std::fs::{read_dir, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

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

pub fn concat_ivf(input: &Path, out: &Path) -> anyhow::Result<()> {
  let mut files: Vec<PathBuf> = std::fs::read_dir(input)?
    .into_iter()
    .filter_map(Result::ok)
    .filter_map(|d| {
      if let Ok(file_type) = d.file_type() {
        if file_type.is_file() {
          Some(d.path())
        } else {
          None
        }
      } else {
        None
      }
    })
    .collect();

  sort_files_by_filename(&mut files);

  assert!(!files.is_empty());

  let output = File::create(out)?;

  let mut muxer = MuxerContext::new(Box::new(IvfMuxer::new()), Box::new(output));

  let global_info = {
    let acc = AccReader::new(std::fs::File::open(&files[0]).unwrap());
    let mut demuxer = DemuxerContext::new(Box::new(IvfDemuxer::new()), Box::new(acc));

    demuxer.read_headers().unwrap();

    // attempt to set the duration correctly
    let duration = demuxer.info.duration.unwrap_or(0)
      + files
        .iter()
        .skip(1)
        .filter_map(|file| {
          let acc = AccReader::new(std::fs::File::open(file).unwrap());
          let mut demuxer = DemuxerContext::new(Box::new(IvfDemuxer::new()), Box::new(acc));

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

    let mut demuxer = DemuxerContext::new(Box::new(IvfDemuxer::new()), Box::new(acc));
    demuxer.read_headers()?;

    trace!("global info: {:#?}", demuxer.info);

    loop {
      match demuxer.read_event() {
        Ok(event) => match event {
          Event::MoreDataNeeded(sz) => panic!("needed more data: {} bytes", sz),
          Event::NewStream(s) => panic!("new stream: {:?}", s),
          Event::NewPacket(mut packet) => {
            if let Some(p) = packet.pos.as_mut() {
              last_pos = *p;
              *p += pos_offset;
            }

            debug!("received packet with pos: {:?}", packet.pos);
            muxer.write_packet(Arc::new(packet))?;
          }
          Event::Continue => continue,
          Event::Eof => {
            debug!("EOF received.");
            break;
          }
          _ => unimplemented!(),
        },
        Err(e) => {
          debug!("error: {:?}", e);
          break;
        }
      }
    }
    pos_offset += last_pos + 1;
  }

  muxer.write_trailer()?;

  Ok(())
}

pub fn concatenate_mkvmerge(encode_folder: String, output: String) -> Result<(), anyhow::Error> {
  let mut encode_folder = PathBuf::from(encode_folder);

  let mut audio_file = PathBuf::from(encode_folder.as_os_str());
  audio_file.push("audio.mkv");

  encode_folder.push("encode");
  let output = PathBuf::from(output);

  let mut files: Vec<_> = read_dir(&encode_folder)
    .unwrap()
    .map(Result::unwrap)
    .collect();

  files.sort_by_key(std::fs::DirEntry::path);

  let mut cmd = Command::new("mkvmerge");
  cmd.args([
    "-o",
    output.as_os_str().to_str().unwrap(),
    "--append-mode",
    "file",
  ]);

  if audio_file.exists() {
    cmd.arg(audio_file.as_os_str().to_str().unwrap());
  };

  let mut append_args = Vec::new();
  for fl in files {
    append_args.push(fl.path().as_os_str().to_str().unwrap().to_string());
    append_args.push("+".to_string());
  }
  append_args.pop().unwrap();

  cmd.args(append_args);
  cmd.output().unwrap();
  Ok(())
}
