use crate::{into_vec, progress_bar, Input, ScenecutMethod, Verbosity};
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};

use std::process::{Command, Stdio};

pub fn av_scenechange_detect(
  input: &Input,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
) -> anyhow::Result<Vec<usize>> {
  if verbosity != Verbosity::Quiet {
    println!("Scene detection");
    progress_bar::init_progress_bar(total_frames as u64);
  }

  let mut frames = scene_detect(
    input,
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

  progress_bar::finish_progress_bar();

  if frames[0] == 0 {
    // TODO refactor the chunk creation to not require this
    // Currently, this is required for compatibility with create_video_queue_vs
    frames.remove(0);
  }

  Ok(frames)
}

/// Detect scene changes using rav1e scene detector.
pub fn scene_detect(
  input: &Input,
  callback: Option<Box<dyn Fn(usize, usize)>>,
  min_scene_len: usize,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
) -> anyhow::Result<Vec<usize>> {
  let filters: Vec<String> = if let Some(downscale_height) = sc_downscale_height {
    into_vec![
      "-pix_fmt",
      "yuv420p",
      "-vf",
      format!("scale=-2:'min({},ih)'", downscale_height)
    ]
  } else {
    into_vec!["-pix_fmt", "yuv420p"]
  };

  Ok(
    detect_scene_changes::<_, u8>(
      &mut y4m::Decoder::new(match input {
        Input::VapourSynth(path) => {
          let vspipe = Command::new("vspipe")
            .arg("-y")
            .arg(path)
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
            .stdout
            .unwrap();

          // TODO: do not convert to yuv420p if the source is already yuv420p
          Command::new("ffmpeg")
            .stdin(vspipe)
            .args(["-i", "pipe:", "-f", "yuv4mpegpipe"])
            .args(filters)
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
            .stdout
            .unwrap()
        }
        Input::Video(path) => Command::new("ffmpeg")
          .arg("-i")
          .arg(path)
          .args(filters)
          .args(["-f", "yuv4mpegpipe", "-"])
          .stdout(Stdio::piped())
          .stderr(Stdio::null())
          .spawn()?
          .stdout
          .unwrap(),
      })?,
      DetectionOptions {
        ignore_flashes: true,
        min_scenecut_distance: Some(min_scene_len),
        fast_analysis: match sc_method {
          ScenecutMethod::Fast => SceneDetectionSpeed::Fast,
          ScenecutMethod::Medium => SceneDetectionSpeed::Medium,
          ScenecutMethod::Slow => SceneDetectionSpeed::Slow,
        },
        ..DetectionOptions::default()
      },
      callback,
    )
    .scene_changes,
  )
}
