use std::sync::{self, Arc, atomic::AtomicBool};

use tracing::debug;

use crate::{
    core::{
        Condor,
        sequence::{Sequence, SequenceDetails, SequenceStatus},
    },
    models::{
        encoder::Encoder,
        sequence::{
            SequenceConfigHandler,
            SequenceDataHandler,
            noise_detector::NoiseDetectorDataHandler,
            noise_scaler::{NoiseScalerConfigHandler, NoiseScalerData, NoiseScalerDataHandler},
        },
    },
};

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Noise Scaler",
    description: "Scales the Photon Noise per scene based on Noise Detector results.",
    version:     "0.0.1",
};

#[derive(Default)]
pub struct NoiseScaler {}

impl<Data, Config> Sequence<Data, Config> for NoiseScaler
where
    Data: SequenceDataHandler + NoiseDetectorDataHandler + NoiseScalerDataHandler,
    Config: SequenceConfigHandler + NoiseScalerConfigHandler,
{
    #[inline]
    fn details(&self) -> SequenceDetails {
        DETAILS
    }

    #[inline]
    fn validate(
        &mut self,
        _condor: &mut Condor<Data, Config>,
    ) -> anyhow::Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        Ok(((), warnings))
    }

    #[inline]
    fn initialize(
        &mut self,
        _condor: &mut Condor<Data, Config>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
    ) -> anyhow::Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        Ok(((), warnings))
    }

    #[inline]
    fn execute(
        &mut self,
        condor: &mut Condor<Data, Config>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
        _cancelled: Arc<AtomicBool>,
    ) -> anyhow::Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];
        let Some(config) = &condor.sequence_config.noise_scaler()? else {
            return Ok(((), warnings));
        };

        if condor.scenes.is_empty() {
            return Ok(((), vec![]));
        }

        let noise_levels = condor
            .scenes
            .iter()
            .filter_map(|scene| scene.sequence_data.get_noise_detection().ok())
            .flatten()
            .map(|data| data.noise)
            .collect::<Vec<_>>();
        let max_noise_level = noise_levels.into_iter().reduce(f64::max).unwrap_or(0.0);

        for (index, scene) in condor.scenes.iter_mut().enumerate() {
            if scene.sequence_data.get_noise_scaling()?.is_some() {
                // Noise already scaled
                continue;
            }
            let scene_photon_noise = match &mut scene.encoder {
                Encoder::AOM {
                    photon_noise, ..
                }
                | Encoder::RAV1E {
                    photon_noise, ..
                } => photon_noise,
                Encoder::SVTAV1 {
                    options,
                    photon_noise,
                    ..
                } => {
                    if !options.contains_key("film-grain") {
                        photon_noise
                    } else {
                        &mut None
                    }
                },
                _ => &mut None,
            };
            let Some(scene_photon_noise) = scene_photon_noise else {
                continue;
            };
            let Some(noise_level) =
                scene.sequence_data.get_noise_detection()?.clone().map(|data| data.noise)
            else {
                continue;
            };
            if noise_level < config.threshold {
                continue;
            }

            let relative_noise_level =
                (noise_level - config.threshold) / (max_noise_level - config.threshold);
            let scaler = config.minimum_scaler
                + (relative_noise_level * (config.maximum_scaler - config.minimum_scaler));
            debug!("Scaling Scene {} Photon Noise by {:.2}", index, scaler);

            scene_photon_noise.iso = (scene_photon_noise.iso as f64 * scaler).round() as u32;
            if config.scale_chroma {
                scene_photon_noise.chroma_iso =
                    scene_photon_noise.chroma_iso.map(|iso| (iso as f64 * scaler).round() as u32);
            }

            *scene.sequence_data.get_noise_scaling_mut()? = Some(NoiseScalerData {
                scaler,
            });
        }

        condor.save()?;

        Ok(((), warnings))
    }
}

impl NoiseScaler {
    pub const DETAILS: SequenceDetails = DETAILS;
}
