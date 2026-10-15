use std::sync::{self, Arc, atomic::AtomicBool};

use anyhow::Result;
use thiserror::Error;
use tracing::debug;

use crate::{
    core::{
        Condor,
        sequence::{Sequence, SequenceDetails, SequenceStatus},
    },
    models::sequence::{
        SequenceConfigHandler,
        SequenceDataHandler,
        bitrate_optimizer::BitrateOptimizerConfigHandler,
        target_quality::TargetQualityDataHandler,
    },
};

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Bitrate Optimizer",
    description: "Optimize scenes that exceed normal bitrate after targeting quality.",
    version:     "0.0.1",
};

#[derive(Debug, Default)]
pub struct BitrateOptimizer {}

impl<DataHandler, ConfigHandler> Sequence<DataHandler, ConfigHandler> for BitrateOptimizer
where
    DataHandler: SequenceDataHandler + TargetQualityDataHandler,
    ConfigHandler: SequenceConfigHandler + BitrateOptimizerConfigHandler,
{
    #[inline]
    fn details(&self) -> SequenceDetails {
        DETAILS
    }

    #[inline]
    fn validate(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        if condor.scenes.is_empty() {
            return Err(anyhow::Error::new(BitrateOptimizerError::ScenesEmpty));
        }

        Ok(((), warnings))
    }

    #[inline]
    fn initialize(
        &mut self,
        _condor: &mut Condor<DataHandler, ConfigHandler>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let warnings = vec![];

        Ok(((), warnings))
    }

    #[inline]
    fn execute(
        &mut self,
        condor: &mut Condor<DataHandler, ConfigHandler>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let mut warnings = vec![];
        let Some(bitrate_sigma_threshold) =
            condor.sequence_config.bitrate_optimizer()?.bitrate_sigma_threshold
        else {
            return Ok(((), warnings));
        };

        if condor.scenes.is_empty() {
            warnings.push(anyhow::Error::new(BitrateOptimizerError::ScenesEmpty));
            return Ok(((), warnings));
        }

        let scene_values = condor
            .scenes
            .iter()
            .filter_map(|scene| scene.sequence_data.get_target_quality().ok())
            .filter_map(|tq| tq.passes.last())
            .map(|pass| (pass.quantizer, pass.bitrate))
            .collect::<Vec<_>>();
        let scene_quantizers = scene_values.iter().map(|(q, _b)| *q).collect::<Vec<_>>();
        let scene_bitrates = scene_values.iter().map(|(_q, b)| *b).collect::<Vec<_>>();

        let quantizer_average =
            scene_quantizers.iter().sum::<f64>() / scene_quantizers.len() as f64;
        debug!("Average Quantizer: {}", quantizer_average);

        let bitrate_average = scene_bitrates.iter().sum::<f64>() / scene_bitrates.len() as f64;
        let bitrate_variance = scene_bitrates
            .iter()
            .map(|bitrate| (bitrate - bitrate_average).powi(2))
            .sum::<f64>()
            / scene_bitrates.len() as f64;
        let bitrate_standard_deviation = bitrate_variance.sqrt();
        let bitrate_sigma_threshold =
            (bitrate_sigma_threshold as f64).mul_add(bitrate_standard_deviation, bitrate_average);

        for (index, scene) in condor.scenes.iter_mut().enumerate() {
            let Some(final_pass) = scene.sequence_data.get_target_quality()?.passes.last() else {
                continue;
            };
            if final_pass.bitrate <= bitrate_sigma_threshold {
                continue;
            }

            debug!(
                "Optimized Scene {}: Bitrate: {}({}), Quantizer: {}",
                index,
                final_pass.bitrate,
                (final_pass.bitrate - bitrate_average) / bitrate_standard_deviation,
                quantizer_average.round(),
            );
            scene.encoder.set_quantizer(quantizer_average.round());

            // let data = scene
            //     .sequence_data
            //     .get_target_quality()?
            //     .passes
            //     .iter()
            //     .map(|pass| {
            //         (
            //             pass.quantizer,
            //             (pass.scores.iter().sum::<f64>() / pass.scores.len()
            // as f64) / pass.bitrate,         )
            //     })
            //     .collect::<Vec<_>>();

            // let fit = CurveFit::<ChebyshevBasis>::new_auto(&data,
            // DegreeBound::Custom(2), &Aic)?;

            // for critical_point in fit.critical_points()? {
            //     if let CriticalPoint::Maxima(quantizer, _efficiency) =
            // critical_point {         debug!(
            //             "Optimized Scene {}: Bitrate: {}({}), Quantizer: {}",
            //             index,
            //             final_pass.bitrate,
            //             (final_pass.bitrate - bitrate_average) /
            // bitrate_standard_deviation,             quantizer
            //         );
            //         scene.encoder.set_quantizer(quantizer.round());
            //     }
            // }
        }

        condor.save()?;

        Ok(((), warnings))
    }
}

#[derive(Debug, Clone, Error)]
pub enum BitrateOptimizerError {
    #[error("No Scenes found")]
    ScenesEmpty,
    #[error("Previous Pass data not found")]
    PreviousPassDataNotFound,
    #[error("Failed to measure quality")]
    QualityMeasurementFailed,
}
