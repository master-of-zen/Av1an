use anyhow::Context;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};


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

pub fn save_chunk_queue(temp: &str, chunk_queue: &[Chunk]) -> anyhow::Result<()> {
  let mut file = File::create(Path::new(temp).join("chunks.json"))
    .with_context(|| "Failed to create chunks.json file")?;

  file
    // serializing chunk_queue as json should never fail, so unwrap is OK here
    .write_all(serde_json::to_string(&chunk_queue).unwrap().as_bytes())
    .with_context(|| format!("Failed to write serialized chunk_queue data to {:?}", &file))?;

  Ok(())
}

