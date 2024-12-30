use crate::{
    error::Error,
    traits::{EncoderCapabilities, VideoEncoder},
};
use std::ffi::OsString;

const NULL: &str = if cfg!(windows) { "nul" } else { "/dev/null" };

#[derive(Default)]
pub struct Rav1eEncoder;

impl VideoEncoder for Rav1eEncoder {
    fn name(&self) -> &'static str {
        "rav1e"
    }

    fn format(&self) -> &'static str {
        "av1"
    }

    fn output_extension(&self) -> &'static str {
        "ivf"
    }

    fn binary_name(&self) -> &'static str {
        "rav1e"
    }

    fn help_command(&self) -> [&'static str; 2] {
        ["rav1e", "--fullhelp"]
    }

    fn default_passes(&self) -> u8 {
        1
    }

    fn get_default_arguments(&self, (cols, rows): (u32, u32)) -> Vec<String> {
        let mut args = vec![
            "--speed".into(),
            "6".into(),
            "--quantizer".into(),
            "100".into(),
            "--no-scene-detection".into(),
        ];

        if cols > 1 || rows > 1 {
            args.extend_from_slice(&[
                "--tiles".into(),
                format!("{}", cols * rows),
            ]);
        }

        args
    }

    fn compose_1_1_pass(
        &self,
        params: Vec<String>,
        output: String,
        frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "rav1e".into(),
            "-".into(),
            "-y".into(),
            "--limit".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend(["--output".into(), output.into()]);
        cmd
    }

    fn compose_1_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "rav1e".into(),
            "-".into(),
            "-y".into(),
            "--quiet".into(),
            "--limit".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--first-pass".into(),
            format!("{fpf}.stat").into(),
            "--output".into(),
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
            "rav1e".into(),
            "-".into(),
            "-y".into(),
            "--quiet".into(),
            "--limit".into(),
            frame_count.to_string().into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--second-pass".into(),
            format!("{fpf}.stat").into(),
            "--output".into(),
            output.into(),
        ]);
        cmd
    }

    fn parse_encoded_frames(&self, s: &str) -> Option<u64> {
        const PREFIX: &str = "encoded ";
        if !s.starts_with(PREFIX) {
            return None;
        }

        let after_prefix = s.get(PREFIX.len()..)?;

        let nums: String = after_prefix
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '/')
            .collect();

        nums.split('/').next()?.parse().ok()
    }

    fn get_format_bit_depth(&self, format: &str) -> Result<usize, Error> {
        match format {
            "yuv420p" | "yuvj420p" | "yuv422p" | "yuvj422p" | "yuv444p"
            | "yuvj444p" => Ok(8),
            "yuv420p10le" | "yuv422p10le" | "yuv444p10le" => Ok(10),
            "yuv420p12le" | "yuv422p12le" | "yuv444p12le" => Ok(12),
            _ => Err(Error::UnsupportedFormat("rav1e".into(), format.into())),
        }
    }
}

impl EncoderCapabilities for Rav1eEncoder {
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
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_encoded_frames() {
        let encoder = Rav1eEncoder::default();
        assert_eq!(
            encoder.parse_encoded_frames("encoded 141 frames, 126.416 fps"),
            Some(141)
        );
        assert_eq!(
            encoder.parse_encoded_frames("encoded 12/240 frames"),
            Some(12)
        );
        assert_eq!(encoder.parse_encoded_frames("invalid"), None);
    }
}
