use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::Path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chunk {
  pub temp: String,
  pub index: usize,
  pub source: Vec<OsString>,
  pub output_ext: String,
  pub frames: usize,
  // do not break compatibility with output produced by older versions of av1an
  /// Optional target quality CQ level
  #[serde(rename = "per_shot_target_quality_cq")]
  pub tq_cq: Option<u32>,
}

impl Chunk {
  pub fn new(
    temp: String,
    index: usize,
    source: Vec<OsString>,
    output_ext: &'static str,
    frames: usize,
    per_shot_target_quality_cq: Option<u32>,
  ) -> Result<Self, anyhow::Error> {
    Ok(Self {
      temp,
      index,
      source,
      output_ext: output_ext.to_owned(),
      frames,
      tq_cq: per_shot_target_quality_cq,
    })
  }

  /// Returns numeric name of chunk `00001`
  pub fn name(&self) -> String {
    format!("{:05}", self.index)
  }

  pub fn output(&self) -> String {
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
      source: vec!["".into()],
      output_ext: "ivf".to_owned(),
      frames: 5,
      tq_cq: None,
    };
    assert_eq!("00001", ch.name());
  }
  #[test]
  fn test_chunk_name_10000() {
    let ch = Chunk {
      temp: "none".to_owned(),
      index: 10000,
      source: vec!["".into()],
      output_ext: "ivf".to_owned(),
      frames: 5,
      tq_cq: None,
    };
    assert_eq!("10000", ch.name());
  }

  #[test]
  fn test_chunk_output() {
    let ch = Chunk {
      temp: "d".to_owned(),
      index: 1,
      source: vec!["".into()],
      output_ext: "ivf".to_owned(),
      frames: 5,
      tq_cq: None,
    };
    assert_eq!("d/encode/00001.ivf", ch.output());
  }
}
