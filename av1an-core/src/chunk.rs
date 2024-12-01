use std::ffi::OsString;
use std::path::Path;

use av1_grain::{generate_photon_noise_params, write_grain_table, NoiseGenArgs};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::encoder::Encoder;
use crate::settings::insert_noise_table_params;
use crate::Input;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
  pub temp: String,
  pub index: usize,
  pub input: Input,
  pub source_cmd: Vec<OsString>,
  pub output_ext: String,
  pub start_frame: usize,
  // End frame is exclusive, i.e. the range of frames is `start_frame..end_frame`
  pub end_frame: usize,
  pub frame_rate: f64,
  pub passes: u8,
  pub video_params: Vec<String>,
  pub encoder: Encoder,
  pub noise_size: (Option<u32>, Option<u32>),
  // do not break compatibility with output produced by older versions of av1an
  /// Optional target quality CQ level
  #[serde(rename = "per_shot_target_quality_cq")]
  pub tq_cq: Option<u32>,
  pub ignore_frame_mismatch: bool,
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

  pub const fn frames(&self) -> usize {
    self.end_frame - self.start_frame
  }

  pub(crate) fn apply_photon_noise_args(
    &mut self,
    photon_noise: Option<u8>,
    chroma_noise: bool,
  ) -> anyhow::Result<()> {
    if let Some(strength) = photon_noise {
      let iso_setting = u32::from(strength) * 100;
      let grain_table = Path::new(&self.temp).join(format!("iso{iso_setting}-grain.tbl"));
      if !grain_table.exists() {
        debug!("Generating grain table at ISO {}", iso_setting);
        let (mut width, mut height) = self.input.resolution()?;
        if self.noise_size.0.is_some() {
          width = self.noise_size.0.unwrap();
        }
        if self.noise_size.1.is_some() {
          height = self.noise_size.1.unwrap();
        }
        let transfer_function = self
          .input
          .transfer_function_params_adjusted(&self.video_params)?;
        let params = generate_photon_noise_params(
          0,
          u64::MAX,
          NoiseGenArgs {
            iso_setting,
            width,
            height,
            transfer_function,
            chroma_grain: chroma_noise,
            random_seed: None,
          },
        );
        write_grain_table(&grain_table, &[params])?;
      }

      insert_noise_table_params(self.encoder, &mut self.video_params, &grain_table);
    }

    Ok(())
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
      input: Input::Video {
        path: "test.mkv".into(),
      },
      source_cmd: vec!["".into()],
      output_ext: "ivf".to_owned(),
      start_frame: 0,
      end_frame: 5,
      frame_rate: 30.0,
      tq_cq: None,
      passes: 1,
      video_params: vec![],
      encoder: Encoder::x264,
      noise_size: (None, None),
      ignore_frame_mismatch: false,
    };
    assert_eq!("00001", ch.name());
  }
  #[test]
  fn test_chunk_name_10000() {
    let ch = Chunk {
      temp: "none".to_owned(),
      index: 10000,
      input: Input::Video {
        path: "test.mkv".into(),
      },
      source_cmd: vec!["".into()],
      output_ext: "ivf".to_owned(),
      start_frame: 0,
      end_frame: 5,
      frame_rate: 30.0,
      tq_cq: None,
      passes: 1,
      video_params: vec![],
      encoder: Encoder::x264,
      noise_size: (None, None),
      ignore_frame_mismatch: false,
    };
    assert_eq!("10000", ch.name());
  }

  #[test]
  fn test_chunk_output() {
    let ch = Chunk {
      temp: "d".to_owned(),
      index: 1,
      input: Input::Video {
        path: "test.mkv".into(),
      },
      source_cmd: vec!["".into()],
      output_ext: "ivf".to_owned(),
      start_frame: 0,
      end_frame: 5,
      frame_rate: 30.0,
      tq_cq: None,
      passes: 1,
      video_params: vec![],
      encoder: Encoder::x264,
      noise_size: (None, None),
      ignore_frame_mismatch: false,
    };
    assert_eq!("d/encode/00001.ivf", ch.output());
  }
}
