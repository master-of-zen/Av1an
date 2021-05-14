use av_format::buffer::AccReader;
use av_format::demuxer::Context as DemuxerContext;
use av_format::demuxer::Event;
use av_format::muxer::Context as MuxerContext;
use av_ivf::demuxer::*;
use av_ivf::muxer::*;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;

// pub fn concatenate_ivf(out: &Path) -> anyhow::Result<()> {
//     let files = [];

//     let output = File::create(out)?;

//     let mux = Box::new(IvfMuxer::new());
//     let mut muxer = MuxerContext::new(mux, Box::new(output));
//     // muxer.set_global_info(demuxer.info.clone())?;
//     muxer.configure()?;
//     muxer.write_header()?;

//     let mut pos_offset: usize = 0;
//     for file in files.iter() {
//         let mut last_pos: usize = 0;
//         let input = std::fs::File::open(file)
//             .with_context(|| format!("Input file {:?} does not exist.", file))?;

//         let acc = AccReader::new(input);

//         let mut demuxer = DemuxerContext::new(Box::new(IvfDemuxer::new()), Box::new(acc));
//         demuxer.read_headers()?;

//         trace!("global info: {:#?}", demuxer.info);

//         loop {
//             match demuxer.read_event() {
//                 Ok(event) => match event {
//                     Event::MoreDataNeeded(sz) => panic!("we needed more data: {} bytes", sz),
//                     Event::NewStream(s) => panic!("new stream: {:?}", s),
//                     Event::NewPacket(mut packet) => {
//                         if let Some(p) = packet.pos.as_mut() {
//                             last_pos = *p;
//                             *p += pos_offset;
//                         }

//                         debug!("received packet with pos: {:?}", packet.pos);
//                         muxer.write_packet(Arc::new(packet))?;
//                     }
//                     Event::Continue => continue,
//                     Event::Eof => {
//                         debug!("EOF received.");
//                         break;
//                     }
//                     _ => unimplemented!(),
//                 },
//                 Err(e) => {
//                     debug!("error: {:?}", e);
//                     break;
//                 }
//             }
//         }
//         pos_offset += last_pos + 1;
//     }

//     muxer.write_trailer()?;

//     Ok(())
// }
