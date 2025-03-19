// crates/av1an-encoders/src/x265.rs
use crate::{
    error::Error,
    traits::{EncoderCapabilities, VideoEncoder},
};
use std::ffi::OsString;

const NULL: &str = if cfg!(windows) { "nul" } else { "/dev/null" };

#[derive(Default)]
pub struct X265Encoder;

impl VideoEncoder for X265Encoder {
    fn name(&self) -> &'static str {
        "x265"
    }

    fn format(&self) -> &'static str {
        "h265"
    }

    fn output_extension(&self) -> &'static str {
        "mkv"
    }

    fn binary_name(&self) -> &'static str {
        "x265"
    }

    fn help_command(&self) -> [&'static str; 2] {
        ["x265", "--fullhelp"]
    }

    fn default_passes(&self) -> u8 {
        1
    }

    fn get_default_arguments(&self, _dims: (u32, u32)) -> Vec<String> {
        vec![
            "-p".into(),
            "slow".into(),
            "--crf".into(),
            "25".into(),
            "-D".into(),
            "10".into(),
            "--level-idc".into(),
            "5.0".into(),
        ]
    }

    fn compose_1_1_pass(
        &self,
        params: Vec<String>,
        output: String,
        frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "x265".into(),
            "--y4m".into(),
            "--frames".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend(["--input".into(), "-".into(), "-o".into(), output.into()]);
        cmd
    }

    fn compose_1_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "x265".into(),
            "--repeat-headers".into(),
            "--log-level".into(),
            "error".into(),
            "--pass".into(),
            "1".into(),
            "--y4m".into(),
            "--frames".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--stats".into(),
            format!("{fpf}.log").into(),
            "--analysis-reuse-file".into(),
            format!("{fpf}_analysis.dat").into(),
            "--input".into(),
            "-".into(),
            "-o".into(),
            NULL.into(),
        ]);
        cmd
    }

    fn compose_2_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        output: String,
        frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "x265".into(),
            "--repeat-headers".into(),
            "--log-level".into(),
            "error".into(),
            "--pass".into(),
            "2".into(),
            "--y4m".into(),
            "--frames".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--stats".into(),
            format!("{fpf}.log").into(),
            "--analysis-reuse-file".into(),
            format!("{fpf}_analysis.dat").into(),
            "--input".into(),
            "-".into(),
            "-o".into(),
            output.into(),
        ]);
        cmd
    }

    fn parse_encoded_frames(&self, s: &str) -> Option<u64> {
        s.split_whitespace()
            .find(|part| !part.starts_with('['))
            .map(|val| val.split('/').next().unwrap_or(val))
            .and_then(|s| s.parse().ok())
    }

    fn get_format_bit_depth(&self, format: &str) -> Result<usize, Error> {
        match format {
            "yuv420p" | "yuvj420p" | "yuv422p" | "yuvj422p" | "yuv444p"
            | "yuvj444p" | "gbrp" | "gray8" => Ok(8),
            "yuv420p10le" | "yuv422p10le" | "yuv444p10le" | "gbrp10le"
            | "gray10le" => Ok(10),
            "yuv420p12le" | "yuv422p12le" | "yuv444p12le" | "gbrp12le"
            | "gray12le" => Ok(12),
            _ => Err(Error::UnsupportedFormat("x265".into(), format.into())),
        }
    }
}

impl EncoderCapabilities for X265Encoder {
    fn supports_two_pass(&self) -> bool {
        true
    }

    fn supports_constant_quality(&self) -> bool {
        true
    }

    fn supports_bitrate(&self) -> bool {
        true
    }

    fn supports_tile_parallel(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_encoded_frames() {
        let encoder = X265Encoder::default();
        assert_eq!(
            encoder.parse_encoded_frames("[23.4%] 141/240 frames"),
            Some(141)
        );
        assert_eq!(encoder.parse_encoded_frames("invalid"), None);
    }
}
