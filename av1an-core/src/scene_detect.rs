use crate::{ffmpeg, progress_bar, Input, ScenecutMethod, Verbosity};
use crate::{into_array, Encoder};
use ansi_term::Style;
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};

use std::process::{Command, Stdio};
use std::thread;

pub fn av_scenechange_detect(
  input: &Input,
  encoder: Encoder,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
) -> anyhow::Result<(Vec<usize>, usize)> {
  if verbosity != Verbosity::Quiet {
    eprintln!("{}", Style::default().bold().paint("Scene detection"));
    progress_bar::init_progress_bar(total_frames as u64);
  }

  let input2 = input.clone();
  let frame_thread = thread::spawn(move || {
    let frames = input2.frames().unwrap();
    progress_bar::convert_to_progress();
    progress_bar::set_len(frames as u64);
    frames
  });

  let mut scenecuts = scene_detect(
    input,
    encoder,
    if verbosity == Verbosity::Quiet {
      None
    } else {
      Some(Box::new(|frames, _keyframes| {
        progress_bar::set_pos(frames as u64);
      }))
    },
    min_scene_len,
    sc_method,
    sc_downscale_height,
  )?;

  let frames = frame_thread.join().unwrap();

  progress_bar::finish_progress_bar();

  if scenecuts[0] == 0 {
    // TODO refactor the chunk creation to not require this
    // Currently, this is required for compatibility with create_video_queue_vs
    scenecuts.remove(0);
  }

  Ok((scenecuts, frames))
}

/// Detect scene changes using rav1e scene detector.
pub fn scene_detect(
  input: &Input,
  encoder: Encoder,
  callback: Option<Box<dyn Fn(usize, usize)>>,
  min_scene_len: usize,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
) -> anyhow::Result<Vec<usize>> {
  let bit_depth;

  let filters: Option<[String; 2]> = sc_downscale_height
    .map(|downscale_height| into_array!["-vf", format!("scale=-2:'min({},ih)'", downscale_height)]);

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

      if let Some(filters) = &filters {
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
      bit_depth = encoder.get_format_bit_depth(input_pix_format)?;
      Command::new("ffmpeg")
        .args(["-r", "1", "-i"])
        .arg(path)
        .args(
          filters
            .as_ref()
            .map_or(&[] as &[String], |filters| &filters[..]),
        )
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
  Ok(if bit_depth > 8 {
    detect_scene_changes::<_, u16>(decoder, options, callback).scene_changes
  } else {
    detect_scene_changes::<_, u8>(decoder, options, callback).scene_changes
  })
}
