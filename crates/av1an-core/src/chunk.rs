use std::{ffi::OsString, path::Path};

use serde::{Deserialize, Serialize};

use crate::{encoder::Encoder, Input};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub temp:                  String,
    pub index:                 usize,
    pub input:                 Input,
    pub source_cmd:            Vec<OsString>,
    pub output_ext:            String,
    pub start_frame:           usize,
    // End frame is exclusive, i.e. the range of frames is
    // `start_frame..end_frame`
    pub end_frame:             usize,
    pub frame_rate:            f64,
    pub passes:                u8,
    pub video_params:          Vec<String>,
    pub encoder:               Encoder,
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
}
