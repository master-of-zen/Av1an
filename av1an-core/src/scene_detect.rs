use crate::{into_vec, progress_bar, ScenecutMethod, Verbosity};
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};

use std::{
  path::Path,
  process::{Command, Stdio},
};

pub fn av_scenechange_detect(
  input: &str,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  is_vs: bool,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
) -> anyhow::Result<Vec<usize>> {
  if verbosity != Verbosity::Quiet {
    println!("Scene detection");
    progress_bar::init_progress_bar(total_frames as u64);
  }

  let mut frames = crate::scene_detect::scene_detect(
    Path::new(input),
    if verbosity == Verbosity::Quiet {
      None
    } else {
      Some(Box::new(|frames, _keyframes| {
        progress_bar::set_pos(frames as u64);
      }))
    },
    min_scene_len,
    is_vs,
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
///
/// src: Input to video.
pub fn scene_detect(
  src: &Path,
  callback: Option<Box<dyn Fn(usize, usize)>>,
  min_scene_len: usize,
  is_vs: bool,
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
      &mut y4m::Decoder::new(if is_vs {
        {
          let vspipe = Command::new("vspipe")
            .arg("-y")
            .arg(src)
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
            .stdout
            .unwrap();

          Command::new("ffmpeg")
            .stdin(vspipe)
            .args(&["-i", "pipe:", "-f", "yuv4mpegpipe"])
            .args(filters)
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?
            .stdout
            .unwrap()
        }
      } else {
        Command::new("ffmpeg")
          .arg("-i")
          .arg(src)
          .args(filters)
          .args(&["-f", "yuv4mpegpipe", "-"])
          .stdout(Stdio::piped())
          .stderr(Stdio::null())
          .spawn()?
          .stdout
          .unwrap()
      })?,
      DetectionOptions {
        ignore_flashes: true,
        min_scenecut_distance: Some(min_scene_len),
        fast_analysis: match sc_method {
          ScenecutMethod::Fast => SceneDetectionSpeed::Fast,
          ScenecutMethod::Medium => SceneDetectionSpeed::Medium,
          ScenecutMethod::Slow => SceneDetectionSpeed::Slow,
        },
        ..Default::default()
      },
      callback,
    )
    .scene_changes,
  )
}
