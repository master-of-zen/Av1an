use serde::{Deserialize, Serialize};
use std::{
  error,
  fs::File,
  io::prelude::*,
  io::BufReader,
  path::Path,
  process::{Command, Stdio},
  string::ToString,
};

pub fn segment(input: impl AsRef<Path>, temp: impl AsRef<Path>, segments: &[usize]) {
  let input = input.as_ref();
  let temp = temp.as_ref();
  let mut cmd = Command::new("ffmpeg");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  cmd.args(&[
    "-hide_banner",
    "-y",
    "-i",
    input.to_str().unwrap(),
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

    cmd.args(&["-f", "segment", "-segment_frames", &segments_joined]);
    let split_path = Path::new(temp).join("split").join("%05d.mkv");
    let split_str = split_path.to_str().unwrap();
    cmd.arg(split_str);
  }
  let out = cmd.output().unwrap();
  assert!(out.status.success());
}

pub fn extra_splits(
  split_locations: Vec<usize>,
  total_frames: usize,
  split_size: usize,
) -> Vec<usize> {
  let mut result_vec: Vec<usize> = split_locations.clone();

  let mut total_length = split_locations;
  total_length.insert(0, 0);
  total_length.push(total_frames);

  let iter = total_length[..total_length.len() - 1]
    .iter()
    .zip(total_length[1..].iter());

  for (x, y) in iter {
    let distance = y - x;
    if distance > split_size {
      let additional_splits = (distance / split_size) + 1;
      for n in 1..additional_splits {
        let new_split = (distance as f64 * (n as f64 / additional_splits as f64)) as usize + x;
        result_vec.push(new_split);
      }
    }
  }

  result_vec.sort_unstable();

  result_vec
}

#[derive(Deserialize, Serialize, Debug)]
struct ScenesData {
  scenes: Vec<usize>,
  frames: usize,
}

pub fn write_scenes_to_file(
  scenes: &[usize],
  total_frames: usize,
  scene_path: impl AsRef<Path>,
) -> std::io::Result<()> {
  // Writes a list of scenes and frame count to the file
  let data = ScenesData {
    scenes: scenes.to_vec(),
    frames: total_frames,
  };

  let serialized = serde_json::to_string(&data).unwrap();

  let mut file = File::create(scene_path)?;

  file.write_all(serialized.as_bytes())?;

  Ok(())
}

pub fn read_scenes_from_file(
  scene_path: &Path,
) -> Result<(Vec<usize>, usize), Box<dyn error::Error>> {
  let file = File::open(scene_path)?;

  let reader = BufReader::new(file);

  let data: ScenesData = serde_json::from_reader(reader)?;

  Ok((data.scenes, data.frames))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_extra_split_no_segments() {
    let total_frames = 300;
    let split_size = 240;
    let done = extra_splits(vec![], total_frames, split_size);
    let expected_split_locations = vec![150];

    assert_eq!(expected_split_locations, done);
  }

  #[test]
  fn test_extra_split_segments() {
    let total_frames = 2000;
    let split_size = 130;
    let done = extra_splits(
      vec![150, 460, 728, 822, 876, 890, 1100, 1399, 1709],
      total_frames,
      split_size,
    );
    let expected_split_locations: Vec<usize> = vec![
      75, 150, 253, 356, 460, 549, 638, 728, 822, 876, 890, 995, 1100, 1199, 1299, 1399, 1502,
      1605, 1709, 1806, 1903,
    ];

    assert_eq!(expected_split_locations, done);
  }
}
