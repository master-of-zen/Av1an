//! How a `condor scenes` run reports itself: selection rendering, change
//! summaries, staleness warnings and saving.

use std::path::Path;

use andean_condor::models::scene::Scene;
use anyhow::Result;
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{SUBJECT, action::ScenesKey};
use crate::{
    commands::{config::PropertyChange, handlers::config::warn_stale},
    configuration::{CliSequenceData, Configuration},
    scenes::{SceneSelection, format_indices, require_scene, stale_result_warnings},
};

/// Prints how many scenes were addressed and the frames they cover, so the
/// indices named can be checked at a glance.
///
/// The summary is reported after the output content and covers only what was
/// addressed: the scenes printed, changed, removed or added. The span
/// runs from the lowest start frame to the highest end frame: edited scenes may
/// leave the list unsorted, so the ends are searched rather than taken from the
/// last scene.
pub(super) fn report_totals(scenes: &[Scene<CliSequenceData>], indices: &[usize]) {
    let Some(first) = indices.iter().map(|&index| scenes[index].start_frame).min() else {
        info!("Scenes: 0");
        return;
    };
    let last = indices
        .iter()
        .map(|&index| scenes[index].end_frame)
        .max()
        .expect("an addressed scene has an end frame");
    info!("Scenes: {} Frames: {first}-{last}", indices.len());
}

/// Saves the run's edits: first the per-scene warnings the change can cause,
/// then the file, then the generic warning that saved results may be stale.
pub(super) fn save_edit(
    configuration: &Configuration,
    path: &Path,
    before: &[Scene<CliSequenceData>],
) -> Result<()> {
    report_stale(configuration, before)?;
    configuration.save(path)?;
    warn_stale(SUBJECT);

    Ok(())
}

/// Reports the saved results and files the scene change can stale, beside the
/// generic staleness warning the edit prints.
fn report_stale(configuration: &Configuration, before: &[Scene<CliSequenceData>]) -> Result<()> {
    let warnings = stale_result_warnings(
        before,
        &configuration.condor.scenes,
        &configuration.condor.sequence_config,
        "scene edits",
    )?;
    for warning in warnings {
        warn!("{warning}");
    }

    Ok(())
}

/// The scenes `index` addresses: the whole list when no index was given.
pub(super) fn resolve(
    scenes: &[Scene<CliSequenceData>],
    index: Option<&SceneSelection>,
) -> Result<Vec<usize>> {
    Ok(match index {
        None => (0..scenes.len()).collect(),
        Some(index) => index.resolve(scenes.len())?,
    })
}

/// How an addressed set of scenes is named in reports.
///
/// ## Examples:
/// * `scene 2`
/// * `scenes 0-4`
pub(super) fn label(indices: &[usize]) -> String {
    let rendered = format_indices(indices);
    if indices.len() == 1 {
        format!("scene {rendered}")
    } else {
        format!("scenes {rendered}")
    }
}

/// What one `set` or `reset` did to every addressed scene: a single scene
/// reports its value as it is, several report theirs as an array. `name` is the
/// property that changed, or `None` when the label names it already.
pub(super) fn aggregate(label: &str, name: Option<&str>, values: Vec<Value>) -> PropertyChange {
    let value = if values.len() == 1 {
        values.into_iter().next().expect("one value")
    } else {
        Value::Array(values)
    };
    let property = name.map_or_else(|| label.to_owned(), |name| format!("{label} {name}"));
    PropertyChange {
        property,
        value,
    }
}

/// The half-open frames each addressed scene covers, as `START-END` pairs:
/// `121-240, 480-528`. A delete reports the scenes it removed this way, which
/// is all of them a later read could still find.
pub(super) fn frame_summary(scenes: &[Scene<CliSequenceData>], indices: &[usize]) -> String {
    indices
        .iter()
        .filter_map(|&index| scenes.get(index))
        .map(|scene| format!("{}-{}", scene.start_frame, scene.end_frame))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One scene's frame range as `info frames` prints it.
pub(super) fn frame_range(scene: &Scene<CliSequenceData>) -> Value {
    json!({
        "start_frame": scene.start_frame,
        "end_frame":   scene.end_frame,
    })
}

/// The value `info` prints for the addressed scenes: the scenes themselves, or
/// `key` of each of them. A single addressed scene is reported on its own;
/// anything else is an array.
pub(super) fn selection_value(
    scenes: &[Scene<CliSequenceData>],
    indices: &[usize],
    key: Option<&ScenesKey>,
    single: bool,
) -> Result<Value> {
    let values = indices
        .iter()
        .map(|&index| {
            let scene = require_scene(scenes, index)?;

            Ok(match key {
                None => serde_json::to_value(scene)?,
                Some(key) => render_key(scene, *key)?,
            })
        })
        .collect::<Result<Vec<Value>>>()?;

    Ok(if single {
        values.into_iter().next().expect("one value")
    } else {
        Value::Array(values)
    })
}

/// The value one `info` key reads out of a scene.
fn render_key(scene: &Scene<CliSequenceData>, key: ScenesKey) -> Result<Value> {
    let data = &scene.sequence_data;

    Ok(match key {
        ScenesKey::Frames => frame_range(scene),
        ScenesKey::Encoder => serde_json::to_value(&scene.encoder)?,
        ScenesKey::SubScenes => serde_json::to_value(&scene.sub_scenes)?,
        ScenesKey::SceneDetection => serde_json::to_value(&data.scene_detection)?,
        ScenesKey::NoiseDetection => serde_json::to_value(&data.noise_detection)?,
        ScenesKey::NoiseScaling => serde_json::to_value(&data.noise_scaling)?,
        ScenesKey::ParallelEncoder => serde_json::to_value(&data.parallel_encoder)?,
        ScenesKey::TargetQuality => serde_json::to_value(&data.target_quality)?,
        ScenesKey::QualityCheck => serde_json::to_value(&data.quality_check)?,
        ScenesKey::Zoner => serde_json::to_value(&data.zoner)?,
    })
}
