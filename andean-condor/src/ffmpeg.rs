use std::str::FromStr;

use anyhow::bail;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vapoursynth::format::PresetFormat;

/// Pixel formats supported by ffmpeg
#[derive(Eq, PartialEq, Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum FFPixelFormat {
    GBRP,
    GBRP10LE,
    GBRP12L,
    GBRP12LE,
    GRAY10LE,
    GRAY12L,
    GRAY12LE,
    GRAY8,
    NV12,
    NV16,
    NV20LE,
    NV21,
    YUV420P,
    YUV420P10LE,
    YUV420P12LE,
    YUV422P,
    YUV422P10LE,
    YUV422P12LE,
    YUV440P,
    YUV440P10LE,
    YUV440P12LE,
    YUV444P,
    YUV444P10LE,
    YUV444P12LE,
    YUVA420P,
    YUVJ420P,
    YUVJ422P,
    YUVJ444P,
}

impl FFPixelFormat {
    /// The string to be used with ffmpeg's `-pix_fmt` argument.
    #[inline]
    pub fn to_pix_fmt_string(&self) -> &'static str {
        match self {
            FFPixelFormat::GBRP => "gbrp",
            FFPixelFormat::GBRP10LE => "gbrp10le",
            FFPixelFormat::GBRP12L => "gbrp12l",
            FFPixelFormat::GBRP12LE => "gbrp12le",
            FFPixelFormat::GRAY10LE => "gray10le",
            FFPixelFormat::GRAY12L => "gray12l",
            FFPixelFormat::GRAY12LE => "gray12le",
            FFPixelFormat::GRAY8 => "gray",
            FFPixelFormat::NV12 => "nv12",
            FFPixelFormat::NV16 => "nv16",
            FFPixelFormat::NV20LE => "nv20le",
            FFPixelFormat::NV21 => "nv21",
            FFPixelFormat::YUV420P => "yuv420p",
            FFPixelFormat::YUV420P10LE => "yuv420p10le",
            FFPixelFormat::YUV420P12LE => "yuv420p12le",
            FFPixelFormat::YUV422P => "yuv422p",
            FFPixelFormat::YUV422P10LE => "yuv422p10le",
            FFPixelFormat::YUV422P12LE => "yuv422p12le",
            FFPixelFormat::YUV440P => "yuv440p",
            FFPixelFormat::YUV440P10LE => "yuv440p10le",
            FFPixelFormat::YUV440P12LE => "yuv440p12le",
            FFPixelFormat::YUV444P => "yuv444p",
            FFPixelFormat::YUV444P10LE => "yuv444p10le",
            FFPixelFormat::YUV444P12LE => "yuv444p12le",
            FFPixelFormat::YUVA420P => "yuva420p",
            FFPixelFormat::YUVJ420P => "yuvj420p",
            FFPixelFormat::YUVJ422P => "yuvj422p",
            FFPixelFormat::YUVJ444P => "yuvj444p",
        }
    }

    #[inline]
    pub fn to_vapoursynth_format(&self) -> anyhow::Result<PresetFormat> {
        Ok(match self {
            // Vapoursynth doesn't have a Gray10/Gray12 so use the next best thing.
            // No quality loss from 10/12->16 but might be slightly slower.
            FFPixelFormat::GRAY10LE => PresetFormat::Gray16,
            FFPixelFormat::GRAY12L => PresetFormat::Gray16,
            FFPixelFormat::GRAY12LE => PresetFormat::Gray16,
            FFPixelFormat::GRAY8 => PresetFormat::Gray8,
            FFPixelFormat::YUV420P => PresetFormat::YUV420P8,
            FFPixelFormat::YUV420P10LE => PresetFormat::YUV420P10,
            FFPixelFormat::YUV420P12LE => PresetFormat::YUV420P12,
            FFPixelFormat::YUV422P => PresetFormat::YUV422P8,
            FFPixelFormat::YUV422P10LE => PresetFormat::YUV422P10,
            FFPixelFormat::YUV422P12LE => PresetFormat::YUV422P12,
            FFPixelFormat::YUV440P => PresetFormat::YUV440P8,
            FFPixelFormat::YUV444P => PresetFormat::YUV444P8,
            FFPixelFormat::YUV444P10LE => PresetFormat::YUV444P10,
            FFPixelFormat::YUV444P12LE => PresetFormat::YUV444P12,
            FFPixelFormat::YUVJ420P => PresetFormat::YUV420P8,
            FFPixelFormat::YUVJ422P => PresetFormat::YUV422P8,
            FFPixelFormat::YUVJ444P => PresetFormat::YUV444P8,
            x => bail!(
                "pixel format {} cannot be converted to Vapoursynth format",
                x.to_pix_fmt_string()
            ),
        })
    }
}

impl FromStr for FFPixelFormat {
    type Err = anyhow::Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "gbrp" => FFPixelFormat::GBRP,
            "gbrp10le" => FFPixelFormat::GBRP10LE,
            "gbrp12l" => FFPixelFormat::GBRP12L,
            "gbrp12le" => FFPixelFormat::GBRP12LE,
            "gray10le" => FFPixelFormat::GRAY10LE,
            "gray12l" => FFPixelFormat::GRAY12L,
            "gray12le" => FFPixelFormat::GRAY12LE,
            "gray" => FFPixelFormat::GRAY8,
            "nv12" => FFPixelFormat::NV12,
            "nv16" => FFPixelFormat::NV16,
            "nv20le" => FFPixelFormat::NV20LE,
            "nv21" => FFPixelFormat::NV21,
            "yuv420p" => FFPixelFormat::YUV420P,
            "yuv420p10le" => FFPixelFormat::YUV420P10LE,
            "yuv420p12le" => FFPixelFormat::YUV420P12LE,
            "yuv422p" => FFPixelFormat::YUV422P,
            "yuv422p10le" => FFPixelFormat::YUV422P10LE,
            "yuv422p12le" => FFPixelFormat::YUV422P12LE,
            "yuv440p" => FFPixelFormat::YUV440P,
            "yuv440p10le" => FFPixelFormat::YUV440P10LE,
            "yuv440p12le" => FFPixelFormat::YUV440P12LE,
            "yuv444p" => FFPixelFormat::YUV444P,
            "yuv444p10le" => FFPixelFormat::YUV444P10LE,
            "yuv444p12le" => FFPixelFormat::YUV444P12LE,
            "yuva420p" => FFPixelFormat::YUVA420P,
            "yuvj420p" => FFPixelFormat::YUVJ420P,
            "yuvj422p" => FFPixelFormat::YUVJ422P,
            "yuvj444p" => FFPixelFormat::YUVJ444P,
            s => bail!("Unsupported pixel format string: {s}"),
        })
    }
}
