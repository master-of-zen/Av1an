use std::{
    borrow::{Borrow, Cow},
    cmp::Ordering,
    collections::HashSet,
    path::PathBuf,
    process::{exit, Command},
};

use anyhow::{bail, ensure};
use av1an_output::ConcatMethod;
use ffmpeg::format::Pixel;
use serde::{Deserialize, Serialize};

use crate::{
    encoder::Encoder,
    parse::valid_params,
    vapoursynth::{
        is_bestsource_installed,
        is_dgdecnv_installed,
        is_ffms2_installed,
        is_lsmash_installed,
    },
    Input,
    ScenecutMethod,
    SplitMethod,
    TaskMethod,
    TaskOrdering,
    Verbosity,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PixelFormat {
    pub format:    Pixel,
    pub bit_depth: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InputPixelFormat {
    VapourSynth { bit_depth: usize },
    FFmpeg { format: Pixel },
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct EncodeArgs {
    pub input:       Input,
    pub temp:        String,
    pub output_file: String,

    pub task_method:           TaskMethod,
    pub task_order:            TaskOrdering,
    pub scenes:                Option<PathBuf>,
    pub split_method:          SplitMethod,
    pub sc_pix_format:         Option<Pixel>,
    pub sc_method:             ScenecutMethod,
    pub sc_only:               bool,
    pub sc_downscale_height:   Option<usize>,
    pub extra_splits_len:      Option<usize>,
    pub min_scene_len:         usize,
    pub force_keyframes:       Vec<usize>,
    pub ignore_frame_mismatch: bool,

    pub max_tries: usize,

    pub passes:              u8,
    pub video_params:        Vec<String>,
    pub encoder:             Encoder,
    pub workers:             usize,
    pub set_thread_affinity: Option<usize>,

    // FFmpeg params
    pub ffmpeg_filter_args: Vec<String>,
    pub audio_params:       Vec<String>,
    pub input_pix_format:   InputPixelFormat,
    pub output_pix_format:  PixelFormat,

    pub verbosity: Verbosity,
    pub log_file:  PathBuf,
    pub resume:    bool,
    pub keep:      bool,
    pub force:     bool,

    pub concat: ConcatMethod,
}

impl EncodeArgs {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        if self.concat == ConcatMethod::Ivf
            && !matches!(
                self.encoder,
                Encoder::rav1e | Encoder::aom | Encoder::svt_av1
            )
        {
            bail!(".ivf only supports VP8, VP9, and AV1");
        }

        ensure!(self.max_tries > 0);

        ensure!(
            self.input.as_path().exists(),
            "Input file {:?} does not exist!",
            self.input
        );

        if which::which("ffmpeg").is_err() {
            bail!("FFmpeg not found. Is it installed in system path?");
        }

        if self.concat == ConcatMethod::MKVMerge
            && which::which("mkvmerge").is_err()
        {
            bail!(
                "mkvmerge not found, but `--concat mkvmerge` was specified. \
                 Is it installed in system path?"
            );
        }

        if self.encoder == Encoder::x265
            && self.concat != ConcatMethod::MKVMerge
        {
            bail!(
                "mkvmerge is required for concatenating x265, as x265 outputs \
                 raw HEVC bitstream files without the timestamps correctly \
                 set, which FFmpeg cannot concatenate properly into a mkv \
                 file. Specify mkvmerge as the concatenation method by \
                 setting `--concat mkvmerge`."
            );
        }

        if self.task_method == TaskMethod::LSMASH {
            ensure!(
                is_lsmash_installed(),
                "LSMASH is not installed, but it was specified as the task \
                 method"
            );
        }
        if self.task_method == TaskMethod::FFMS2 {
            ensure!(
                is_ffms2_installed(),
                "FFMS2 is not installed, but it was specified as the task \
                 method"
            );
        }
        if self.task_method == TaskMethod::DGDECNV
            && which::which("dgindexnv").is_err()
        {
            ensure!(
                is_dgdecnv_installed(),
                "Either DGDecNV is not installed or DGIndexNV is not in \
                 system path, but it was specified as the task method"
            );
        }
        if self.task_method == TaskMethod::BESTSOURCE {
            ensure!(
                is_bestsource_installed(),
                "BestSource is not installed, but it was specified as the \
                 task method"
            );
        }
        if self.task_method == TaskMethod::Select {
            warn!(
                "It is not recommended to use the \"select\" task method, as \
                 it is very slow"
            );
        }

        if self.ignore_frame_mismatch {
            warn!(
                "The output video's frame count may differ, and VMAF \
                 calculations may be incorrect"
            );
        }

        let encoder_bin = self.encoder.bin();
        if which::which(encoder_bin).is_err() {
            bail!(
                "Encoder {} not found. Is it installed in the system path?",
                encoder_bin
            );
        }

        if self.video_params.is_empty() {
            self.video_params = self
                .encoder
                .get_default_arguments(self.input.calculate_tiles());
        }

        if self.encoder == Encoder::aom
            && self.concat != ConcatMethod::MKVMerge
            && self
                .video_params
                .iter()
                .any(|param| param == "--enable-keyframe-filtering=2")
        {
            bail!(
                "keyframe filtering mode 2 currently only works when using \
                 mkvmerge as the concat method"
            );
        }

        if matches!(self.encoder, Encoder::aom)
            && self.passes != 1
            && self
                .video_params
                .iter()
                .any(|param| param == "--rt")
        {
            // --rt must be used with 1-pass mode
            self.passes = 1;
        }

        if !self.force {
            self.validate_encoder_params();
            self.check_rate_control();
        }

        Ok(())
    }

    fn validate_encoder_params(&self) {
        let video_params: Vec<&str> = self
            .video_params
            .iter()
            .filter_map(|param| {
                if param.starts_with('-')
                    && [Encoder::aom].contains(&self.encoder)
                {
                    // These encoders require args to be passed using an equal
                    // sign, e.g. `--cq-level=30`
                    param.split('=').next()
                } else {
                    // The other encoders use a space, so we don't need to do
                    // extra splitting, e.g. `--crf 30`
                    None
                }
            })
            .collect();

        let help_text = {
            let [cmd, arg] = self.encoder.help_command();
            String::from_utf8(
                Command::new(cmd)
                    .arg(arg)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
        };
        let valid_params = valid_params(&help_text, self.encoder);
        let invalid_params = invalid_params(&video_params, &valid_params);

        for wrong_param in &invalid_params {
            eprintln!(
                "'{}' isn't a valid parameter for {}",
                wrong_param, self.encoder,
            );
            if let Some(suggestion) = suggest_fix(wrong_param, &valid_params) {
                eprintln!("\tDid you mean '{suggestion}'?");
            }
        }

        if !invalid_params.is_empty() {
            println!("\nTo continue anyway, run av1an with '--force'");
            exit(1);
        }
    }

    /// Warns if rate control was not specified in encoder arguments
    fn check_rate_control(&self) {
        if self.encoder == Encoder::aom {
            if !self
                .video_params
                .iter()
                .any(|f| Self::check_aom_encoder_mode(f))
            {
                warn!("[WARN] --end-usage was not specified");
            }

            if !self
                .video_params
                .iter()
                .any(|f| Self::check_aom_rate(f))
            {
                warn!(
                    "[WARN] --cq-level or --target-bitrate was not specified"
                );
            }
        }
    }

    fn check_aom_encoder_mode(s: &str) -> bool {
        const END_USAGE: &str = "--end-usage=";
        if s.len() <= END_USAGE.len() || !s.starts_with(END_USAGE) {
            return false;
        }

        s.as_bytes()[END_USAGE.len()..]
            .iter()
            .all(|&b| (b as char).is_ascii_alphabetic())
    }

    fn check_aom_rate(s: &str) -> bool {
        const CQ_LEVEL: &str = "--cq-level=";
        const TARGET_BITRATE: &str = "--target-bitrate=";

        if s.len() <= CQ_LEVEL.len()
            || !(s.starts_with(TARGET_BITRATE) || s.starts_with(CQ_LEVEL))
        {
            return false;
        }

        if s.starts_with(CQ_LEVEL) {
            s.as_bytes()[CQ_LEVEL.len()..]
                .iter()
                .all(|&b| (b as char).is_ascii_digit())
        } else {
            s.as_bytes()[TARGET_BITRATE.len()..]
                .iter()
                .all(|&b| (b as char).is_ascii_digit())
        }
    }
}

#[must_use]
pub(crate) fn invalid_params<'a>(
    params: &'a [&'a str],
    valid_options: &'a HashSet<Cow<'a, str>>,
) -> Vec<&'a str> {
    params
        .iter()
        .filter(|param| {
            !valid_options.contains(Borrow::<str>::borrow(&**param))
        })
        .copied()
        .collect()
}

#[must_use]
pub(crate) fn suggest_fix<'a>(
    wrong_arg: &str,
    arg_dictionary: &'a HashSet<Cow<'a, str>>,
) -> Option<&'a str> {
    // Minimum threshold to consider a suggestion similar enough that it could
    // be a typo
    const MIN_THRESHOLD: f64 = 0.75;

    arg_dictionary
        .iter()
        .map(|arg| (arg, strsim::jaro_winkler(arg, wrong_arg)))
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Less))
        .and_then(|(suggestion, score)| {
            if score > MIN_THRESHOLD {
                Some(suggestion.borrow())
            } else {
                None
            }
        })
}
