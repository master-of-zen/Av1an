//! The CLI-only Zoner sequence.
//!
//! A [`Zone`](zone::Zone) is a frame range plus the encoder settings and scene
//! length cap its scenes take. Zones reach Condor through
//! [`ZonerConfig`], written by the *--zones* flag on
//! [`condor`](../../commands/condor.md) or by the
//! [`zone`](../../commands/zone.md) command from zone strings, JSON, JSON
//! lines, or text files.
//!
//! The work is split three ways: [`zone`] holds the data model and the encoder
//! application, [`parse`] reads zones from strings and files, and this module
//! owns the configuration, the validation and the [`Sequence`]
//! implementation. Application and validation are plain functions over scenes
//! so they can be tested without an open input.
//!
//! The pipeline applies zones after Scene Detector, then records the pass on
//! every scene ([`ZonerData::zoned`]); later runs skip the Zoner until
//! `condor scenes reset zoner` clears that record. Individual overrides are
//! idempotent, but a length cap can change which zones a scene intersects on
//! a later pass, so use freshly detected scenes when reapplying interacting
//! zones.

use std::sync::{self, Arc, atomic::AtomicBool};

use andean_condor::{
    core::{
        Condor,
        sequence::{Sequence, SequenceDetails, SequenceStatus},
    },
    models::scene::Scene,
};
use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    configuration::{CliSequenceConfig, CliSequenceData},
    scenes::stale_result_warnings,
    zoner::zone::zone_label,
};

pub mod parse;
pub mod zone;

pub use parse::{load_zones, parse_zone, parse_zones};
pub use zone::{
    Zone,
    ZoneEncoder,
    ZoneEncoderMerge,
    ZoneOptions,
    ZoneOptionsBody,
    ZonePhotonNoise,
    ZonePhotonNoiseBody,
};

pub static DETAILS: SequenceDetails = SequenceDetails {
    name:        "Zoner",
    description: "Overrides the encoder of the scenes inside frame ranges.",
    version:     "0.0.1",
};

/// `condor.sequence_config.zoner` in `condor.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ZonerConfig {
    /// The overrides, applied in start-frame order.
    #[serde(default)]
    pub zones: Vec<Zone>,
}

/// `condor.scenes[].sequence_data.zoner` in `condor.json`.
///
/// The record the Zoner writes when it has applied the configured zones to
/// the scene list, which is what later runs read to skip.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ZonerData {
    /// Whether the Zoner has processed this scene list. `condor scenes reset
    /// zoner` clears it so the zones apply again.
    #[serde(default)]
    pub zoned: bool,
}

#[derive(Default)]
pub struct Zoner {}

impl Zoner {
    pub const DETAILS: SequenceDetails = DETAILS;

    /// Validates and applies the configured zones.
    ///
    /// The pass records itself on the scenes it processed, so a later run
    /// applies nothing until `condor scenes reset zoner` clears the record.
    pub fn run(condor: &mut Condor<CliSequenceData, CliSequenceConfig>) -> Result<()> {
        if condor.scenes.iter().any(|scene| scene.sequence_data.zoner.zoned) {
            info!(
                "Zoner: the scenes already record a zoning pass; nothing to apply (reset the \
                 record with `condor scenes reset zoner`)"
            );
            return Ok(());
        }

        let mut zoner = Self::default();

        let (_, warnings) = zoner.validate(condor)?;
        log_warnings(&warnings);

        let (progress_tx, _progress_rx) = sync::mpsc::channel();
        let (_, warnings) = zoner.execute(condor, progress_tx, Arc::new(AtomicBool::new(false)))?;
        log_warnings(&warnings);

        Ok(())
    }
}

impl Sequence<CliSequenceData, CliSequenceConfig> for Zoner {
    #[inline]
    fn details(&self) -> SequenceDetails {
        DETAILS
    }

    fn validate(
        &mut self,
        condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let zones = configured_zones(&condor.sequence_config);
        if !zones.is_empty() {
            validate_zones(zones, condor.input.clip_info()?.num_frames)?;
        }

        Ok(((), Vec::new()))
    }

    #[inline]
    fn initialize(
        &mut self,
        _condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        Ok(((), Vec::new()))
    }

    fn execute(
        &mut self,
        condor: &mut Condor<CliSequenceData, CliSequenceConfig>,
        _progress_tx: sync::mpsc::Sender<SequenceStatus>,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<((), Vec<anyhow::Error>)> {
        let zones = configured_zones(&condor.sequence_config);
        if zones.is_empty() {
            return Ok(((), Vec::new()));
        }
        if condor.scenes.is_empty() {
            bail!("no scenes to zone; run Scene Detector first");
        }

        let original_scenes = condor.scenes.clone();
        let mut warnings = apply_zones(
            zones,
            &mut condor.scenes,
            condor.input.clip_info()?.num_frames,
        );
        let stale_warnings = stale_result_warnings(
            &original_scenes,
            &condor.scenes,
            &condor.sequence_config,
            "zoning",
        )?;
        if !stale_warnings.is_empty() {
            info!(
                "Zoner: {} additional stale-result warnings; existing results and files were left \
                 untouched",
                stale_warnings.len()
            );
            warnings.extend(stale_warnings);
        }
        // The record the skip check reads: this list has been through every
        // configured zone, whether a scene changed or not.
        for scene in &mut condor.scenes {
            scene.sequence_data.zoner.zoned = true;
        }
        condor.save()?;

        Ok(((), warnings))
    }
}

fn configured_zones(config: &CliSequenceConfig) -> &[Zone] {
    config.zoner.as_ref().map_or(&[], |zoner| &zoner.zones)
}

/// Checks every zone against an input of `num_frames` and that no two zones
/// overlap, which would make the settings a scene takes depend on their order.
pub fn validate_zones(zones: &[Zone], num_frames: usize) -> Result<()> {
    let mut ranges = Vec::with_capacity(zones.len());
    for (index, zone) in zones.iter().enumerate() {
        let (start, end) = zone.resolve(num_frames);
        zone.validate(num_frames).with_context(|| zone_label(index, start, end))?;
        ranges.push((index, start, end));
    }
    ranges.sort_by_key(|(_, start, _)| *start);

    for pair in ranges.windows(2) {
        let (index, start, end) = pair[0];
        let (next_index, next_start, next_end) = pair[1];
        if next_start < end {
            bail!(
                "{} overlaps {}; zones must not overlap",
                zone_label(index, start, end),
                zone_label(next_index, next_start, next_end)
            );
        }
    }

    Ok(())
}

/// Applies the zones to `scenes` in start-frame order.
///
/// Scenes are zoned whole: a scene that shares a frame with a zone takes its
/// settings, and a scene long enough to reach into two zones takes both, the
/// later one last.
pub fn apply_zones(
    zones: &[Zone],
    scenes: &mut Vec<Scene<CliSequenceData>>,
    num_frames: usize,
) -> Vec<anyhow::Error> {
    let mut warnings = Vec::new();
    let initial_scenes = scenes.len();
    let mut total_changes = 0usize;
    info!(
        "Zoner: applying {} zones to {initial_scenes} scenes",
        zones.len()
    );
    let mut ordered = zones
        .iter()
        .enumerate()
        .map(|(index, zone)| {
            let (start, end) = zone.resolve(num_frames);

            (index, zone, start, end)
        })
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(_, _, start, _)| *start);

    for (index, zone, start, end) in ordered {
        let mut zoned = Vec::with_capacity(scenes.len());
        let mut matched = 0usize;
        let mut modified = 0usize;
        let mut split = 0usize;
        let mut added = 0usize;
        let label = zone_label(index, start, end);
        for scene in scenes.drain(..) {
            if scene.end_frame <= start || scene.start_frame >= end {
                zoned.push(scene);
            } else {
                matched += 1;
                let scene_start = scene.start_frame;
                let scene_end = scene.end_frame;
                let original_encoder = scene.encoder.clone();
                let applied = zone.apply(index, scene, &mut warnings);
                let encoder_changed = applied[0].encoder != original_encoder;
                let was_split = applied.len() > 1;
                if encoder_changed || was_split {
                    modified += 1;
                }
                if encoder_changed {
                    info!(
                        "Zoner: {label} modified encoder settings for scene \
                         {scene_start}..{scene_end} ({} -> {})",
                        original_encoder.base(),
                        applied[0].encoder.base()
                    );
                }
                if was_split {
                    split += 1;
                    added += applied.len() - 1;
                    let ranges = applied
                        .iter()
                        .map(|scene| (scene.start_frame, scene.end_frame))
                        .collect::<Vec<_>>();
                    info!(
                        "Zoner: {label} split scene {scene_start}..{scene_end} into {} scenes ({} \
                         added), frame ranges: {ranges:?}",
                        applied.len(),
                        applied.len() - 1
                    );
                }
                zoned.extend(applied);
            }
        }
        *scenes = zoned;
        total_changes += modified;

        info!(
            "Zoner: {label} matched {matched} scenes, modified {modified}, split {split}, added \
             {added} ({} unchanged)",
            matched - modified
        );
        if matched == 0 {
            warnings.push(anyhow::anyhow!(
                "{} matched no scenes",
                zone_label(index, start, end)
            ));
        }
    }

    info!(
        "Zoner finished: {} zones, {total_changes} scene changes across zones, {} scenes added \
         ({initial_scenes} -> {} total), {} warnings",
        zones.len(),
        scenes.len() - initial_scenes,
        scenes.len(),
        warnings.len()
    );

    warnings
}

/// Logs sequence warnings.
fn log_warnings(warnings: &[anyhow::Error]) {
    for warning in warnings {
        warn!("{warning}");
    }
}

#[cfg(test)]
mod tests {
    use andean_condor::models::{
        encoder::{Encoder, EncoderBase, cli_parameter::CLIParameter},
        scene::Scene,
    };

    use super::{
        zone::{ZoneEncoder, ZoneEncoderMerge, ZoneOptions, ZoneOptionsBody},
        *,
    };
    use crate::configuration::CliSequenceData;

    fn scenes() -> Vec<Scene<CliSequenceData>> {
        vec![scene(0, 100), scene(100, 200), scene(200, 400)]
    }

    fn scene(start_frame: usize, end_frame: usize) -> Scene<CliSequenceData> {
        Scene {
            start_frame,
            end_frame,
            sub_scenes: None,
            encoder: Encoder::default_from_base(&EncoderBase::SVTAV1, false),
            sequence_data: CliSequenceData::default(),
        }
    }

    /// A zone replacing the encoder of `start..end` with x265.
    fn replace(start_frame: Option<usize>, end_frame: Option<usize>) -> Zone {
        Zone {
            start_frame,
            end_frame,
            maximum_frames: None,
            encoder: Some(ZoneEncoder::Replace(Encoder::default_from_base(
                &EncoderBase::X265,
                false,
            ))),
        }
    }

    #[test]
    fn every_scene_sharing_a_frame_with_a_zone_is_zoned() {
        let mut scenes = scenes();

        let warnings = apply_zones(&[replace(Some(50), Some(200))], &mut scenes, 400);

        assert!(warnings.is_empty(), "every zone matches");
        assert_eq!(scenes.len(), 3, "no scene is split");
        assert_eq!(
            scenes[0].encoder.base(),
            EncoderBase::X265,
            "frame 50 is inside"
        );
        assert_eq!(
            scenes[1].encoder.base(),
            EncoderBase::X265,
            "frame 199 is inside"
        );
        assert_eq!(
            scenes[2].encoder.base(),
            EncoderBase::SVTAV1,
            "frame 200 is outside the exclusive end"
        );
    }

    #[test]
    fn unset_bounds_reach_the_input_edges() {
        let mut scenes = scenes();

        let warnings = apply_zones(&[replace(None, None)], &mut scenes, 400);

        assert!(warnings.is_empty());
        assert!(
            scenes.iter().all(|scene| scene.encoder.base() == EncoderBase::X265),
            "an unbounded zone covers every scene"
        );
    }

    #[test]
    fn a_zone_matching_nothing_warns() {
        let mut scenes = vec![scene(0, 100)];

        let warnings = apply_zones(&[replace(Some(300), Some(400))], &mut scenes, 400);

        assert_eq!(warnings.len(), 1, "the unmatched zone is reported");
        assert!(
            warnings[0].to_string().contains("matched no scenes"),
            "warning names the problem: {}",
            warnings[0]
        );
        assert_eq!(scenes[0].encoder.base(), EncoderBase::SVTAV1);
    }

    /// A zone merging `crf` into the scenes of `start..end`.
    fn crf(start_frame: usize, end_frame: usize, crf: f64) -> Zone {
        Zone {
            start_frame:    Some(start_frame),
            end_frame:      Some(end_frame),
            maximum_frames: None,
            encoder:        Some(ZoneEncoder::Merge(ZoneEncoderMerge {
                base: Some(EncoderBase::SVTAV1),
                options: Some(ZoneOptions::Merge(ZoneOptionsBody::Map(
                    CLIParameter::new_numbers("--", " ", &[("crf", crf)]),
                ))),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn zones_apply_in_start_frame_order() {
        let mut scenes = vec![scene(0, 300)];

        // Given out of order, the zone that starts later still wins the scene
        // both zones touch.
        let warnings = apply_zones(&[crf(200, 400, 30.0), crf(0, 100, 23.0)], &mut scenes, 400);

        assert!(warnings.is_empty(), "both zones match");
        assert_eq!(
            scenes[0].encoder.parameters().get("crf"),
            Some(&CLIParameter::new_number("--", " ", 30.0)),
            "the later zone is applied last"
        );
    }

    #[test]
    fn a_zone_caps_the_length_of_the_scenes_it_matches() {
        let mut scenes = scenes();
        let zone = Zone {
            start_frame:    Some(200),
            end_frame:      None,
            maximum_frames: Some(100),
            encoder:        None,
        };

        let warnings = apply_zones(&[zone], &mut scenes, 400);

        assert!(warnings.is_empty());
        assert_eq!(scenes.len(), 4, "the 200-frame scene becomes two");
        assert_eq!((scenes[3].start_frame, scenes[3].end_frame), (300, 400));
    }

    #[test]
    fn validation_rejects_overlapping_zones() {
        let zones = vec![replace(Some(0), Some(250)), replace(Some(200), Some(300))];

        let error = validate_zones(&zones, 400).expect_err("the zones overlap");

        assert!(
            error.to_string().contains("zones must not overlap"),
            "error explains the overlap: {error}"
        );
    }

    #[test]
    fn validation_accepts_adjacent_and_disjoint_zones() {
        let adjacent = vec![replace(Some(0), Some(200)), replace(Some(200), Some(300))];
        assert!(
            validate_zones(&adjacent, 400).is_ok(),
            "touching ranges do not overlap"
        );

        let disjoint = vec![replace(Some(300), Some(400)), replace(Some(0), Some(100))];
        assert!(
            validate_zones(&disjoint, 400).is_ok(),
            "out-of-order zones are fine"
        );
    }

    #[test]
    fn validation_names_the_invalid_zone() {
        let zones = vec![replace(Some(0), Some(200)), replace(Some(100), Some(100))];

        let error = validate_zones(&zones, 400).expect_err("the second zone is empty");

        assert!(
            error.to_string().starts_with("zone 2"),
            "error names the zone: {error}"
        );
    }

    #[test]
    fn no_zones_change_nothing() {
        let mut scenes = scenes();

        let warnings = apply_zones(&[], &mut scenes, 400);

        assert!(warnings.is_empty());
        assert_eq!(scenes.len(), 3);
    }
}
