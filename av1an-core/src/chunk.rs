use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct Chunk {
  pub temp: String,
  pub index: usize,
  pub ffmpeg_gen_cmd: Vec<String>,
  pub output_ext: String,
  pub size: usize,
  pub frames: usize,
  pub per_shot_target_quality_cq: Option<u32>,
}

impl Chunk {
  pub fn new(
    temp: String,
    index: usize,
    ffmpeg_gen_cmd: Vec<String>,
    output_ext: String,
    size: usize,
    frames: usize,
    per_shot_target_quality_cq: Option<u32>,
  ) -> Result<Self, anyhow::Error> {
    Ok(Self {
      temp,
      index,
      ffmpeg_gen_cmd,
      output_ext,
      size,
      frames,
      per_shot_target_quality_cq,
    })
  }

  /// Returns numeric name of chunk `00001`
  pub fn name(&self) -> String {
    format!("{:05}", self.index)
  }

  pub fn output(&self) -> String {
    self.output_path()
  }

  pub fn output_path(&self) -> String {
    Path::new(&self.temp)
      .join("encode")
      .join(format!("{}.{}", self.name(), self.output_ext))
      .to_str()
      .unwrap()
      .to_owned()
  }
}

#[cfg(test)]
mod tests {

  use super::*;

  #[test]
  fn test_chunk_name_1() {
    let ch = Chunk {
      temp: "none".to_owned(),
      index: 1,
      ffmpeg_gen_cmd: vec!["".to_owned()],
      output_ext: "ivf".to_owned(),
      size: 2,
      frames: 5,
      per_shot_target_quality_cq: None,
    };
    assert_eq!("00001", ch.name());
  }
  #[test]
  fn test_chunk_name_10000() {
    let ch = Chunk {
      temp: "none".to_owned(),
      index: 10000,
      ffmpeg_gen_cmd: vec!["".to_owned()],
      output_ext: "ivf".to_owned(),
      size: 2,
      frames: 5,
      per_shot_target_quality_cq: None,
    };
    assert_eq!("10000", ch.name());
  }

  #[test]
  fn test_chunk_output() {
    let ch = Chunk {
      temp: "d".to_owned(),
      index: 1,
      ffmpeg_gen_cmd: vec!["".to_owned()],
      output_ext: "ivf".to_owned(),
      size: 2,
      frames: 5,
      per_shot_target_quality_cq: None,
    };
    assert_eq!("d/encode/00001.ivf", ch.output_path());
  }
}
