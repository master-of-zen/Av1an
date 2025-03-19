// crates/av1an-encoders/src/traits.rs
use std::ffi::OsString;

use crate::error::Error;

pub trait VideoEncoder {
    fn name(&self) -> &'static str;
    fn format(&self) -> &'static str;
    fn output_extension(&self) -> &'static str;
    fn binary_name(&self) -> &'static str;
    fn help_command(&self) -> [&'static str; 2];

    fn default_passes(&self) -> u8;
    fn get_default_arguments(&self, dims: (u32, u32)) -> Vec<String>;

    fn compose_1_1_pass(
        &self,
        params: Vec<String>,
        output: String,
        frame_count: usize,
    ) -> Vec<OsString>;
    fn compose_1_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        frame_count: usize,
    ) -> Vec<OsString>;
    fn compose_2_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        output: String,
        frame_count: usize,
    ) -> Vec<OsString>;

    fn parse_encoded_frames(&self, line: &str) -> Option<u64>;
    fn get_format_bit_depth(&self, format: &str) -> Result<usize, Error>;
}

pub trait EncoderCapabilities {
    fn supports_two_pass(&self) -> bool;
    fn supports_constant_quality(&self) -> bool;
    fn supports_bitrate(&self) -> bool;
    fn supports_tile_parallel(&self) -> bool;
}
