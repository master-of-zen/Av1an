use std::path::{Path, PathBuf};

use andean_condor::{
    models::input::{
        ImportMethod,
        Input as InputModel,
        VapourSynthImportMethod,
        VapourSynthScriptSource,
    },
    vapoursynth::vapoursynth_filters::VapourSynthFilter,
};
use anyhow::{Result, bail};
use tracing::{debug, error, warn};

use crate::{
    DEFAULT_CONFIG_PATH,
    commands::{
        CondorCliError,
        DecoderMethod,
        config::{InputOverride, Toggle},
    },
    configuration::{ConfigError, Configuration},
};

pub mod benchmarker;
pub mod concatenate;
pub mod config;
pub mod detect_noise;
pub mod detect_scenes;
pub mod encode;
pub mod init;
pub mod optimize_bitrate;
pub mod quality_check;
pub mod scale_noise;
pub mod scale_speed;
pub mod scenes;
pub mod start;
pub mod target_quality;
pub mod zone;

pub fn load_configuration(config_path: Option<&Path>) -> Result<(Configuration, PathBuf)> {
    if let Some(config_path) = config_path
        && !config_path.exists()
    {
        let err = CondorCliError::ConfigFileNotFound(config_path.to_path_buf());
        error!("{}", err);
        bail!(err);
    }
    let config_path =
        path_abs::PathAbs::new(config_path.unwrap_or_else(|| Path::new(DEFAULT_CONFIG_PATH)))?
            .as_path()
            .to_path_buf();
    if !config_path.exists() {
        let err = CondorCliError::NoConfig;
        error!("{}", err);
        bail!(err);
    }

    let configuration = {
        debug!("Loading existing configuration");
        match Configuration::load(&config_path) {
            Ok(config) => config.expect("Config should exist"),
            Err(err) => match err {
                ConfigError::Load(path) => {
                    let err = CondorCliError::ConfigLoadError(path);
                    error!("{}", err);
                    bail!(err);
                },
                ConfigError::Parse(err) => {
                    let err = CondorCliError::ConfigParseError(err);
                    error!("{}", err);
                    bail!(err);
                },
                ConfigError::Serialize(err) => {
                    let err = CondorCliError::ConfigSerializeError(err);
                    error!("{}", err);
                    bail!(err);
                },
                ConfigError::Save(err) => {
                    let err = CondorCliError::ConfigSaveError(err);
                    error!("{}", err);
                    bail!(err);
                },
            },
        }
    };

    Ok((configuration, config_path))
}

pub fn configure_temp(configuration: &mut Configuration, temp_path: Option<&Path>) -> Result<()> {
    if let Some(temp_path) = temp_path {
        configuration.temp = path_abs::PathAbs::new(temp_path)?.as_path().to_path_buf();
    }

    Ok(())
}

pub fn configure_input(
    configuration: &Configuration,
    existing_input: &InputModel,
    input_path: Option<&Path>,
    decoder: Option<&DecoderMethod>,
    vs_args: Option<&[String]>,
    index: Option<u8>,
    // cache_path: Option<&Path>,
) -> Result<InputModel> {
    let (existing_input_path, existing_decoder, existing_vs_args, existing_index) =
        match existing_input {
            InputModel::Video {
                path,
                import_method,
                ..
            } => match import_method {
                ImportMethod::FFMS2 {
                    index,
                } => (path, Some(DecoderMethod::FFMS2), None, *index),
            },
            InputModel::VapourSynth {
                path,
                import_method,
                ..
            } => match import_method {
                VapourSynthImportMethod::LSMASHWorks {
                    index,
                } => (path, Some(DecoderMethod::LSMASHWorks), None, *index),
                VapourSynthImportMethod::DGDecNV {
                    ..
                } => (path, Some(DecoderMethod::DGDecodeNV), None, None),
                VapourSynthImportMethod::FFMS2 {
                    index,
                } => (path, Some(DecoderMethod::VSFFMS2), None, *index),
                VapourSynthImportMethod::BestSource {
                    index,
                } => (path, Some(DecoderMethod::BestSource), None, *index),
            },
            InputModel::VapourSynthScript {
                source,
                variables,
                index,
                ..
            } => match source {
                VapourSynthScriptSource::Path(path) => {
                    (path, None, Some(variables.clone()), Some(*index))
                },
                VapourSynthScriptSource::Text(_) => (
                    &configuration.input,
                    None,
                    Some(variables.clone()),
                    Some(*index),
                ),
            },
        };

    let existing_vs_args: Option<Vec<String>> = existing_vs_args
        .map(|args| args.iter().map(|(key, value)| format!("{}={}", key, value)).collect());

    // Only a script has this knob, and the rebuild below cannot know it, so it
    // is read separately rather than folded into the tuple above.
    let existing_stream_concurrently = match existing_input {
        InputModel::VapourSynthScript {
            stream_concurrently,
            ..
        } => Some(*stream_concurrently),
        InputModel::Video {
            ..
        }
        | InputModel::VapourSynth {
            ..
        } => None,
    };

    let mut input = Configuration::new_input_model(
        path_abs::PathAbs::new(input_path.unwrap_or(existing_input_path))?.as_path(),
        decoder.or(existing_decoder.as_ref()),
        vs_args.or(existing_vs_args.as_deref()),
        index.or(existing_index),
        // cache_path: None, // TODO: Support Cache Path
    )?;
    warn_unused_vs_args(&input, vs_args);
    // The input is rebuilt from its path and decoder, which drops its filters,
    // so carry them over.
    input.adopt_filters(existing_input);
    if let Some(stream_concurrently) = existing_stream_concurrently
        && let InputModel::VapourSynthScript {
            stream_concurrently: rebuilt,
            ..
        } = &mut input
    {
        *rebuilt = stream_concurrently;
    }
    Ok(input)
}

/// A human-readable description of an input's resolved kind, for diagnostics.
pub fn input_kind_label(input: &InputModel) -> &'static str {
    match input {
        InputModel::Video {
            import_method: ImportMethod::FFMS2 {
                ..
            },
            ..
        } => "native FFMS2 video input",
        InputModel::VapourSynth {
            import_method: VapourSynthImportMethod::BestSource {
                ..
            },
            ..
        } => "BestSource video input",
        InputModel::VapourSynth {
            import_method: VapourSynthImportMethod::FFMS2 {
                ..
            },
            ..
        } => "VapourSynth FFMS2 video input",
        InputModel::VapourSynth {
            import_method: VapourSynthImportMethod::LSMASHWorks {
                ..
            },
            ..
        } => "L-SMASH-Works video input",
        InputModel::VapourSynth {
            import_method: VapourSynthImportMethod::DGDecNV {
                ..
            },
            ..
        } => "DGDecNV video input",
        InputModel::VapourSynthScript {
            ..
        } => "VapourSynth script input",
    }
}

/// Warns that a script-only flag was ignored for an input that cannot use it.
///
/// VapourSynth script arguments and concurrent streaming only reach a
/// `.vpy`/`.py` input; a video or an indexed source has no script environment
/// to pass them to, so the flag would otherwise be dropped without a word.
pub fn warn_script_flag_ignored(flag: &str, input: &InputModel) {
    let kind = input_kind_label(input);
    warn!(
        "{flag} applies only to VapourSynth script (.vpy/.py) inputs and was ignored for this \
         {kind}; drop {flag} or use a script input"
    );
}

/// Warns when `--vs-args` was given for an input that cannot use it.
pub fn warn_unused_vs_args(input: &InputModel, vs_args: Option<&[String]>) {
    let given = vs_args.is_some_and(|args| !args.is_empty());
    if given && !matches!(input, InputModel::VapourSynthScript { .. }) {
        warn_script_flag_ignored("--vs-args", input);
    }
}

/// Applies `--stream-concurrently` to `input`, or warns when `input` is not a
/// VapourSynth script.
pub fn apply_stream_concurrently(input: &mut InputModel, toggle: Option<Toggle>) {
    let Some(toggle) = toggle else {
        return;
    };
    if let InputModel::VapourSynthScript {
        stream_concurrently,
        ..
    } = input
    {
        *stream_concurrently = toggle.enabled();
    } else {
        warn_script_flag_ignored("--stream-concurrently", input);
    }
}

/// Warns when `path` does not exist yet on disk.
///
/// Editing a configuration does not open the file immediately, so a missing
/// path is accepted with a warning rather than rejected.
pub fn warn_missing_path(what: &str, path: &Path) {
    if !path.exists() {
        warn!(
            "{what} `{}` does not exist; it will be needed when the step runs",
            path.display()
        );
    }
}

/// Builds the override input an [`InputOverride`] describes, warning if a
/// supplied path does not exist yet.
///
/// The caller has already rejected an override that sets no flag
/// ([`ensure_override_requested`](super::config::ensure_override_requested)),
/// so this always builds one.
pub fn apply_input_override(
    configuration: &Configuration,
    existing_input: &InputModel,
    args: &InputOverride,
) -> Result<InputModel> {
    if let Some(path) = args.path.as_deref() {
        warn_missing_path("input path", path);
    }
    let mut input = configure_input(
        configuration,
        existing_input,
        args.path.as_deref(),
        args.decoder.as_ref(),
        args.vs_args.as_deref(),
        None,
    )?;
    if let Some(filters) = args.filters.as_deref() {
        apply_input_filters(&mut input, filters);
    }
    apply_stream_concurrently(&mut input, args.stream_concurrently);
    Ok(input)
}

/// Replaces `input`'s filters, warning about any its decoder cannot run.
pub fn apply_input_filters(input: &mut InputModel, filters: &[VapourSynthFilter]) {
    let unsupported = InputModel::unsupported_filters(filters);
    input.set_filters(filters.to_vec());
    for filter in unsupported {
        warn!(
            "{filter} needs VapourSynth and was dropped: a native FFMS2 input can only convert \
             bit depth, chroma and resolution"
        );
    }
}

/// Builds a sequence's override input, or `None` when no override flag is set.
///
/// Filters count as an override, so passing only filters still gives the
/// sequence its own input. Writing them to the main input instead would replace
/// its filters, losing the conversions the encoder depends on.
pub fn configure_override_input(
    configuration: &Configuration,
    existing_input: &InputModel,
    input_path: Option<&Path>,
    decoder: Option<&DecoderMethod>,
    filters: Option<&[VapourSynthFilter]>,
    vs_args: Option<&[String]>,
) -> Result<Option<InputModel>> {
    if input_path.is_none() && decoder.is_none() && filters.is_none() && vs_args.is_none() {
        return Ok(None);
    }

    let mut input = configure_input(
        configuration,
        existing_input,
        input_path,
        decoder,
        vs_args,
        None,
    )?;
    if let Some(filters) = filters {
        apply_input_filters(&mut input, filters);
    }

    Ok(Some(input))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, collections::HashMap};

    use super::*;

    fn script_input(stream_concurrently: bool) -> InputModel {
        InputModel::VapourSynthScript {
            source: VapourSynthScriptSource::Path("script.vpy".into()),
            variables: HashMap::new(),
            index: 0,
            filters: Vec::new(),
            stream_concurrently,
        }
    }

    fn built_stream_concurrently(input: &InputModel) -> bool {
        let InputModel::VapourSynthScript {
            stream_concurrently,
            ..
        } = input
        else {
            panic!("expected a VapourSynthScript input");
        };
        *stream_concurrently
    }

    /// Rebuilding the input for a CLI override must not silently re-enable
    /// concurrent streaming, since the user turned it off on purpose.
    #[test]
    fn rebuilding_a_script_input_keeps_concurrent_streaming_off() {
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let script = temp.path().join("script.vpy");
        // `BlankClip` needs no source file, but does produce a real output
        // node, which `Configuration::new` requires.
        std::fs::write(
            &script,
            "import vapoursynth as vs\ncore = vs.core\nclip = core.std.BlankClip(width=16, \
             height=16, format=vs.YUV420P8)\nclip.set_output(0)\n",
        )
        .expect("script file writes to disk");

        let configuration =
            Configuration::new(&script, &PathBuf::from("out.mkv"), None, None, None)
                .expect("a configuration can be built from a script");

        let rebuilt = configure_input(&configuration, &script_input(false), None, None, None, None)
            .expect("the input can be rebuilt");

        assert!(
            !built_stream_concurrently(&rebuilt),
            "stream_concurrently: false must survive the rebuild"
        );
    }

    #[test]
    fn invalid_json_reports_parse_error() {
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = temp.path().join("condor.json");
        std::fs::write(&config_path, "{ invalid JSON").expect("config file writes to disk");

        let error = load_configuration(Some(&config_path))
            .expect_err("invalid JSON should fail to load")
            .downcast::<CondorCliError>()
            .expect("load_configuration error should be CondorCliError");
        let error_string = format!("{error:#}");

        assert_matches!(
            error,
            CondorCliError::ConfigParseError(_),
            "invalid JSON should be ConfigParseError"
        );
        assert!(
            error_string.contains("line"),
            "error should include the JSON parse detail, got: {error_string}"
        );
    }
}
