// crates/av1an-encoders/src/lib.rs
pub mod error;
pub mod traits;

mod aom;
mod rav1e;
mod svt_av1;
mod x264;
mod x265;

use error::Error;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Aom,
    Rav1e,
    SvtAv1,
    X264,
    X265,
}

impl FromStr for Encoder {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "aom" => Ok(Self::Aom),
            "rav1e" => Ok(Self::Rav1e),
            "svt-av1" => Ok(Self::SvtAv1),
            "x264" => Ok(Self::X264),
            "x265" => Ok(Self::X265),
            _ => Err(Error::EncoderNotFound(s.to_string())),
        }
    }
}

impl Encoder {
    pub fn get_instance(&self) -> Box<dyn crate::traits::VideoEncoder> {
        match self {
            Self::Aom => Box::new(aom::AomEncoder::default()),
            Self::Rav1e => Box::new(rav1e::Rav1eEncoder::default()),
            Self::SvtAv1 => Box::new(svt_av1::SvtAv1Encoder::default()),
            Self::X264 => Box::new(x264::X264Encoder::default()),
            Self::X265 => Box::new(x265::X265Encoder::default()),
        }
    }
}
