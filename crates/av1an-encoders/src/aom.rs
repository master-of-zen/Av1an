use crate::{
    error::Error,
    traits::{EncoderCapabilities, VideoEncoder},
};
use std::ffi::OsString;

const NULL: &str = if cfg!(windows) { "nul" } else { "/dev/null" };

#[derive(Default)]
pub struct AomEncoder;

impl VideoEncoder for AomEncoder {
    fn name(&self) -> &'static str {
        "aomenc"
    }

    fn format(&self) -> &'static str {
        "av1"
    }

    fn output_extension(&self) -> &'static str {
        "ivf"
    }

    fn binary_name(&self) -> &'static str {
        "aomenc"
    }

    fn help_command(&self) -> [&'static str; 2] {
        ["aomenc", "--help"]
    }

    fn default_passes(&self) -> u8 {
        2
    }

    fn get_default_arguments(&self, (cols, rows): (u32, u32)) -> Vec<String> {
        let mut args = vec![
            "--threads=8".into(),
            "--cpu-used=6".into(),
            "--end-usage=q".into(),
            "--cq-level=30".into(),
        ];

        if cols > 1 || rows > 1 {
            let columns = (31 - cols.leading_zeros()) as usize;
            let rows = (31 - rows.leading_zeros()) as usize;
            args.extend_from_slice(&[
                format!("--tile-columns={columns}"),
                format!("--tile-rows={rows}"),
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
        let mut cmd = vec!["aomenc".into(), "--passes=1".into()];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend(["-o".into(), output.into(), "-".into()]);
        cmd
    }

    fn compose_1_2_pass(
        &self,
        params: Vec<String>,
        fpf: &str,
        _frame_count: usize,
    ) -> Vec<OsString> {
        let mut cmd =
            vec!["aomenc".into(), "--passes=2".into(), "--pass=1".into()];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            format!("--fpf={fpf}.log").into(),
            "-o".into(),
            NULL.into(),
            "-".into(),
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
        let mut cmd =
            vec!["aomenc".into(), "--passes=2".into(), "--pass=2".into()];
        cmd.extend(params.into_iter().map(Into::into));
        cmd.extend([
            format!("--fpf={fpf}.log").into(),
            "-o".into(),
            output.into(),
            "-".into(),
        ]);
        cmd
    }

    fn parse_encoded_frames(&self, s: &str) -> Option<u64> {
        const PREFIX: &str = "Pass x/x frame    x/";
        if !s.starts_with(PREFIX) {
            return None;
        }

        let after_prefix = s.get(PREFIX.len()..)?;
        let first_digit_pos = after_prefix.find('/')?;
        let first_space_pos =
            after_prefix[first_digit_pos..].find(' ')? + first_digit_pos;

        after_prefix
            .get(first_digit_pos + 1..first_space_pos)?
            .parse()
            .ok()
    }

    fn get_format_bit_depth(&self, format: &str) -> Result<usize, Error> {
        match format {
            "yuv420p" | "yuv422p" | "yuv444p" | "gbrp" | "gray8" => Ok(8),
            "yuv420p10le" | "yuv422p10le" | "yuv444p10le" | "gbrp10le"
            | "gray10le" => Ok(10),
            "yuv420p12le" | "yuv422p12le" | "yuv444p12le" | "gbrp12le"
            | "gray12le" => Ok(12),
            _ => Err(Error::UnsupportedFormat("aomenc".into(), format.into())),
        }
    }
}

impl EncoderCapabilities for AomEncoder {
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
        let encoder = AomEncoder::default();
        assert_eq!(
            encoder.parse_encoded_frames(
                "Pass 1/2 frame  142/141   156465B  208875 us 679.83 fps"
            ),
            Some(141)
        );
        assert_eq!(encoder.parse_encoded_frames("invalid"), None);
    }
}
