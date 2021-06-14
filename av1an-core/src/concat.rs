use av_format::buffer::AccReader;
use av_format::demuxer::Context as DemuxerContext;
use av_format::demuxer::Event;
use av_format::muxer::Context as MuxerContext;
use av_ivf::demuxer::*;
use av_ivf::muxer::*;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn concat_ivf(input: &Path, out: &Path) -> anyhow::Result<()> {
  let mut files: Vec<PathBuf> = std::fs::read_dir(input)?
    .into_iter()
    .filter_map(|d| d.ok())
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

  assert!(!files.is_empty());

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
  for file in files.iter() {
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
