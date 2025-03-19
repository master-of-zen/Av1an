// crates/av1an-core/src/encoder.rs
use std::{fmt::Display, iter::Iterator};

use arrayvec::ArrayVec;
use cfg_if::cfg_if;
use ffmpeg::format::Pixel;
use itertools::chain;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{into_array, into_vec};

const NULL: &str = if cfg!(windows) { "nul" } else { "/dev/null" };

#[allow(non_camel_case_types)]
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Debug,
    strum::EnumString,
    strum::IntoStaticStr,
)]
pub enum Encoder {
    aom,
    rav1e,
    #[strum(serialize = "svt-av1")]
    svt_av1,
    x264,
    x265,
}

#[tracing::instrument]
pub(crate) fn parse_svt_av1_version(version: &[u8]) -> Option<(u32, u32, u32)> {
    let v_idx = memchr::memchr(b'v', version)?;
    let s = version.get(v_idx + 1..)?;
    let s = simdutf8::basic::from_utf8(s).ok()?;
    let version = s
        .split_ascii_whitespace()
        .next()?
        .split('.')
        .filter_map(|s| s.split('-').next())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect::<ArrayVec<u32, 3>>();

    if let [major, minor, patch] = version[..] {
        Some((major, minor, patch))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {}

impl Display for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(<&'static str>::from(self))
    }
}

impl Encoder {
    pub fn compose_1_1_pass(
        self,
        params: Vec<String>,
        output: String,
        frame_count: usize,
    ) -> Vec<String> {
        match self {
            Self::aom => chain!(
                into_array!["aomenc", "--passes=1"],
                params,
                into_array!["-o", output, "-"],
            )
            .collect(),
            Self::rav1e => chain!(
                into_array![
                    "rav1e",
                    "-",
                    "-y",
                    "--limit",
                    frame_count.to_string()
                ],
                params,
                into_array!["--output", output]
            )
            .collect(),

            Self::svt_av1 => chain!(
                into_array!["SvtAv1EncApp", "-i", "stdin", "--progress", "2"],
                params,
                into_array!["-b", output],
            )
            .collect(),
            Self::x264 => chain!(
                into_array![
                    "x264",
                    "--stitchable",
                    "--log-level",
                    "error",
                    "--demuxer",
                    "y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array!["-", "-o", output]
            )
            .collect(),
            Self::x265 => chain!(
                into_array![
                    "x265",
                    "--y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array!["--input", "-", "-o", output]
            )
            .collect(),
        }
    }

    /// Composes 1st pass command for 2 pass encoding
    pub fn compose_1_2_pass(
        self,
        params: Vec<String>,
        fpf: &str,
        frame_count: usize,
    ) -> Vec<String> {
        match self {
            Self::aom => chain!(
                into_array!["aomenc", "--passes=2", "--pass=1"],
                params,
                into_array![format!("--fpf={fpf}.log"), "-o", NULL, "-"],
            )
            .collect(),
            Self::rav1e => chain!(
                into_array![
                    "rav1e",
                    "-",
                    "-y",
                    "--quiet",
                    "--limit",
                    frame_count.to_string()
                ],
                params,
                into_array![
                    "--first-pass",
                    format!("{fpf}.stat"),
                    "--output",
                    NULL
                ]
            )
            .collect(),
            Self::svt_av1 => chain!(
                into_array![
                    "SvtAv1EncApp",
                    "-i",
                    "stdin",
                    "--progress",
                    "2",
                    "--irefresh-type",
                    "2",
                ],
                params,
                into_array![
                    "--pass",
                    "1",
                    "--stats",
                    format!("{fpf}.stat"),
                    "-b",
                    NULL,
                ],
            )
            .collect(),
            Self::x264 => chain!(
                into_array![
                    "x264",
                    "--stitchable",
                    "--log-level",
                    "error",
                    "--pass",
                    "1",
                    "--demuxer",
                    "y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array!["--stats", format!("{fpf}.log"), "-", "-o", NULL]
            )
            .collect(),
            Self::x265 => chain!(
                into_array![
                    "x265",
                    "--repeat-headers",
                    "--log-level",
                    "error",
                    "--pass",
                    "1",
                    "--y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array![
                    "--stats",
                    format!("{fpf}.log"),
                    "--analysis-reuse-file",
                    format!("{fpf}_analysis.dat"),
                    "--input",
                    "-",
                    "-o",
                    NULL
                ]
            )
            .collect(),
        }
    }

    /// Composes 2st pass command for 2 pass encoding
    pub fn compose_2_2_pass(
        self,
        params: Vec<String>,
        fpf: &str,
        output: String,
        frame_count: usize,
    ) -> Vec<String> {
        match self {
            Self::aom => chain!(
                into_array!["aomenc", "--passes=2", "--pass=2"],
                params,
                into_array![format!("--fpf={fpf}.log"), "-o", output, "-"],
            )
            .collect(),
            Self::rav1e => chain!(
                into_array![
                    "rav1e",
                    "-",
                    "-y",
                    "--quiet",
                    "--limit",
                    frame_count.to_string()
                ],
                params,
                into_array![
                    "--second-pass",
                    format!("{fpf}.stat"),
                    "--output",
                    output
                ]
            )
            .collect(),
            Self::svt_av1 => chain!(
                into_array![
                    "SvtAv1EncApp",
                    "-i",
                    "stdin",
                    "--progress",
                    "2",
                    "--irefresh-type",
                    "2",
                ],
                params,
                into_array![
                    "--pass",
                    "2",
                    "--stats",
                    format!("{fpf}.stat"),
                    "-b",
                    output,
                ],
            )
            .collect(),
            Self::x264 => chain!(
                into_array![
                    "x264",
                    "--stitchable",
                    "--log-level",
                    "error",
                    "--pass",
                    "2",
                    "--demuxer",
                    "y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array!["--stats", format!("{fpf}.log"), "-", "-o", output]
            )
            .collect(),
            Self::x265 => chain!(
                into_array![
                    "x265",
                    "--repeat-headers",
                    "--log-level",
                    "error",
                    "--pass",
                    "2",
                    "--y4m",
                    "--frames",
                    frame_count.to_string()
                ],
                params,
                into_array![
                    "--stats",
                    format!("{fpf}.log"),
                    "--analysis-reuse-file",
                    format!("{fpf}_analysis.dat"),
                    "--input",
                    "-",
                    "-o",
                    output
                ]
            )
            .collect(),
        }
    }

    /// Returns default settings for the encoder
    pub fn get_default_arguments(
        self,
        (cols, rows): (u32, u32),
    ) -> Vec<String> {
        /// Integer log base 2
        pub const fn ilog2(x: u32) -> u32 {
            // TODO: switch to built-in integer log2 functions once they are
            // stabilized https://github.com/rust-lang/rust/issues/70887
            if x == 0 {
                0
            } else {
                u32::BITS - 1 - x.leading_zeros()
            }
        }

        match self {
            // aomenc automatically infers the correct bit depth, and thus for
            // aomenc, not specifying the bit depth is actually more
            // accurate because if for example you specify
            // `--pix-format yuv420p`, aomenc will encode 10-bit when that is
            // not actually the desired pixel format.
            Encoder::aom => {
                let defaults: Vec<String> = into_vec![
                    "--threads=8",
                    "--cpu-used=6",
                    "--end-usage=q",
                    "--cq-level=30",
                ];

                if cols > 1 || rows > 1 {
                    let columns = ilog2(cols);
                    let rows = ilog2(rows);

                    let aom_tiles: Vec<String> = into_vec![
                        format!("--tile-columns={columns}"),
                        format!("--tile-rows={rows}")
                    ];
                    chain!(defaults, aom_tiles).collect()
                } else {
                    defaults
                }
            },
            Encoder::rav1e => {
                let defaults: Vec<String> = into_vec![
                    "--speed",
                    "6",
                    "--quantizer",
                    "100",
                    "--no-scene-detection"
                ];

                if cols > 1 || rows > 1 {
                    let tiles: Vec<String> =
                        into_vec!["--tiles", format!("{}", cols * rows)];
                    chain!(defaults, tiles).collect()
                } else {
                    defaults
                }
            },
            Encoder::svt_av1 => {
                let defaults = into_vec![
                    "--preset", "4", "--keyint", "240", "--rc", "0", "--crf",
                    "25"
                ];
                if cols > 1 || rows > 1 {
                    let columns = ilog2(cols);
                    let rows = ilog2(rows);

                    let tiles: Vec<String> = into_vec![
                        "--tile-columns",
                        columns.to_string(),
                        "--tile-rows",
                        rows.to_string()
                    ];
                    chain!(defaults, tiles).collect()
                } else {
                    defaults
                }
            },
            Encoder::x264 => into_vec!["--preset", "slow", "--crf", "25"],
            Encoder::x265 => into_vec![
                "-p",
                "slow",
                "--crf",
                "25",
                "-D",
                "10",
                "--level-idc",
                "5.0"
            ],
        }
    }

    /// Return number of default passes for encoder
    pub const fn get_default_pass(self) -> u8 {
        match self {
            Self::aom => 2,
            _ => 1,
        }
    }

    /// Returns help command for encoder
    pub const fn help_command(self) -> [&'static str; 2] {
        match self {
            Self::aom => ["aomenc", "--help"],
            Self::rav1e => ["rav1e", "--fullhelp"],
            Self::svt_av1 => ["SvtAv1EncApp", "--help"],
            Self::x264 => ["x264", "--fullhelp"],
            Self::x265 => ["x265", "--fullhelp"],
        }
    }

    /// Get the name of the executable/binary for the encoder
    pub const fn bin(self) -> &'static str {
        match self {
            Self::aom => "aomenc",
            Self::rav1e => "rav1e",
            Self::svt_av1 => "SvtAv1EncApp",
            Self::x264 => "x264",
            Self::x265 => "x265",
        }
    }

    /// Get the name of the video format associated with the encoder
    pub const fn format(self) -> &'static str {
        match self {
            Self::aom | Self::rav1e | Self::svt_av1 => "av1",
            Self::x264 => "h264",
            Self::x265 => "h265",
        }
    }

    /// Get the default output extension for the encoder
    pub const fn output_extension(&self) -> &'static str {
        match &self {
            Self::aom | Self::rav1e | Self::svt_av1 => "ivf",
            Self::x264 | Self::x265 => "mkv",
        }
    }

    /// Parses the number of encoded frames
    pub(crate) fn parse_encoded_frames(self, line: &str) -> Option<u64> {
        use crate::parse::*;

        match self {
            Self::aom => {
                cfg_if! {
                  if #[cfg(any(target_arch = "x86", target_arch = "x86_64"))] {
                    if is_x86_feature_detected!("sse4.1") && is_x86_feature_detected!("ssse3") {
                      return unsafe { parse_aom_frames_sse41(line.as_bytes()) };
                    }
                  }
                }

                parse_aom_frames(line)
            },
            Self::rav1e => parse_rav1e_frames(line),
            Self::svt_av1 => parse_svt_av1_frames(line),
            Self::x264 | Self::x265 => parse_x26x_frames(line),
        }
    }

    pub fn remove_patterns(args: &mut Vec<String>, patterns: &[&str]) {
        for pattern in patterns {
            if let Some(index) = args
                .iter()
                .position(|value| value.contains(pattern))
            {
                args.remove(index);
                // If pattern does not contain =, we need to remove the index
                // that follows.
                if !pattern.contains('=') {
                    args.remove(index);
                }
            }
        }
    }

    pub fn get_format_bit_depth(
        self,
        format: Pixel,
    ) -> Result<usize, UnsupportedPixelFormatError> {
        macro_rules! impl_this_function {
      ($($encoder:ident),*) => {
        match self {
          $(
            Encoder::$encoder => paste::paste! { [<get_ $encoder _format_bit_depth>](format) },
          )*
        }
      };
    }
        impl_this_function!(x264, x265, aom, rav1e, svt_av1)
    }
}

#[derive(Error, Debug)]
pub enum UnsupportedPixelFormatError {
    #[error("{0} does not support {1:?}")]
    UnsupportedFormat(Encoder, Pixel),
}

macro_rules! create_get_format_bit_depth_function {
  ($encoder:ident, 8: $_8bit_fmts:expr, 10: $_10bit_fmts:expr, 12: $_12bit_fmts:expr) => {
    paste::paste! {
      pub fn [<get_ $encoder _format_bit_depth>](format: Pixel) -> Result<usize, UnsupportedPixelFormatError> {
        use Pixel::*;
        if $_8bit_fmts.contains(&format) {
          Ok(8)
        } else if $_10bit_fmts.contains(&format) {
          Ok(10)
        } else if $_12bit_fmts.contains(&format) {
          Ok(12)
        } else {
          Err(UnsupportedPixelFormatError::UnsupportedFormat(Encoder::$encoder, format))
        }
      }
    }
  };
}

// The supported bit depths are taken from ffmpeg,
// e.g.: `ffmpeg -h encoder=libx264`
create_get_format_bit_depth_function!(
  x264,
   8: [YUV420P, YUVJ420P, YUV422P, YUVJ422P, YUV444P, YUVJ444P, NV12, NV16, NV21, GRAY8],
  10: [YUV420P10LE, YUV422P10LE, YUV444P10LE, NV20LE, GRAY10LE],
  12: []
);
create_get_format_bit_depth_function!(
  x265,
   8: [YUV420P, YUVJ420P, YUV422P, YUVJ422P, YUV444P, YUVJ444P, GBRP, GRAY8],
  10: [YUV420P10LE, YUV422P10LE, YUV444P10LE, GBRP10LE, GRAY10LE],
  12: [YUV420P12LE, YUV422P12LE, YUV444P12LE, GBRP12LE, GRAY12LE]
);
create_get_format_bit_depth_function!(
  aom,
   8: [YUV420P, YUV422P, YUV444P, GBRP, GRAY8],
  10: [YUV420P10LE, YUV422P10LE, YUV444P10LE, GBRP10LE, GRAY10LE],
  12: [YUV420P12LE, YUV422P12LE, YUV444P12LE, GBRP12LE, GRAY12LE,]
);
create_get_format_bit_depth_function!(
  rav1e,
   8: [YUV420P, YUVJ420P, YUV422P, YUVJ422P, YUV444P, YUVJ444P],
  10: [YUV420P10LE, YUV422P10LE, YUV444P10LE],
  12: [YUV420P12LE, YUV422P12LE, YUV444P12LE,]
);
create_get_format_bit_depth_function!(
  svt_av1,
   8: [YUV420P],
  10: [YUV420P10LE],
  12: []
);
