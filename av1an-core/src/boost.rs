use std::process::{Stdio, Command};

use crate::{chunk::Chunk, encoder::Encoder};

const BOOST_THRESHOLD: f32 = 50.0;

pub fn boost_low_luma(chunk: &Chunk, encoder: Encoder) -> Option<usize> {
  if let Ok(luma) = get_avg_luma(chunk) {
    if luma < BOOST_THRESHOLD {
      return Some(
        encoder.get_boosted_q(BOOST_THRESHOLD - luma)
      );
    }
  }

  None
}

pub fn get_avg_luma(chunk: &Chunk) -> anyhow::Result<f32> {
  let source_pipe = if let [source, args @ ..] = &*chunk.source {
    Command::new(source)
      .args(args)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()
      .unwrap()
      .stdout
      .unwrap()
  } else {
    unreachable!();
  };
  
  let mut decoder = y4m::Decoder::new(source_pipe)?;
  let bit_depth = decoder.get_bit_depth();
  assert!(bit_depth == 8, "currently only supports 8-bit input");

  let mut result: Vec<f32> = Vec::new();
  while let Ok(frame) = decoder.read_frame() {
    let luma = frame.get_y_plane();
    let mut sum: usize = 0;
    for b in luma.iter().copied() {
      sum += b as usize;
    }

    result.push(sum as f32 / luma.len() as f32);
  }

  Ok(result.iter().sum::<f32>() / result.len() as f32)
}
