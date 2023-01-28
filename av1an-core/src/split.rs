use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::string::ToString;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::scenes::Scene;

pub fn segment(input: impl AsRef<Path>, temp: impl AsRef<Path>, segments: &[usize]) {
  let input = input.as_ref();
  let temp = temp.as_ref();
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  cmd.args(["-hide_banner", "-y", "-i"]);
  cmd.arg(input);
  cmd.args([
    "-map",
    "0:V:0",
    "-an",
    "-c",
    "copy",
    "-avoid_negative_ts",
    "1",
    "-vsync",
    "0",
  ]);

  if segments.is_empty() {
    let split_path = Path::new(temp).join("split").join("0.mkv");
    let split_str = split_path.to_str().unwrap();
    cmd.arg(split_str);
  } else {
    let segments_to_string = segments
      .iter()
      .map(ToString::to_string)
      .collect::<Vec<String>>();
    let segments_joined = segments_to_string.join(",");

    cmd.args(["-f", "segment", "-segment_frames", &segments_joined]);
    let split_path = Path::new(temp).join("split").join("%05d.mkv");
    cmd.arg(split_path);
  }
  let out = cmd.output().unwrap();
  assert!(out.status.success(), "FFmpeg failed to segment: {out:#?}");
}

pub fn extra_splits(scenes: &[Scene], total_frames: usize, split_size: usize) -> Vec<Scene> {
  let mut new_scenes: Vec<Scene> = Vec::with_capacity(scenes.len());

  if let Some(scene) = scenes.last() {
    assert!(
      scene.end_frame <= total_frames,
      "scenecut reported at index {}, but there are only {} frames",
      scene.end_frame,
      total_frames
    );
  }

  for scene in scenes.iter() {
    let distance = scene.end_frame - scene.start_frame;
    let split_size = scene
      .zone_overrides
      .as_ref()
      .map_or(split_size, |ovr| ovr.extra_splits_len.unwrap_or(usize::MAX));
    if distance > split_size {
      let additional_splits = (distance / split_size) + 1;
      for n in 1..additional_splits {
        let new_split =
          (distance as f64 * (n as f64 / additional_splits as f64)) as usize + scene.start_frame;
        new_scenes.push(Scene {
          start_frame: new_scenes
            .last()
            .map_or(scene.start_frame, |scene| scene.end_frame),
          end_frame: new_split,
          ..scene.clone()
        });
      }
    }
    new_scenes.push(Scene {
      start_frame: new_scenes
        .last()
        .map_or(scene.start_frame, |scene| scene.end_frame),
      end_frame: scene.end_frame,
      ..scene.clone()
    });
  }

  new_scenes
}

#[derive(Deserialize, Serialize, Debug)]
struct ScenesData {
  scenes: Vec<Scene>,
  frames: usize,
}

pub fn write_scenes_to_file(
  scenes: &[Scene],
  total_frames: usize,
  scene_path: impl AsRef<Path>,
) -> std::io::Result<()> {
  // Writes a list of scenes and frame count to the file
  let data = ScenesData {
    scenes: scenes.to_vec(),
    frames: total_frames,
  };

  // serializing the data should never fail, so unwrap is OK
  let serialized = serde_json::to_string(&data).unwrap();

  let mut file = File::create(scene_path)?;

  file.write_all(serialized.as_bytes())?;

  Ok(())
}

pub fn read_scenes_from_file(scene_path: &Path) -> anyhow::Result<(Vec<Scene>, usize)> {
  let file = File::open(scene_path)?;

  let reader = BufReader::new(file);

  let data: ScenesData = serde_json::from_reader(reader).with_context(|| {
    format!(
      "Failed to parse scenes file {scene_path:?}, this likely means that the scenes file is corrupted"
    )
  })?;

  Ok((data.scenes, data.frames))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::encoder::Encoder;
  use crate::into_vec;
  use crate::scenes::ZoneOptions;

  #[test]
  fn test_extra_split_no_segments() {
    let total_frames = 300;
    let split_size = 240;
    let done = extra_splits(
      &[Scene {
        start_frame: 0,
        end_frame: 300,
        zone_overrides: None,
      }],
      total_frames,
      split_size,
    );
    let expected_split_locations = vec![0usize, 150];

    assert_eq!(
      expected_split_locations,
      done
        .into_iter()
        .map(|done| done.start_frame)
        .collect::<Vec<usize>>()
    );
  }

  #[test]
  fn test_extra_split_segments() {
    let total_frames = 2000;
    let split_size = 130;
    let done = extra_splits(
      &[
        Scene {
          start_frame: 0,
          end_frame: 150,
          zone_overrides: None,
        },
        Scene {
          start_frame: 150,
          end_frame: 460,
          zone_overrides: None,
        },
        Scene {
          start_frame: 460,
          end_frame: 728,
          zone_overrides: None,
        },
        Scene {
          start_frame: 728,
          end_frame: 822,
          zone_overrides: None,
        },
        Scene {
          start_frame: 822,
          end_frame: 876,
          zone_overrides: None,
        },
        Scene {
          start_frame: 876,
          end_frame: 890,
          zone_overrides: None,
        },
        Scene {
          start_frame: 890,
          end_frame: 1100,
          zone_overrides: None,
        },
        Scene {
          start_frame: 1100,
          end_frame: 1399,
          zone_overrides: None,
        },
        Scene {
          start_frame: 1399,
          end_frame: 1709,
          zone_overrides: None,
        },
        Scene {
          start_frame: 1709,
          end_frame: 2000,
          zone_overrides: None,
        },
      ],
      total_frames,
      split_size,
    );
    let expected_split_locations = [
      0usize, 75, 150, 253, 356, 460, 549, 638, 728, 822, 876, 890, 995, 1100, 1199, 1299, 1399,
      1502, 1605, 1709, 1806, 1903,
    ];

    assert_eq!(
      expected_split_locations,
      done
        .into_iter()
        .map(|done| done.start_frame)
        .collect::<Vec<usize>>()
        .as_slice()
    );
  }

  #[test]
  fn test_extra_split_preserves_zone_overrides() {
    let total_frames = 2000;
    let split_size = 130;
    let done = extra_splits(
      &[
        Scene {
          start_frame: 0,
          end_frame: 150,
          zone_overrides: None,
        },
        Scene {
          start_frame: 150,
          end_frame: 460,
          zone_overrides: None,
        },
        Scene {
          start_frame: 460,
          end_frame: 728,
          zone_overrides: Some(ZoneOptions {
            encoder: Encoder::rav1e,
            passes: 1,
            extra_splits_len: Some(50),
            min_scene_len: 12,
            photon_noise: None,
            video_params: into_vec!["--speed", "8"],
          }),
        },
        Scene {
          start_frame: 728,
          end_frame: 822,
          zone_overrides: None,
        },
        Scene {
          start_frame: 822,
          end_frame: 876,
          zone_overrides: None,
        },
        Scene {
          start_frame: 876,
          end_frame: 890,
          zone_overrides: None,
        },
        Scene {
          start_frame: 890,
          end_frame: 1100,
          zone_overrides: None,
        },
        Scene {
          start_frame: 1100,
          end_frame: 1399,
          zone_overrides: None,
        },
        Scene {
          start_frame: 1399,
          end_frame: 1709,
          zone_overrides: Some(ZoneOptions {
            encoder: Encoder::rav1e,
            passes: 1,
            extra_splits_len: Some(split_size),
            min_scene_len: 12,
            photon_noise: None,
            video_params: into_vec!["--speed", "3"],
          }),
        },
        Scene {
          start_frame: 1709,
          end_frame: 2000,
          zone_overrides: None,
        },
      ],
      total_frames,
      split_size,
    );
    let expected_split_locations = [
      0, 75, 150, 253, 356, 460, 504, 549, 594, 638, 683, 728, 822, 876, 890, 995, 1100, 1199,
      1299, 1399, 1502, 1605, 1709, 1806, 1903,
    ];

    for (i, scene) in done.into_iter().enumerate() {
      assert_eq!(scene.start_frame, expected_split_locations[i]);
      match scene.start_frame {
        460..=727 => {
          assert!(scene.zone_overrides.is_some());
          let overrides = scene.zone_overrides.unwrap();
          assert_eq!(
            overrides.video_params,
            vec!["--speed".to_owned(), "8".to_owned()]
          );
        }
        1399..=1708 => {
          assert!(scene.zone_overrides.is_some());
          let overrides = scene.zone_overrides.unwrap();
          assert_eq!(
            overrides.video_params,
            vec!["--speed".to_owned(), "3".to_owned()]
          );
        }
        _ => {
          assert!(scene.zone_overrides.is_none());
        }
      }
    }
  }
}
