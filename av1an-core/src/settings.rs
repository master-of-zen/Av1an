use std::{
    borrow::{Borrow, Cow},
    cmp::Ordering,
    collections::HashSet,
    fmt::Display,
    path::{absolute, Path, PathBuf},
    process::{exit, Command},
};

use anyhow::{bail, ensure};
use itertools::{chain, Itertools};
use path_abs::PathInfo;
use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr};
use tracing::warn;

use crate::{
    concat::ConcatMethod,
    encoder::Encoder,
    ffmpeg::FFPixelFormat,
    metrics::{vmaf::validate_libvmaf, xpsnr::validate_libxpsnr},
    parse::valid_params,
    target_quality::TargetQuality,
    vapoursynth::{CacheSource, VSZipVersion, VapoursynthPlugins},
    ChunkMethod,
    ChunkOrdering,
    Input,
    ScenecutMethod,
    SplitMethod,
    TargetMetric,
    Verbosity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct InterceptedEncoderLogArgs {
    params:         Vec<String>,
    text_log_file:  Option<String>,
    x265_csv_file:  Option<String>,
    merge_text_log: bool,
}

fn parse_log_level_value(param: &str, next_param: Option<&String>) -> Option<(String, bool)> {
    // Only `--log-level` may be parsed here: every other parameter that happens
    // to use the `--option=value` syntax would otherwise be mistaken for a log
    // level and silently disable the log merge.
    if let Some(value) = param.strip_prefix("--log-level=") {
        return Some((value.to_ascii_lowercase(), false));
    }

    if param == "--log-level" {
        return next_param.map(|value| (value.to_ascii_lowercase(), true));
    }

    None
}

fn parse_path_value(
    flag: &str,
    param: &str,
    next_param: Option<&String>,
) -> Option<(String, bool)> {
    if let Some(value) = param.strip_prefix(&format!("{flag}=")) {
        return Some((value.to_string(), false));
    }

    if param == flag {
        return next_param.cloned().map(|value| (value, true));
    }

    None
}

fn intercept_encoder_log_args(params: &[String], encoder: Encoder) -> InterceptedEncoderLogArgs {
    let mut cleaned = Vec::with_capacity(params.len());
    let mut skip_next = false;
    let mut text_log_file = None;
    let mut x265_csv_file = None;
    let mut log_level = None;

    for (i, param) in params.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        if let Some((path, consumes_next)) =
            parse_path_value("--log-file", param, params.get(i + 1))
        {
            text_log_file = Some(path);
            skip_next = consumes_next;
            continue;
        }

        if encoder == Encoder::x265 {
            if let Some((path, consumes_next)) = parse_path_value("--csv", param, params.get(i + 1))
            {
                x265_csv_file = Some(path);
                skip_next = consumes_next;
                continue;
            }

            if param == "--csv-log-level" {
                skip_next = params.get(i + 1).is_some();
                continue;
            }

            if param.starts_with("--csv-log-level=") {
                continue;
            }
        }

        if let Some((level, consumes_next)) = parse_log_level_value(param, params.get(i + 1)) {
            log_level = Some(level);
            skip_next = consumes_next;
            cleaned.push(param.clone());
            if consumes_next && let Some(next_param) = params.get(i + 1) {
                cleaned.push(next_param.clone());
            }
            continue;
        }

        cleaned.push(param.clone());
    }

    let merge_text_log = if x265_csv_file.is_some() {
        false
    } else if let Some(level) = log_level.as_deref() {
        matches!(level, "info" | "debug" | "full")
    } else {
        text_log_file.is_some()
    };

    InterceptedEncoderLogArgs {
        params: cleaned,
        text_log_file,
        x265_csv_file,
        merge_text_log,
    }
}

#[derive(EnumString, IntoStaticStr, Debug, PartialEq, Clone, Copy)]
pub enum PixelFormatConverter {
    #[strum(serialize = "ffmpeg")]
    FFMPEG,
    #[strum(serialize = "vs-resize")]
    VsResize,
}

impl Display for PixelFormatConverter {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(<&'static str>::from(self))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PixelFormat {
    pub format:    FFPixelFormat,
    pub bit_depth: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InputPixelFormat {
    VapourSynth { bit_depth: usize },
    FFmpeg { format: FFPixelFormat },
}

impl InputPixelFormat {
    #[inline]
    pub fn as_bit_depth(&self) -> anyhow::Result<usize> {
        match self {
            InputPixelFormat::VapourSynth {
                bit_depth,
            } => Ok(*bit_depth),
            InputPixelFormat::FFmpeg {
                ..
            } => Err(anyhow::anyhow!("failed to get bit depth; wrong input type")),
        }
    }

    #[inline]
    pub fn as_pixel_format(&self) -> anyhow::Result<FFPixelFormat> {
        match self {
            InputPixelFormat::VapourSynth {
                ..
            } => Err(anyhow::anyhow!("failed to get bit depth; wrong input type")),
            InputPixelFormat::FFmpeg {
                format,
            } => Ok(*format),
        }
    }
}

#[expect(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct EncodeArgs {
    pub input:       Input,
    pub proxy:       Option<Input>,
    pub temp:        String,
    pub output_file: String,

    pub chunk_method:          ChunkMethod,
    pub chunk_order:           ChunkOrdering,
    pub scaler:                String,
    pub scenes:                Option<PathBuf>,
    pub split_method:          SplitMethod,
    pub sc_pix_format:         Option<FFPixelFormat>,
    pub sc_method:             ScenecutMethod,
    pub sc_only:               bool,
    pub sc_downscale_height:   Option<usize>,
    pub extra_splits_len:      Option<usize>,
    pub min_scene_len:         usize,
    pub force_keyframes:       Vec<usize>,
    pub ignore_frame_mismatch: bool,

    pub max_tries: usize,

    pub passes:               u8,
    pub video_params:         Vec<String>,
    pub tiles:                (u32, u32), /* tile (cols, rows) count; log2 will be
                                           * applied
                                           * later
                                           * for specific encoders */
    pub encoder:              Encoder,
    pub workers:              usize,
    pub set_thread_affinity:  Option<usize>,
    pub photon_noise:         Option<u8>,
    pub photon_noise_size:    (Option<u32>, Option<u32>), // Width and Height
    pub chroma_noise:         bool,
    pub zones:                Option<PathBuf>,
    pub cache_mode:           CacheSource,
    pub pix_format_converter: PixelFormatConverter,

    // FFmpeg params
    pub ffmpeg_filter_args: Vec<String>,
    pub audio_params:       Vec<String>,
    pub input_pix_format:   InputPixelFormat,
    pub output_pix_format:  PixelFormat,

    pub verbosity:   Verbosity,
    pub resume:      bool,
    pub keep:        bool,
    pub force:       bool,
    pub no_defaults: bool,
    pub tile_auto:   bool,

    pub concat:         ConcatMethod,
    pub target_quality: TargetQuality,
    pub vmaf:           bool,
    pub vmaf_path:      Option<PathBuf>,
    pub vmaf_res:       String,
    pub probe_res:      Option<String>,
    pub vmaf_threads:   Option<usize>,
    pub vmaf_filter:    Option<String>,

    pub vapoursynth_plugins: Option<VapoursynthPlugins>,

    // Intercepted --log-file from user params
    pub user_log_file:   Option<String>,
    pub x265_csv_file:   Option<String>,
    pub merge_text_logs: bool,
}

impl EncodeArgs {
    #[inline]
    pub fn validate(&mut self) -> anyhow::Result<()> {
        if self.concat == ConcatMethod::Ivf
            && !matches!(
                self.encoder,
                Encoder::rav1e | Encoder::aom | Encoder::svt_av1 | Encoder::vpx
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

        if let Some(proxy) = &self.proxy {
            ensure!(
                proxy.as_path().exists(),
                "Proxy file {:?} does not exist!",
                proxy
            );

            // Frame count must match
            let input_frame_count = self.input.clip_info()?.num_frames;
            let proxy_frame_count = proxy.clip_info()?.num_frames;

            ensure!(
                input_frame_count == proxy_frame_count,
                "Input and Proxy do not have the same number of frames! ({input_frame_count} != \
                 {proxy_frame_count})",
            );
        }

        if self.target_quality.target.is_some() && self.input.is_vapoursynth() {
            let input_absolute_path = absolute(self.input.as_path())?;
            if !input_absolute_path.starts_with(std::env::current_dir()?) {
                warn!(
                    "Target Quality with VapourSynth script file input not in current working \
                     directory. It is recommended to run in the same directory."
                );
            }
        }
        if self.target_quality.target.is_some() {
            match self.target_quality.metric {
                TargetMetric::VMAF => validate_libvmaf()?,
                TargetMetric::SSIMULACRA2 => self.validate_ssimulacra2()?,
                TargetMetric::ButteraugliINF => self.validate_butteraugli_inf()?,
                TargetMetric::Butteraugli3 => self.validate_butteraugli_3()?,
                TargetMetric::XPSNR | TargetMetric::XPSNRWeighted => self
                    .validate_xpsnr(self.target_quality.metric, self.target_quality.probing_rate)?,
            }
        }

        if which::which("ffmpeg").is_err() {
            bail!("FFmpeg not found. Is it installed in system path?");
        }

        if self.concat == ConcatMethod::MKVMerge && which::which("mkvmerge").is_err() {
            if self.sc_only {
                warn!(
                    "mkvmerge not found, but `--concat mkvmerge` was specified. Make sure to \
                     install mkvmerge or specify a different concatenation method (e.g. `--concat \
                     ffmpeg`) before encoding."
                );
            } else {
                bail!(
                    "mkvmerge not found, but `--concat mkvmerge` was specified. Is it installed \
                     in system path?"
                );
            }
        }

        if self.encoder == Encoder::x265 && self.concat != ConcatMethod::MKVMerge {
            bail!(
                "mkvmerge is required for concatenating x265, as x265 outputs raw HEVC bitstream \
                 files without the timestamps correctly set, which FFmpeg cannot concatenate \
                 properly into a mkv file. Specify mkvmerge as the concatenation method by \
                 setting `--concat mkvmerge`."
            );
        }

        if self.encoder == Encoder::vpx && self.concat != ConcatMethod::MKVMerge {
            warn!(
                "mkvmerge is recommended for concatenating vpx, as vpx outputs with incorrect \
                 frame rates, which we can only resolve using mkvmerge. Specify mkvmerge as the \
                 concatenation method by setting `--concat mkvmerge`."
            );
        }

        if self.chunk_method == ChunkMethod::LSMASH {
            ensure!(
                self.vapoursynth_plugins.is_some_and(|p| p.lsmash),
                "LSMASH is not installed, but it was specified as the chunk method"
            );
        }
        if self.chunk_method == ChunkMethod::FFMS2 {
            ensure!(
                self.vapoursynth_plugins.is_some_and(|p| p.ffms2),
                "FFMS2 is not installed, but it was specified as the chunk method"
            );
        }
        if self.chunk_method == ChunkMethod::DGDECNV && which::which("dgindexnv").is_err() {
            ensure!(
                self.vapoursynth_plugins.is_some_and(|p| p.dgdecnv),
                "Either DGDecNV is not installed or DGIndexNV is not in system path, but it was \
                 specified as the chunk method"
            );
        }
        if self.chunk_method == ChunkMethod::BESTSOURCE {
            ensure!(
                self.vapoursynth_plugins.is_some_and(|p| p.bestsource),
                "BestSource is not installed, but it was specified as the chunk method"
            );
        }
        if self.chunk_method == ChunkMethod::Select {
            warn!("It is not recommended to use the \"select\" chunk method, as it is very slow");
        }

        if self.ignore_frame_mismatch {
            warn!(
                "The output video's frame count may differ, and target metric calculations may be \
                 incorrect"
            );
        }

        if let Some(vmaf_path) = self.target_quality.model.as_ref() {
            ensure!(vmaf_path.exists());
        }

        if self.target_quality.probes < 4 {
            warn!("Target quality with fewer than 4 probes is experimental and not recommended");
        }

        let encoder_bin = self.encoder.bin();
        if which::which(encoder_bin).is_err() {
            bail!(
                "Encoder {} not found. Is it installed in the system path?",
                encoder_bin
            );
        }

        if self.tile_auto {
            self.tiles = self.input.calculate_tiles();
        }

        if !self.no_defaults {
            if self.video_params.is_empty() {
                self.video_params = self.encoder.get_default_arguments(self.tiles);
            } else {
                // merge video_params with defaults, overriding defaults
                // TODO: consider using hashmap to store program arguments
                // instead of string vector
                let default_video_params = self.encoder.get_default_arguments(self.tiles);
                let mut skip = false;
                let mut _default_params: Vec<String> = Vec::new();
                for param in default_video_params {
                    if skip && !(param.starts_with("-") && param != "-1") {
                        skip = false;
                        continue;
                    }

                    skip = false;
                    if (param.starts_with("-") && param != "-1")
                        && self.video_params.contains(&param)
                    {
                        skip = true;
                        continue;
                    }

                    _default_params.push(param);
                }
                self.video_params = chain!(_default_params, self.video_params.clone()).collect();
            }
        }

        // Intercept encoder log-output options to prevent concurrent writes
        // from workers.
        if self.encoder == Encoder::x264 || self.encoder == Encoder::x265 {
            let intercepted = intercept_encoder_log_args(&self.video_params, self.encoder);
            self.video_params = intercepted.params;
            self.merge_text_logs = intercepted.merge_text_log;
            self.x265_csv_file = intercepted.x265_csv_file;

            if let Some(lf) = intercepted.text_log_file {
                let trimmed = lf.trim_matches(|c| c == '"' || c == '\'');
                let p = Path::new(trimmed);
                if p.is_absolute() {
                    self.user_log_file = Some(trimmed.to_string());
                } else if let Ok(cwd) = std::env::current_dir() {
                    self.user_log_file = Some(cwd.join(p).to_string_lossy().to_string());
                } else {
                    self.user_log_file = Some(trimmed.to_string());
                }
            }

            if self.x265_csv_file.is_some() {
                bail!(
                    "x265 CSV logging is not supported with av1an chunked encoding yet; use \
                     `--log-file` text output instead"
                );
            }
        }

        if let Some(strength) = self.photon_noise {
            if strength > 64 {
                bail!("Valid strength values for photon noise are 0-64");
            }
            if ![Encoder::aom, Encoder::rav1e, Encoder::svt_av1].contains(&self.encoder) {
                bail!("Photon noise synth is only supported with aomenc, rav1e, and svt-av1");
            }
        }

        if self.encoder == Encoder::aom
            && self.concat != ConcatMethod::MKVMerge
            && self.video_params.iter().any(|param| param == "--enable-keyframe-filtering=2")
        {
            bail!(
                "keyframe filtering mode 2 currently only works when using mkvmerge as the concat \
                 method"
            );
        }

        if matches!(self.encoder, Encoder::aom | Encoder::vpx)
            && self.passes != 1
            && self.video_params.iter().any(|param| param == "--rt")
        {
            // --rt must be used with 1-pass mode
            self.passes = 1;
        }

        if !self.force {
            self.validate_encoder_params()?;
            self.check_rate_control();
        }

        Ok(())
    }

    fn validate_encoder_params(&self) -> anyhow::Result<()> {
        let video_params: Vec<&str> = self
            .video_params
            .iter()
            .filter_map(|param| {
                if param.starts_with('-') && [Encoder::aom, Encoder::vpx].contains(&self.encoder) {
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
            String::from_utf8_lossy(&Command::new(cmd).arg(arg).output()?.stdout).to_string()
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

        Ok(())
    }

    /// Warns if rate control was not specified in encoder arguments
    fn check_rate_control(&self) {
        if self.encoder == Encoder::aom {
            if !self.video_params.iter().any(|f| Self::check_aom_encoder_mode(f)) {
                warn!("[WARN] --end-usage was not specified");
            }

            if !self.video_params.iter().any(|f| Self::check_aom_rate(f)) {
                warn!("[WARN] --cq-level or --target-bitrate was not specified");
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

        if s.len() <= CQ_LEVEL.len() || !(s.starts_with(TARGET_BITRATE) || s.starts_with(CQ_LEVEL))
        {
            return false;
        }

        if s.starts_with(CQ_LEVEL) {
            s.as_bytes()[CQ_LEVEL.len()..].iter().all(|&b| (b as char).is_ascii_digit())
        } else {
            s.as_bytes()[TARGET_BITRATE.len()..]
                .iter()
                .all(|&b| (b as char).is_ascii_digit())
        }
    }

    #[inline]
    pub fn validate_ssimulacra2(&self) -> anyhow::Result<()> {
        ensure!(
            self.vapoursynth_plugins.is_some_and(|p| p.vship)
                || self.vapoursynth_plugins.is_some_and(|p| p.vszip != VSZipVersion::None),
            "SSIMULACRA2 metric requires either Vapoursynth-HIP or VapourSynth Zig Image Process \
             to be installed"
        );
        self.ensure_chunk_method(
            "Chunk method must be lsmash, ffms2, bestsource, or dgdecnv for SSIMULACRA2"
                .to_string(),
        )?;

        Ok(())
    }

    #[inline]
    pub fn validate_butteraugli_inf(&self) -> anyhow::Result<()> {
        ensure!(
            self.vapoursynth_plugins.is_some_and(|p| p.vship)
                || self.vapoursynth_plugins.is_some_and(|p| p.julek),
            "Butteraugli metric requires either Vapoursynth-HIP or vapoursynth-julek-plugin to be \
             installed"
        );
        self.ensure_chunk_method(
            "Chunk method must be lsmash, ffms2, bestsource, or dgdecnv for butteraugli"
                .to_string(),
        )?;

        Ok(())
    }

    #[inline]
    pub fn validate_butteraugli_3(&self) -> anyhow::Result<()> {
        ensure!(
            self.vapoursynth_plugins.is_some_and(|p| p.vship),
            "Butteraugli 3 Norm metric requires Vapoursynth-HIP plugin to be installed"
        );
        self.ensure_chunk_method(
            "Chunk method must be lsmash, ffms2, bestsource, or dgdecnv for butteraugli 3-Norm"
                .to_string(),
        )?;

        Ok(())
    }

    #[inline]
    pub fn validate_xpsnr(&self, metric: TargetMetric, probing_rate: usize) -> anyhow::Result<()> {
        let metric_name = if metric == TargetMetric::XPSNRWeighted {
            "Weighted XPSNR"
        } else {
            "XPSNR"
        };
        if probing_rate > 1 {
            ensure!(
                self.vapoursynth_plugins.is_some_and(|p| p.vszip == VSZipVersion::New),
                format!(
                    "{metric_name} metric with probing rate greater than 1 requires \
                     VapourSynth-Zig Image Process R7 or newer to be installed"
                )
            );
            self.ensure_chunk_method(format!(
                "Chunk method must be lsmash, ffms2, bestsource, or dgdecnv for {metric_name} \
                 metric with probing rate greater than 1"
            ))?;
        } else {
            validate_libxpsnr()?;
        }

        Ok(())
    }

    fn ensure_chunk_method(&self, error_message: String) -> anyhow::Result<()> {
        ensure!(
            matches!(
                self.chunk_method,
                ChunkMethod::LSMASH
                    | ChunkMethod::FFMS2
                    | ChunkMethod::BESTSOURCE
                    | ChunkMethod::DGDECNV
            ),
            error_message
        );
        Ok(())
    }
    #[inline]
    pub fn params_indicate_logging(&self) -> bool {
        self.merge_text_logs
    }
}

#[must_use]
pub(crate) fn invalid_params<'a>(
    params: &'a [&'a str],
    valid_options: &'a HashSet<Cow<'a, str>>,
) -> Vec<&'a str> {
    params
        .iter()
        .filter(|param| !valid_options.contains(Borrow::<str>::borrow(&**param)))
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
        .and_then(|(suggestion, score)| (score > MIN_THRESHOLD).then(|| suggestion.borrow()))
}

pub(crate) fn insert_noise_table_params(
    encoder: Encoder,
    video_params: &mut Vec<String>,
    table: &Path,
) -> anyhow::Result<()> {
    match encoder {
        Encoder::aom => {
            video_params.retain(|param| !param.starts_with("--denoise-noise-level="));
            video_params.push(format!("--film-grain-table={}", table.to_string_lossy()));
        },
        Encoder::svt_av1 => {
            let film_grain_idx =
                video_params.iter().find_position(|param| param.as_str() == "--film-grain");
            if let Some((idx, _)) = film_grain_idx {
                video_params.remove(idx + 1);
                video_params.remove(idx);
            }
            video_params.push("--fgs-table".to_string());
            video_params.push(table.to_string_lossy().to_string());
        },
        Encoder::rav1e => {
            let photon_noise_idx =
                video_params.iter().find_position(|param| param.as_str() == "--photon-noise");
            if let Some((idx, _)) = photon_noise_idx {
                video_params.remove(idx + 1);
                video_params.remove(idx);
            }
            video_params.push("--photon-noise-table".to_string());
            video_params.push(table.to_string_lossy().to_string());
        },
        _ => bail!("This encoder does not support grain synth through av1an"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        concat::ConcatMethod,
        encoder::Encoder,
        ffmpeg::FFPixelFormat,
        settings::{EncodeArgs, InputPixelFormat, PixelFormat},
        target_quality::TargetQuality,
        vapoursynth::CacheSource,
        ChunkMethod,
        ChunkOrdering,
        Input,
        PixelFormatConverter,
        ScenecutMethod,
        SplitMethod,
        Verbosity,
    };

    fn test_args(video_params: Vec<String>) -> EncodeArgs {
        EncodeArgs {
            input: Input::Video {
                path:         PathBuf::new(),
                temp:         String::new(),
                chunk_method: ChunkMethod::LSMASH,
                is_proxy:     false,
                cache_mode:   CacheSource::SOURCE,
            },
            proxy: None,
            temp: String::new(),
            output_file: String::new(),
            chunk_method: ChunkMethod::LSMASH,
            chunk_order: ChunkOrdering::Random,
            scaler: String::new(),
            scenes: None,
            split_method: SplitMethod::AvScenechange,
            sc_pix_format: None,
            sc_method: ScenecutMethod::Standard,
            sc_only: false,
            sc_downscale_height: None,
            extra_splits_len: None,
            min_scene_len: 10,
            force_keyframes: Vec::new(),
            ignore_frame_mismatch: false,
            max_tries: 3,
            passes: 1,
            video_params,
            tiles: (1, 1),
            encoder: Encoder::x265,
            workers: 1,
            set_thread_affinity: None,
            photon_noise: None,
            photon_noise_size: (None, None),
            chroma_noise: false,
            zones: None,
            cache_mode: CacheSource::SOURCE,
            pix_format_converter: PixelFormatConverter::FFMPEG,
            ffmpeg_filter_args: Vec::new(),
            audio_params: Vec::new(),
            input_pix_format: InputPixelFormat::FFmpeg {
                format: FFPixelFormat::YUV420P10LE,
            },
            output_pix_format: PixelFormat {
                format:    FFPixelFormat::YUV420P10LE,
                bit_depth: 10,
            },
            verbosity: Verbosity::Normal,
            resume: false,
            keep: false,
            force: false,
            no_defaults: false,
            tile_auto: false,
            concat: ConcatMethod::FFmpeg,
            target_quality: TargetQuality::default("", Encoder::x265),
            vmaf: false,
            vmaf_path: None,
            vmaf_res: "1920x1080".to_string(),
            probe_res: None,
            vmaf_threads: None,
            vmaf_filter: None,
            vapoursynth_plugins: None,
            user_log_file: None,
            x265_csv_file: None,
            merge_text_logs: false,
        }
    }

    #[test]
    fn x265_csv_does_not_request_text_log_merge() {
        let args = test_args(vec!["--csv".to_string(), "stats.csv".to_string()]);
        assert!(!args.params_indicate_logging());
    }

    #[test]
    fn intercepts_equals_style_log_file_syntax() {
        let intercepted = super::intercept_encoder_log_args(
            &["--log-file=chunked.log".to_string()],
            Encoder::x264,
        );

        assert_eq!(intercepted.text_log_file.as_deref(), Some("chunked.log"));
        assert!(intercepted.merge_text_log);
        assert!(intercepted.params.is_empty());
    }

    #[test]
    fn log_file_with_disabled_log_level_does_not_merge_text_logs() {
        let intercepted = super::intercept_encoder_log_args(
            &["--log-file=chunked.log".to_string(), "--log-level=none".to_string()],
            Encoder::x264,
        );

        assert_eq!(intercepted.text_log_file.as_deref(), Some("chunked.log"));
        assert_eq!(intercepted.params, vec!["--log-level=none".to_string()]);
        assert!(!intercepted.merge_text_log);
    }

    #[test]
    fn unrelated_equals_parameters_do_not_disable_log_merging() {
        let intercepted = super::intercept_encoder_log_args(
            &["--log-level=info".to_string(), "--crf=20".to_string()],
            Encoder::x264,
        );

        assert_eq!(intercepted.params, vec![
            "--log-level=info".to_string(),
            "--crf=20".to_string()
        ]);
        assert!(intercepted.merge_text_log);
    }

    #[test]
    fn space_separated_log_level_keeps_its_value() {
        let intercepted = super::intercept_encoder_log_args(
            &["--log-level".to_string(), "info".to_string()],
            Encoder::x264,
        );

        assert_eq!(intercepted.params, vec![
            "--log-level".to_string(),
            "info".to_string()
        ]);
        assert!(intercepted.merge_text_log);
    }
}
