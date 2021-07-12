use av_scenechange::{detect_scene_changes, DetectionOptions};
use std::{
  path::Path,
  process::{Command, Stdio},
};

/// Detect scene changes using rav1e scene detector.
///
/// src: Input to video.
pub fn scene_detect(
  src: &Path,
  callback: Option<Box<dyn Fn(usize, usize)>>,
  min_scene_len: usize,
  is_vs: bool,
) -> anyhow::Result<Vec<usize>> {
  Ok(
    detect_scene_changes::<_, u8>(
      &mut y4m::Decoder::new(if is_vs {
        {
          // vspipe -y .b8a0d8d/split/loadscript.vpy - | ffmpeg -i pipe: -f yuv4mpegpipe -pix_fmt yuv420p -vf scale=360:-2 - | mpv -
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
            .args(&[
              "-i",
              "pipe:",
              "-f",
              "yuv4mpegpipe",
              "-pix_fmt",
              "yuv420p",
              "-vf",
              "scale=360:-2",
              "-",
            ])
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
          .args(&[
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "scale=360:-2",
            "-f",
            "yuv4mpegpipe",
            "-",
          ])
          .stdout(Stdio::piped())
          .stderr(Stdio::null())
          .spawn()?
          .stdout
          .unwrap()
      })?,
      DetectionOptions {
        ignore_flashes: true,
        min_scenecut_distance: Some(min_scene_len),
        ..Default::default()
      },
      callback,
    )
    .scene_changes,
  )
}
