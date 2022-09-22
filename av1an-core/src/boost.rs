use std::{io::Read, process::{Stdio, Command}};

use crate::{Input, scenes::Scene, encoder::Encoder, chunk::Chunk};

pub trait Pixel {}

pub fn analyze<T: Pixel>(input: &Input, encoder: Encoder, scene: Scene, chunk: Chunk) -> anyhow::Result<Vec<f32>> {
  let mut decoder = build_decoder(input, encoder)?;
  let bit_depth = decoder.get_bit_depth();
  assert!(bit_depth == 8, "currently only supports 8-bit input");

  

  let frames = scene.end_frame - scene.start_frame;
  let mut result: Vec<f32> = Vec::with_capacity(frames);
  while let Ok(frame) = decoder.read_frame() {
    let luma = frame.get_y_plane();
    let mut sum: u32 = 0;
    for b in luma.iter().copied() {
      sum += b as u32;
    }
    
    result.push(sum as f32 / luma.len() as f32);
  }

  Ok(result)
}

fn analyze_8bit() {

}

fn analyze_hbd() {

}

fn get_average_luma(scene: Scene) {

}

fn build_decoder(input: &Input, encoder: Encoder) -> anyhow::Result<y4m::Decoder<impl Read>> {
  let decoder = match input {
    Input::VapourSynth(path) => {
      let vspipe = Command::new("vspipe")
        .arg("-c")
        .arg("y4m")
        .arg(path)
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?
        .stdout
        .unwrap();
  
      y4m::Decoder::new(vspipe)?
    },
    Input::Video(path) => {
      let ffpipe = Command::new("ffmpeg")
        .args(["-r", "1", "-i"])
        .arg(path)
        //.args(filters.as_ref())
        .args(["-f", "yuv4mpegpipe", "-strict", "-1", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?
        .stdout
        .unwrap();
      
      y4m::Decoder::new(ffpipe)?
    },
  };

  Ok(decoder)
}
