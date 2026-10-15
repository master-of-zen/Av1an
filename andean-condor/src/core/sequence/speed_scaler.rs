use std::sync::{self, Arc, atomic::AtomicBool};

use thiserror::Error;

use crate::{
    core::{
        Condor,
        sequence::{Sequence, SequenceDetails, SequenceStatus},
    },
    models::sequence::{
        SequenceConfigHandler,
        SequenceDataHandler,
        speed_scaler::SpeedScalerConfigHandler,
    },
    utils::interpolators::natural_cubic_spline,
};

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Convex Hull",
    description: "Applies a speed based on the quantiizer per scene.",
    version:     "0.0.1",
};

#[derive(Default)]
pub struct SpeedScaler {}

impl<Data, Config> Sequence<Data, Config> for SpeedScaler
where
    Data: SequenceDataHandler,
    Config: SequenceConfigHandler + SpeedScalerConfigHandler,
{
    #[inline]
    fn details(&self) -> SequenceDetails {
        DETAILS
    }

    #[inline]
    fn validate(
        &mut self,
        condor: &mut Condor<Data, Config>,
    ) -> anyhow::Result<((), Vec<anyhow::Error>)> {
        let mut warnings = vec![];
        let speed_quantizers = &condor.sequence_config.speed_scaler()?.speed_quantizers;
        // Ensure we have at least 2 pairs for interpolation
        if speed_quantizers.len() < 2 {
            warnings.push(anyhow::Error::new(
                SpeedScalerError::MinimumSpeedQuantizerPairsRequired,
            ));
        }

        // Ensure scenes is not empty
        if condor.scenes.is_empty() {
            warnings.push(anyhow::Error::new(SpeedScalerError::ScenesEmpty));
        }
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
        let speed_quantizers = &condor.sequence_config.speed_scaler()?.speed_quantizers;
        // Ensure we have at least 2 pairs for interpolation and scenes is not empty
        if speed_quantizers.len() < 2 || condor.scenes.is_empty() {
            return Ok(((), vec![]));
        }

        let mut sorted_speed_quantizers = speed_quantizers.clone();
        sorted_speed_quantizers.sort_by(|(_s1, q1), (_s2, q2)| {
            q1.partial_cmp(q2).unwrap_or(std::cmp::Ordering::Equal)
        });

        for scene in condor.scenes.iter_mut() {
            if let Some(quantizer) = scene.encoder.quantizer() {
                // Interpolate speed based on quantizer
                let quantizers =
                    sorted_speed_quantizers.iter().map(|(_s, q)| *q).collect::<Vec<f64>>();
                let speeds =
                    sorted_speed_quantizers.iter().map(|(s, _q)| *s as f64).collect::<Vec<f64>>();
                let interpolated_speed = natural_cubic_spline(&quantizers, &speeds, quantizer);
                if let Some(speed) = interpolated_speed {
                    scene.encoder.set_speed(speed.round() as i8);
                } else {
                    // Quantizer outside of range, use extremes
                    let (fastest, _lowest_quantizer) =
                        sorted_speed_quantizers.first().expect("Speed-Quantizer exists");
                    let (slowest, highest_quantizer) =
                        sorted_speed_quantizers.last().expect("Speed-Quantizer exists");
                    if quantizer >= *highest_quantizer {
                        scene.encoder.set_speed(*slowest);
                    } else {
                        scene.encoder.set_speed(*fastest);
                    }
                }
            }
        }

        condor.save()?;

        Ok(((), vec![]))
    }
}

impl SpeedScaler {
    pub const DETAILS: SequenceDetails = DETAILS;
}

#[derive(Debug, Error)]
pub enum SpeedScalerError {
    #[error("No Scenes found")]
    ScenesEmpty,
    #[error("At least 2 speed-quantizer pairs are required")]
    MinimumSpeedQuantizerPairsRequired,
}
