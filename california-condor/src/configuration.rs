use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use andean_condor::{
    core::{Condor, SaveCallback, output::Output},
    ffmpeg::FFPixelFormat,
    models::{
        Condor as CondorModel,
        encoder::{Encoder, EncoderBase},
        input::{
            Ffms2Filter,
            Input as InputModel,
            VapourSynthImportMethod,
            VapourSynthScriptSource,
        },
        output::Output as OutputModel,
        sequence::{
            SequenceConfigHandler,
            SequenceDataHandler,
            benchmarker::{BenchmarkerConfig, BenchmarkerConfigHandler},
            bitrate_optimizer::{BitrateOptimizerConfig, BitrateOptimizerConfigHandler},
            noise_detector::{
                NoiseDetectorConfig,
                NoiseDetectorConfigHandler,
                NoiseDetectorData,
                NoiseDetectorDataHandler,
            },
            noise_scaler::{
                NoiseScalerConfig,
                NoiseScalerConfigHandler,
                NoiseScalerData,
                NoiseScalerDataHandler,
            },
            parallel_encoder::{
                ParallelEncoderConfig,
                ParallelEncoderConfigHandler,
                ParallelEncoderData,
                ParallelEncoderDataHandler,
            },
            quality_check::{
                QualityCheckConfig,
                QualityCheckConfigHandler,
                QualityCheckData,
                QualityCheckDataHandler,
            },
            scene_concatenator::{SceneConcatenatorConfig, SceneConcatenatorConfigHandler},
            scene_detector::{
                DEFAULT_MAX_SCENE_LENGTH_SECONDS,
                SceneDetectionMethod,
                SceneDetectorConfig,
                SceneDetectorData,
                SceneDetectorDataHandler,
                ScenecutMethod,
            },
            speed_scaler::{SpeedScalerConfig, SpeedScalerConfigHandler},
            target_quality::{
                TargetQualityConfig,
                TargetQualityConfigHandler,
                TargetQualityData,
                TargetQualityDataHandler,
            },
        },
    },
    vapoursynth::{plugins::resize::Scaler, vapoursynth_filters::VapourSynthFilter},
};
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    commands::DecoderMethod,
    tui::open_input,
    utils::hash_path::hash_path,
    zoner::{ZonerConfig, ZonerData},
};

/// Fallback schema URL used when the binary was built without git metadata
/// (e.g. from a source tarball). Normal builds bake the correct URL in via
/// `CONDOR_SCHEMA_URL` in `build.rs`.
pub const CONFIGURATION_SCHEMA_URL: &str =
    "https://github.com/rust-av/Av1an/releases/download/latest/configuration.schema.json";

#[inline]
fn default_schema_url() -> String {
    option_env!("CONDOR_SCHEMA_URL").unwrap_or(CONFIGURATION_SCHEMA_URL).to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Configuration {
    #[serde(rename = "$schema", default = "default_schema_url")]
    #[schemars(description = "URL of the JSON schema used to validate this configuration file")]
    pub schema: String,
    pub condor: CondorModel<CliSequenceData, CliSequenceConfig>,
    // Duplicated in case Condor instantiates a VapourSynthScript Input
    pub input:  PathBuf,
    pub temp:   PathBuf,
}

impl Configuration {
    #[inline]
    pub fn new(
        input: &Path,
        output: &Path,
        temp: Option<&Path>,
        vs_args: Option<&[String]>,
        decoder: Option<&DecoderMethod>,
    ) -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let input_abs = path_abs::PathAbs::new(input)?;
        let temp = path_abs::PathAbs::new(
            temp.map_or_else(|| cwd.join(hash_path(input_abs.as_path())), PathBuf::from),
        )?
        .as_path()
        .to_path_buf();
        let input_data = Self::new_input_model(input, decoder, vs_args, None, None)?;
        // The startup open, needed here for the frame rate. This is also what
        // builds a missing index, which `open_input` reports on the log.
        let (_, clip_info) = open_input(
            &input_data,
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?;
        let fps = *clip_info.frame_rate.numer() as f64 / *clip_info.frame_rate.denom() as f64;

        let scenes_directory = temp.join("scenes");

        let mut configuration = Self {
            schema: default_schema_url(),
            condor: CondorModel {
                input:           input_data,
                output:          OutputModel {
                    path:       output.to_path_buf(),
                    tags:       HashMap::new(),
                    video_tags: HashMap::new(),
                },
                encoder:         Encoder::default(),
                scenes:          Vec::new(),
                sequence_config: CliSequenceConfig {
                    scene_detector:     SceneDetectorConfig {
                        input:  None,
                        method: SceneDetectionMethod::AVSceneChange {
                            minimum_length: fps.round() as usize,
                            maximum_length: DEFAULT_MAX_SCENE_LENGTH_SECONDS as usize
                                * fps.round() as usize,
                            method:         ScenecutMethod::Standard,
                            save_scores:    false,
                        },
                    },
                    zoner:              None,
                    noise_detector:     None,
                    noise_scaler:       None,
                    benchmarker:        BenchmarkerConfig::default(),
                    parallel_encoder:   ParallelEncoderConfig::new(&scenes_directory),
                    scene_concatenator: SceneConcatenatorConfig::new(&scenes_directory),
                    target_quality:     None,
                    quality_check:      None,
                    bitrate_optimizer:  BitrateOptimizerConfig::default(),
                    speed_scaler:       SpeedScalerConfig::default(),
                },
            },
            input: input.to_path_buf(),
            temp,
        };

        // `init` defaults to encoding 10-bit, so the input carries the
        // conversion rather than the config holding a filter list.
        match &mut configuration.condor.input {
            InputModel::Video {
                filters, ..
            } => {
                *filters = vec![Ffms2Filter::OutputFormat {
                    bit_depth: Some(10),
                    chroma:    None,
                    width:     None,
                    height:    None,
                }];
            },
            InputModel::VapourSynth {
                filters, ..
            }
            | InputModel::VapourSynthScript {
                filters, ..
            } => {
                *filters = vec![VapourSynthFilter::Resize {
                    scaler: Some(Scaler::Bicubic),
                    width:  None,
                    height: None,
                    format: Some(FFPixelFormat::YUV420P10LE),
                }];
            },
        }

        *configuration.condor.encoder.parameters_mut() = EncoderBase::SVTAV1.default_parameters();

        Ok(configuration)
    }

    #[inline]
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        Self::save_data(self, path)?;
        Ok(())
    }

    #[inline]
    pub fn save_data(data: &Configuration, path: &Path) -> Result<(), ConfigError> {
        let mut buffer = vec![];
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
        data.serialize(&mut serializer).map_err(ConfigError::Serialize)?;
        let directory = path.parent();
        if let Some(directory) = directory {
            std::fs::create_dir_all(directory).map_err(ConfigError::Save)?;
        }
        let temp_path = path.with_extension("temp");
        std::fs::write(&temp_path, &buffer).map_err(ConfigError::Save)?;
        std::fs::rename(&temp_path, path).map_err(ConfigError::Save)?;
        Ok(())
    }

    #[inline]
    pub fn load(config_path: &Path) -> Result<Option<Configuration>, ConfigError> {
        if !config_path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(config_path)
            .map_err(|_| ConfigError::Load(config_path.to_path_buf()))?;
        let data = serde_json::from_str(&data).map_err(ConfigError::Parse)?;

        Ok(Some(data))
    }

    #[inline]
    pub fn instantiate_condor(
        &self,
        save_callback: SaveCallback<CliSequenceData, CliSequenceConfig>,
    ) -> Result<Condor<CliSequenceData, CliSequenceConfig>> {
        // The input carries its own filters, so every input kind is opened the
        // same way — and opened once here, so the `Input` every later phase
        // reads through `condor.input` is the one indexed here.
        let (input, _clip_info) = open_input(
            &self.condor.input,
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?;
        let output = Output::new(&self.condor.output)?;

        let condor = Condor {
            input,
            output,
            encoder: self.condor.encoder.clone(),
            scenes: self.condor.scenes.clone(),
            sequence_config: self.condor.sequence_config.clone(),
            save_callback,
        };

        Ok(condor)
    }

    #[inline]
    pub fn new_input_model(
        input: &Path,
        decoder: Option<&DecoderMethod>,
        vs_args: Option<&[String]>,
        index: Option<u8>,
        cache_path: Option<&Path>,
    ) -> Result<InputModel> {
        if Self::input_is_script(input) {
            return Self::new_vs_input_model(input, None, vs_args, index);
        }

        let input_model = match decoder {
            Some(DecoderMethod::FFMS2) => InputModel::Video {
                path:          input.to_path_buf(),
                import_method: andean_condor::models::input::ImportMethod::FFMS2 {
                    index,
                    cache_path: cache_path.map(Path::to_path_buf),
                },
                filters:       Vec::new(),
            },
            Some(decoder) => Self::new_vs_input_model(
                input,
                match decoder {
                    DecoderMethod::VSFFMS2 => Some(VapourSynthImportMethod::FFMS2 {
                        index,
                    }),
                    DecoderMethod::BestSource => Some(VapourSynthImportMethod::BestSource {
                        index,
                    }),
                    DecoderMethod::DGDecodeNV => Some(VapourSynthImportMethod::DGDecNV {
                        dgindexnv_executable: None,
                    }),
                    DecoderMethod::LSMASHWorks => Some(VapourSynthImportMethod::LSMASHWorks {
                        index,
                    }),
                    _ => unreachable!(),
                },
                vs_args,
                index,
            )?,
            None => Self::new_vs_input_model(input, None, vs_args, index)?,
        };

        Ok(input_model)
    }

    #[inline]
    pub fn new_vs_input_model(
        input: &Path,
        decoder: Option<VapourSynthImportMethod>,
        vs_args: Option<&[String]>,
        index: Option<u8>,
    ) -> Result<InputModel> {
        let input_data = if Self::input_is_script(input) {
            let variables = vs_args.map_or_else(HashMap::new, |vs_args| {
                vs_args
                    .iter()
                    .map(|arg| {
                        let (key, value) = arg.split_once('=').unwrap_or((arg, ""));
                        (key.to_string(), value.to_string())
                    })
                    .collect()
            });
            InputModel::VapourSynthScript {
                source: VapourSynthScriptSource::Path(input.to_path_buf()),
                variables,
                index: index.unwrap_or_default(),
                filters: Vec::new(),
                stream_concurrently: true,
            }
        } else {
            InputModel::VapourSynth {
                path:          input.to_path_buf(),
                import_method: decoder.unwrap_or(VapourSynthImportMethod::BestSource {
                    index,
                }),
                cache_path:    None,
                filters:       Vec::new(),
            }
        };

        Ok(input_data)
    }

    /// Returns `true` if the input path is a VapourSynth script, ending with
    /// `.vpy` or `.py`.
    #[inline]
    pub fn input_is_script(input: &Path) -> bool {
        input
            .extension()
            .map(|s| s.to_str())
            .is_some_and(|s| s.is_some_and(|extension| matches!(extension, "vpy" | "py")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliSequenceConfig
where
    Self: SequenceConfigHandler
        + BenchmarkerConfigHandler
        + NoiseDetectorConfigHandler
        + NoiseScalerConfigHandler
        + TargetQualityConfigHandler
        + QualityCheckConfigHandler
        + BitrateOptimizerConfigHandler
        + SpeedScalerConfigHandler
        + ParallelEncoderConfigHandler
        + SceneConcatenatorConfigHandler,
{
    pub scene_detector:     SceneDetectorConfig,
    pub zoner:              Option<ZonerConfig>,
    pub noise_detector:     Option<NoiseDetectorConfig>,
    pub noise_scaler:       Option<NoiseScalerConfig>,
    pub benchmarker:        BenchmarkerConfig,
    pub parallel_encoder:   ParallelEncoderConfig,
    pub scene_concatenator: SceneConcatenatorConfig,
    pub target_quality:     Option<TargetQualityConfig>,
    pub quality_check:      Option<QualityCheckConfig>,
    pub bitrate_optimizer:  BitrateOptimizerConfig,
    pub speed_scaler:       SpeedScalerConfig,
}

impl Default for CliSequenceConfig {
    #[inline]
    fn default() -> Self {
        Self {
            scene_detector:     SceneDetectorConfig::default(),
            zoner:              None,
            noise_detector:     None,
            noise_scaler:       None,
            benchmarker:        BenchmarkerConfig::default(),
            parallel_encoder:   ParallelEncoderConfig::default(),
            scene_concatenator: SceneConcatenatorConfig::default(),
            target_quality:     None,
            quality_check:      None,
            bitrate_optimizer:  BitrateOptimizerConfig::default(),
            speed_scaler:       SpeedScalerConfig::default(),
        }
    }
}

impl SequenceConfigHandler for CliSequenceConfig {
}

impl BenchmarkerConfigHandler for CliSequenceConfig {
    fn benchmarker(&self) -> Result<&BenchmarkerConfig> {
        Ok(&self.benchmarker)
    }

    fn benchmarker_mut(&mut self) -> Result<&mut BenchmarkerConfig> {
        Ok(&mut self.benchmarker)
    }
}

impl NoiseDetectorConfigHandler for CliSequenceConfig {
    fn noise_detector(&self) -> Result<&Option<NoiseDetectorConfig>> {
        Ok(&self.noise_detector)
    }

    fn noise_detector_mut(&mut self) -> Result<&mut Option<NoiseDetectorConfig>> {
        Ok(&mut self.noise_detector)
    }
}

impl NoiseScalerConfigHandler for CliSequenceConfig {
    fn noise_scaler(&self) -> Result<&Option<NoiseScalerConfig>> {
        Ok(&self.noise_scaler)
    }

    fn noise_scaler_mut(&mut self) -> Result<&mut Option<NoiseScalerConfig>> {
        Ok(&mut self.noise_scaler)
    }
}

impl TargetQualityConfigHandler for CliSequenceConfig {
    fn target_quality(&self) -> Result<&Option<TargetQualityConfig>> {
        Ok(&self.target_quality)
    }

    fn target_quality_mut(&mut self) -> Result<&mut Option<TargetQualityConfig>> {
        Ok(&mut self.target_quality)
    }
}

impl QualityCheckConfigHandler for CliSequenceConfig {
    fn quality_check(&self) -> Result<&Option<QualityCheckConfig>> {
        Ok(&self.quality_check)
    }

    fn quality_check_mut(&mut self) -> Result<&mut Option<QualityCheckConfig>> {
        Ok(&mut self.quality_check)
    }
}

impl BitrateOptimizerConfigHandler for CliSequenceConfig {
    fn bitrate_optimizer(&self) -> Result<&BitrateOptimizerConfig> {
        Ok(&self.bitrate_optimizer)
    }

    fn bitrate_optimizer_mut(&mut self) -> Result<&mut BitrateOptimizerConfig> {
        Ok(&mut self.bitrate_optimizer)
    }
}

impl SpeedScalerConfigHandler for CliSequenceConfig {
    fn speed_scaler(&self) -> Result<&SpeedScalerConfig> {
        Ok(&self.speed_scaler)
    }

    fn speed_scaler_mut(&mut self) -> Result<&mut SpeedScalerConfig> {
        Ok(&mut self.speed_scaler)
    }
}

impl ParallelEncoderConfigHandler for CliSequenceConfig {
    fn parallel_encoder(&self) -> Result<&ParallelEncoderConfig> {
        Ok(&self.parallel_encoder)
    }

    fn parallel_encoder_mut(&mut self) -> Result<&mut ParallelEncoderConfig> {
        Ok(&mut self.parallel_encoder)
    }
}

impl SceneConcatenatorConfigHandler for CliSequenceConfig {
    fn scene_concatenator(&self) -> Result<&SceneConcatenatorConfig> {
        Ok(&self.scene_concatenator)
    }

    fn scene_concatenator_mut(&mut self) -> Result<&mut SceneConcatenatorConfig> {
        Ok(&mut self.scene_concatenator)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliSequenceData
where
    Self: SequenceDataHandler
        + SceneDetectorDataHandler
        + NoiseDetectorDataHandler
        + NoiseScalerDataHandler
        + ParallelEncoderDataHandler
        + TargetQualityDataHandler
        + QualityCheckDataHandler,
{
    pub scene_detection:  SceneDetectorData,
    pub noise_detection:  Option<NoiseDetectorData>,
    pub noise_scaling:    Option<NoiseScalerData>,
    pub parallel_encoder: ParallelEncoderData,
    pub target_quality:   TargetQualityData,
    pub quality_check:    QualityCheckData,
    /// The Zoner's record of having processed the list; absent in
    /// configurations written before zoning became idempotent.
    #[serde(default)]
    pub zoner:            ZonerData,
}

impl SequenceDataHandler for CliSequenceData {
}

impl Default for CliSequenceData {
    #[inline]
    fn default() -> Self {
        Self {
            scene_detection:  SceneDetectorData::default(),
            noise_detection:  None,
            noise_scaling:    None,
            parallel_encoder: ParallelEncoderData::default(),
            target_quality:   TargetQualityData::default(),
            quality_check:    QualityCheckData::default(),
            zoner:            ZonerData::default(),
        }
    }
}

impl SceneDetectorDataHandler for CliSequenceData {
    fn get_scene_detection(&self) -> Result<&SceneDetectorData> {
        Ok(&self.scene_detection)
    }

    fn get_scene_detection_mut(&mut self) -> Result<&mut SceneDetectorData> {
        Ok(&mut self.scene_detection)
    }
}

impl NoiseDetectorDataHandler for CliSequenceData {
    fn get_noise_detection(&self) -> Result<&Option<NoiseDetectorData>> {
        Ok(&self.noise_detection)
    }

    fn get_noise_detection_mut(&mut self) -> Result<&mut Option<NoiseDetectorData>> {
        Ok(&mut self.noise_detection)
    }
}

impl NoiseScalerDataHandler for CliSequenceData {
    fn get_noise_scaling(&self) -> Result<&Option<NoiseScalerData>> {
        Ok(&self.noise_scaling)
    }

    fn get_noise_scaling_mut(&mut self) -> Result<&mut Option<NoiseScalerData>> {
        Ok(&mut self.noise_scaling)
    }
}

impl ParallelEncoderDataHandler for CliSequenceData {
    fn get_parallel_encoder(&self) -> Result<&ParallelEncoderData> {
        Ok(&self.parallel_encoder)
    }

    fn get_parallel_encoder_mut(&mut self) -> Result<&mut ParallelEncoderData> {
        Ok(&mut self.parallel_encoder)
    }
}

impl TargetQualityDataHandler for CliSequenceData {
    fn get_target_quality(&self) -> Result<&TargetQualityData> {
        Ok(&self.target_quality)
    }

    fn get_target_quality_mut(&mut self) -> Result<&mut TargetQualityData> {
        Ok(&mut self.target_quality)
    }
}

impl QualityCheckDataHandler for CliSequenceData {
    fn get_quality_check(&self) -> Result<&QualityCheckData> {
        Ok(&self.quality_check)
    }

    fn get_quality_check_mut(&mut self) -> Result<&mut QualityCheckData> {
        Ok(&mut self.quality_check)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load config file: {0}")]
    Load(PathBuf),
    #[error("Failed to parse config file: {0}")]
    Parse(serde_json::Error),
    #[error("Failed to serialize/deserialize config file: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Failed to save config file: {0}")]
    Save(#[from] std::io::Error),
}
