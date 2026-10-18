use std::path::{Path, PathBuf};

use andean_condor::models::sequence::scene_concatenator::{
    ConcatMethod,
    FfmpegConfig,
    MkvmergeConfig,
    SceneConcatenatorConfig,
};
use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::{
    commands::{
        ConcatenationMethod,
        config::{ConfigAction, ConfigCommand, PropertyChange},
        handlers::{config::edit, configure_temp, load_configuration},
    },
    configuration::Configuration,
};

#[allow(clippy::too_many_arguments)]
pub fn concatenate_handler(
    config_path: Option<&Path>,
    temp_path: Option<&Path>,
    method: Option<&ConcatenationMethod>,
) -> Result<(Configuration, PathBuf)> {
    let (mut configuration, config_path) = load_configuration(config_path)?;

    configure_temp(&mut configuration, temp_path)?;
    configure_concatenate(&mut configuration, method)?;

    configuration.save(&config_path)?;

    Ok((configuration, config_path))
}

pub fn configure_concatenate(
    configuration: &mut Configuration,
    method: Option<&ConcatenationMethod>,
) -> Result<()> {
    if let Some(concat) = method {
        // Switching methods keeps that method's existing settings; changing to
        // a different method starts from its defaults.
        let existing = configuration.condor.sequence_config.scene_concatenator.method.clone();
        let concat = match concat {
            ConcatenationMethod::MkvMerge => match existing {
                ConcatMethod::MKVMerge(config) => ConcatMethod::MKVMerge(config),
                _ => ConcatMethod::MKVMerge(MkvmergeConfig::default()),
            },
            ConcatenationMethod::FFmpeg => match existing {
                ConcatMethod::FFmpeg(config) => ConcatMethod::FFmpeg(config),
                _ => ConcatMethod::FFmpeg(FfmpegConfig::default()),
            },
            ConcatenationMethod::Ivf => ConcatMethod::Ivf,
        };
        configuration.condor.sequence_config.scene_concatenator.method = concat;
    }

    Ok(())
}

/// `info` and `reset` keys of `sequence_config.scene_concatenator`.
#[derive(Debug, Clone, Subcommand)]
pub enum ConcatenateKey {
    /// The tool used to join the encoded scenes.
    Method,
    /// The file the concatenated video is written to.
    Output,
    /// The directory the encoded scenes are read from.
    ScenesDirectory,
}

/// Settings `condor concatenate set` can change.
#[derive(Debug, Clone, Subcommand)]
pub enum ConcatenateSet {
    /// The tool used to join the encoded scenes.
    ///
    /// Keeping the current tool keeps its settings; switching to another tool
    /// starts from that tool's defaults.
    Method {
        #[arg(value_name = "Method")]
        method: ConcatenationMethod,
    },
    /// The file the concatenated video is written to.
    Output {
        #[arg(value_name = "Path")]
        path: PathBuf,
    },
    /// The directory the encoded scenes are read from.
    ScenesDirectory {
        #[arg(value_name = "Path")]
        path: PathBuf,
    },
}

/// The action triple attached to `condor concatenate`.
pub type ConcatenateAction = ConfigAction<ConcatenateKey, ConcatenateSet>;

/// The `condor concatenate` configuration section.
pub struct ConcatenateCrud;

impl ConfigCommand for ConcatenateCrud {
    type Key = ConcatenateKey;
    type Set = ConcatenateSet;

    fn subject() -> &'static str {
        "Concatenation settings"
    }

    fn render(configuration: &Configuration, key: Option<&Self::Key>) -> Result<Value> {
        let concat = &configuration.condor.sequence_config.scene_concatenator;
        Ok(match key {
            None => serde_json::to_value(concat)?,
            Some(ConcatenateKey::Method) => serde_json::to_value(&concat.method)?,
            Some(ConcatenateKey::Output) => serde_json::to_value(&concat.output)?,
            Some(ConcatenateKey::ScenesDirectory) => {
                serde_json::to_value(&concat.scenes_directory)?
            },
        })
    }

    fn apply(configuration: &mut Configuration, set: &Self::Set) -> Result<PropertyChange> {
        match set {
            ConcatenateSet::Method {
                method,
            } => {
                configure_concatenate(configuration, Some(method))?;
                PropertyChange::new(
                    "method",
                    &configuration.condor.sequence_config.scene_concatenator.method,
                )
            },
            ConcatenateSet::Output {
                path: output,
            } => {
                configuration.condor.sequence_config.scene_concatenator.output =
                    Some(output.clone());
                PropertyChange::new(
                    "output",
                    &configuration.condor.sequence_config.scene_concatenator.output,
                )
            },
            ConcatenateSet::ScenesDirectory {
                path: directory,
            } => {
                configuration.condor.sequence_config.scene_concatenator.scenes_directory =
                    directory.clone();
                PropertyChange::new(
                    "scenes-directory",
                    &configuration.condor.sequence_config.scene_concatenator.scenes_directory,
                )
            },
        }
    }

    fn restore(
        configuration: &mut Configuration,
        key: Option<&Self::Key>,
    ) -> Result<Option<PropertyChange>> {
        let defaults = SceneConcatenatorConfig::default();
        let concat = &mut configuration.condor.sequence_config.scene_concatenator;
        let (property, value) = match key {
            // `scenes_directory` is derived from --temp, so resetting the whole
            // section restores the method and output but keeps it in place.
            None => {
                concat.method = defaults.method;
                concat.output = None;
                ("section", serde_json::to_value(&*concat)?)
            },
            Some(ConcatenateKey::Method) => {
                concat.method = defaults.method;
                ("method", serde_json::to_value(&concat.method)?)
            },
            Some(ConcatenateKey::Output) => {
                concat.output = None;
                ("output", serde_json::to_value(&concat.output)?)
            },
            Some(ConcatenateKey::ScenesDirectory) => {
                concat.scenes_directory = defaults.scenes_directory;
                (
                    "scenes-directory",
                    serde_json::to_value(&concat.scenes_directory)?,
                )
            },
        };
        Ok(Some(PropertyChange {
            property: property.to_owned(),
            value,
        }))
    }
}

/// `condor concatenate info|set|reset`.
pub fn config(config_path: Option<&Path>, action: &ConcatenateAction) -> Result<()> {
    edit::<ConcatenateCrud>(config_path, action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::handlers::init::init_handler,
        test_helpers::{check_basic_config, default_config, get_test_video},
        utils::hash_path::hash_path,
    };

    #[test]
    fn detect_scenes_default_config() {
        let test_video = get_test_video();
        let input_abs = path_abs::PathAbs::new(&test_video.path)
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let temp = tempfile::tempdir().expect("temp directory");
        let temp_abs = path_abs::PathAbs::new(temp.path().join(hash_path(&input_abs)))
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let output = temp.path().join("out.mkv");
        let config_path = temp.path().join("condor.json");
        let config_path_abs = path_abs::PathAbs::new(&config_path)
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();

        let expected_config = default_config(&test_video, &output, &temp_abs);

        init_handler(
            // Simulate default directory to avoid changing CWD
            Some(&config_path),
            Some(&temp.path().join(hash_path(&input_abs))),
            &test_video.path,
            &output,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("init_handler should succeed");
        let (config, found_config_path) = concatenate_handler(
            // Simulate default directory to avoid changing CWD
            Some(&config_path),
            None,
            None,
        )
        .expect("concatenate_handler should succeed");

        assert_eq!(
            found_config_path,
            config_path_abs,
            "config path is {}",
            config_path_abs.display()
        );
        check_basic_config(&config, &expected_config);
        assert!(config.condor.scenes.is_empty(), "scenes is empty");
    }

    #[test]
    fn concatenate_custom_config() {
        let test_video = get_test_video();
        let input_abs = path_abs::PathAbs::new(&test_video.path)
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let temp = tempfile::tempdir().expect("temp directory");
        let temp_abs = path_abs::PathAbs::new(temp.path().join(hash_path(&input_abs)))
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let output = temp.path().join("out.mkv");
        let config_path = temp.path().join("condor.json");
        let config_path_abs = path_abs::PathAbs::new(&config_path)
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();

        let mut expected_config = default_config(&test_video, &output, &temp_abs);
        expected_config.condor.sequence_config.scene_concatenator.method =
            ConcatMethod::FFmpeg(Default::default());
        expected_config.condor.sequence_config.scene_concatenator.scenes_directory =
            temp_abs.join("scenes");
        // immutable shadow
        let expected_config = expected_config;

        init_handler(
            // Simulate default directory to avoid changing CWD
            Some(&config_path),
            Some(&temp.path().join(hash_path(&input_abs))),
            &test_video.path,
            &output,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("init_handler should succeed");
        let (config, found_config_path) = concatenate_handler(
            // Simulate default directory to avoid changing CWD
            Some(&config_path),
            Some(&temp.path().join(hash_path(&input_abs))),
            Some(&ConcatenationMethod::FFmpeg),
        )
        .expect("concatanate_handler should succeed");

        assert_eq!(
            found_config_path,
            config_path_abs,
            "config path is {}",
            config_path_abs.display()
        );
        check_basic_config(&config, &expected_config);
        assert!(config.condor.scenes.is_empty(), "scenes is empty");
    }
}
