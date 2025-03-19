// crates/av1an-encoders/src/svt_av1.rs
use crate::{
    error::Error,
    traits::{EncoderCapabilities, VideoEncoder},
};
use std::ffi::OsString;

const NULL: &str = if cfg!(windows) { "nul" } else { "/dev/null" };

#[derive(Default)]
pub struct SvtAv1Encoder;

impl VideoEncoder for SvtAv1Encoder {
    fn name(&self) -> &'static str {
        "SvtAv1EncApp"
    }

    fn format(&self) -> &'static str {
        "av1"
    }

    fn output_extension(&self) -> &'static str {
        "ivf"
    }

    fn binary_name(&self) -> &'static str {
        "SvtAv1EncApp"
    }

    fn help_command(&self) -> [&'static str; 2] {
        ["SvtAv1EncApp", "--help"]
    }

    fn default_passes(&self) -> u8 {
        1
    }

    fn get_default_arguments(&self, (cols, rows): (u32, u32)) -> Vec<String> {
        let mut args = vec![
            "--preset".into(),
            "4".into(),
            "--keyint".into(),
            "240".into(),
            "--rc".into(),
            "0".into(),
            "--crf".into(),
            "25".into(),
        ];

        if cols > 1 || rows > 1 {
            let columns = (31 - cols.leading_zeros()) as usize;
            let rows = (31 - rows.leading_zeros()) as usize;
            args.extend_from_slice(&[
                "--tile-columns".into(),
                columns.to_string(),
                "--tile-rows".into(),
                rows.to_string(),
            ]);
        }

        args
    }

    fn compose_1_1_pass(
        &self,
        params: Vec<String>,
        output: String,
        _frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "SvtAv1EncApp".into(),
            "-i".into(),
            "stdin".into(),
            "--progress".into(),
            "2".into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend(["-b".into(), output.into()]);
        cmd
    }

    fn compose_1_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        _frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "SvtAv1EncApp".into(),
            "-i".into(),
            "stdin".into(),
            "--progress".into(),
            "2".into(),
            "--irefresh-type".into(),
            "2".into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--pass".into(),
            "1".into(),
            "--stats".into(),
            format!("{fpf}.stat").into(),
            "-b".into(),
            NULL.into(),
        ]);
        cmd
    }

    fn compose_2_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        output: String,
        _frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd = vec![
            "SvtAv1EncApp".into(),
            "-i".into(),
            "stdin".into(),
            "--progress".into(),
            "2".into(),
            "--irefresh-type".into(),
            "2".into(),
        ];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            "--pass".into(),
            "2".into(),
            "--stats".into(),
            format!("{fpf}.stat").into(),
            "-b".into(),
            output.into(),
        ]);
        cmd
    }

    fn parse_encoded_frames(&self, s: &str) -> Option<u64> {
        const PREFIX: &str = "Encoding frame";
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
            "yuv420p" => Ok(8),
            "yuv420p10le" => Ok(10),
            _ => Err(Error::UnsupportedFormat(
                "SvtAv1EncApp".into(),
                format.into(),
            )),
        }
    }
}

impl EncoderCapabilities for SvtAv1Encoder {
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
        let encoder = SvtAv1Encoder::default();
        assert_eq!(
            encoder.parse_encoded_frames("Encoding frame 141"),
            Some(141)
        );
        assert_eq!(encoder.parse_encoded_frames("invalid"), None);
    }
}
