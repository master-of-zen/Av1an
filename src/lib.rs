#[macro_use]
extern crate log;
extern crate av_format;
extern crate av_ivf;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use sysinfo::SystemExt;

mod concat;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum Encoder {
    libaom,
    rav1e,
    libvpx,
    SvtAv1,
    SvtVp9,
    x264,
    x265,
}

impl FromStr for Encoder {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // set to match usage in python code
        match s {
            "aom" => Ok(Self::libaom),
            "rav1e" => Ok(Self::rav1e),
            "vpx" => Ok(Self::libvpx),
            "svt_av1" => Ok(Self::SvtAv1),
            "svt_vp9" => Ok(Self::SvtVp9),
            "x264" => Ok(Self::x264),
            "x265" => Ok(Self::x265),
            _ => Err(()),
        }
    }
}

// TODO
pub enum ConcatMethod {
    /// MKVToolNix
    MKVMerge,
    /// FFmpeg
    FFmpeg,
    /// Use native functions implemented in av1an
    Native,
}

pub enum SplitMethod {
    PySceneDetect,
    AOMKeyframes,
    FFmpeg,
}

pub enum ChunkMethod {
    Select,
    FFMS2,
    LSMASH,
    Hybrid,
}

pub struct EncodeConfig {
    frames: usize,
    counter: (),
    is_vs: bool,
    input: PathBuf,
    temp: PathBuf,
    output_file: PathBuf,

    concat_method: ConcatMethod,
    config: (),
    webm: (),
    chunk_method: ChunkMethod,
    scenes: PathBuf,
    split_method: SplitMethod,
    extra_split: usize,
    min_scene_len: usize,
    // PySceneDetect split
    threshold: f32,

    // TODO refactor, this should really be in the enum of each encoder
    reuse_first_pass: bool,

    // Encoding
    passes: (),
    video_params: (),
    encoder: Encoder,
    workers: usize,

    // FFmpeg params
    ffmpeg_pipe: (),
    ffmpeg: (),
    audio_params: (),
    pix_format: (),

    quiet: bool,
    logging: (),
    resume: bool,
    no_check: bool,
    keep: bool,
    force: bool,

    // VMAF
    vmaf: bool,
    vmaf_path: PathBuf,
    vmaf_res: (),

    // TODO refactor into VMAF struct, and this struct contains an Option<VMAF> or something
    // which indicates whether or not the encode uses target_quality/vmaf

    // except for the vmaf options which you can use regardless of target_quality, like the
    // vmaf plot option

    // Target quality
    target_quality: u8,
    probes: u16,
    min_q: u8,
    max_q: u8,
    vmaf_plots: bool,
    probing_rate: u32,
    n_threads: usize,
    vmaf_filter: (),
}

/// Sanity check for FFmpeg
#[pyfunction]
fn get_ffmpeg_info() -> String {
    let mut cmd = Command::new("ffmpeg");
    cmd.stderr(Stdio::piped());
    String::from_utf8(cmd.output().unwrap().stderr).unwrap()
}

#[pyfunction]
// TODO take type Encoder as argument eventually
fn determine_workers(encoder: &str) -> PyResult<u64> {
    let encoder = Encoder::from_str(encoder)
        // TODO remove boilerplate somehow
        .map_err(|_| {
            pyo3::exceptions::PyTypeError::new_err(format!("Unsupported encoder: {:?}", encoder))
        })?;

    // let system = sysinfo::System::new_with_specifics(sysinfo::RefreshKind::new());
    let system = sysinfo::System::new_all();

    let cpu = num_cpus::get() as u64;
    // get_total_memory returns kb, convert to bytes
    let ram = system.get_total_memory() * 1000 / 2u64.pow(30);

    Ok(std::cmp::max(
        match encoder {
            Encoder::libaom | Encoder::rav1e | Encoder::libvpx => {
                // converts to f64 to match behavior of python equivalent
                std::cmp::min(
                    (cpu as f64 / 3.0).round() as u64,
                    (ram as f64 / 1.5).round() as u64,
                )
            }
            Encoder::SvtAv1 | Encoder::SvtVp9 | Encoder::x264 | Encoder::x265 => {
                std::cmp::min(cpu, ram) / 8
            }
        },
        1,
    ))
}

/// A Python module implemented in Rust.
#[pymodule]
fn av1an(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_ffmpeg_info, m)?)?;
    m.add_function(wrap_pyfunction!(determine_workers, m)?)?;

    Ok(())
}
