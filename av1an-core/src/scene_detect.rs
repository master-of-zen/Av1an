use crossbeam_utils::thread::Scope;
use ffmpeg_next::format::Pixel;
use smallvec::{smallvec, SmallVec};

use crate::chunk::Chunk;
use crate::settings::EncodeArgs;
use crate::Encoder;
use crate::{ffmpeg, into_smallvec, progress_bar, Input, ScenecutMethod, Verbosity};
use ansi_term::Style;
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

// TODO: reimplement frame count on another thread
pub fn av_scenechange_detect(
  input: &Input,
  encoder: Encoder,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  sc_pix_format: Option<Pixel>,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
  sender: crossbeam_channel::Sender<Chunk>,
  vs_script: PathBuf,
  out_ext: String,
  temp: String,
) -> anyhow::Result<()> {
  let (tx, rx) = mpsc::channel::<usize>();

  let x = input.clone();
  let t = thread::spawn(move || {
    scene_detect(
      &x,
      encoder,
      None,
      min_scene_len,
      sc_pix_format,
      sc_method,
      sc_downscale_height,
      tx,
    )
    .unwrap();
  });

  let mut last = 0;
  let mut idx = 0;
  while let Ok(frame_idx) = rx.recv() {
    // TODO do this without a branch every time?
    if frame_idx != 0 {
      sender
        .send(EncodeArgs::create_vs_chunk(
          idx,
          &vs_script,
          last,
          frame_idx,
          out_ext.clone(),
          temp.clone(),
        ))
        .unwrap();

      idx += 1;
      last = frame_idx;
    }
  }

  t.join().unwrap();

  // TODO: get this from the number of analyzed frames rather than calculating it separately
  let frames = input.frames().unwrap();

  // send the last chunk
  sender
    .send(EncodeArgs::create_vs_chunk(
      idx,
      &vs_script,
      last,
      frames,
      out_ext.clone(),
      temp.clone(),
    ))
    .unwrap();

  Ok(())
}

/// Detect scene changes using rav1e scene detector.
pub fn scene_detect(
  input: &Input,
  encoder: Encoder,
  callback: Option<Box<dyn Fn(usize, usize)>>,
  min_scene_len: usize,
  sc_pix_format: Option<Pixel>,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
  sender: mpsc::Sender<usize>,
) -> anyhow::Result<()> {
  let bit_depth;

  let filters: SmallVec<[String; 4]> = match (sc_downscale_height, sc_pix_format) {
    (Some(sdh), Some(spf)) => into_smallvec![
      "-vf",
      format!(
        "format={},scale=-2:'min({},ih)'",
        spf.descriptor().unwrap().name(),
        sdh
      )
    ],
    (Some(sdh), None) => into_smallvec!["-vf", format!("scale=-2:'min({},ih)'", sdh)],
    (None, Some(spf)) => into_smallvec!["-pix_fmt", spf.descriptor().unwrap().name()],
    (None, None) => smallvec![],
  };

  let decoder = &mut y4m::Decoder::new(match input {
    Input::VapourSynth(path) => {
      bit_depth = crate::vapoursynth::bit_depth(path.as_ref())?;
      let vspipe = Command::new("vspipe")
        .arg("-y")
        .arg(path)
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?
        .stdout
        .unwrap();

      if !filters.is_empty() {
        Command::new("ffmpeg")
          .stdin(vspipe)
          .args(["-i", "pipe:", "-f", "yuv4mpegpipe", "-strict", "-1"])
          .args(filters)
          .arg("-")
          .stdout(Stdio::piped())
          .stderr(Stdio::null())
          .spawn()?
          .stdout
          .unwrap()
      } else {
        vspipe
      }
    }
    Input::Video(path) => {
      let input_pix_format = ffmpeg::get_pixel_format(path.as_ref())
        .unwrap_or_else(|e| panic!("FFmpeg failed to get pixel format for input video: {:?}", e));
      bit_depth = encoder.get_format_bit_depth(sc_pix_format.unwrap_or(input_pix_format))?;
      Command::new("ffmpeg")
        .args(["-r", "1", "-i"])
        .arg(path)
        .args(filters.as_ref())
        .args(["-f", "yuv4mpegpipe", "-strict", "-1", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?
        .stdout
        .unwrap()
    }
  })?;

  let options = DetectionOptions {
    min_scenecut_distance: Some(min_scene_len),
    analysis_speed: match sc_method {
      ScenecutMethod::Fast => SceneDetectionSpeed::Fast,
      ScenecutMethod::Standard => SceneDetectionSpeed::Standard,
    },
    ..DetectionOptions::default()
  };

  if bit_depth > 8 {
    detect_scene_changes::<_, u16>(decoder, options, callback, sender).scene_changes;
  } else {
    detect_scene_changes::<_, u8>(decoder, options, callback, sender).scene_changes;
  }

  Ok(())
}
