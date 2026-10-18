#[path = "common.rs"]
mod common;
#[path = "config_helpers.rs"]
mod config_helpers;

use andean_condor::models::{
    encoder::{EncoderBase, cli_parameter::CLIParameter},
    sequence::target_quality::{
        TargetQualityConfig,
        types::{QualityMetric, TargetQualityProbing},
    },
};
use california_condor::{
    commands::handlers::load_configuration,
    test_helpers::*,
    utils::{hash_path::hash_path, parameter_parser::EncoderParamsParser},
};
use common::condor_cmd;
use config_helpers::{init_config, load};

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use andean_condor::{
        ffmpeg::FFPixelFormat,
        models::{
            input::{Input, VapourSynthImportMethod},
            sequence::target_quality::types::{
                DEFAULT_SSIMULACRA2_TARGET_RANGE,
                DEFAULT_XPSNR_TARGET_RANGE,
                ProbeStatistic,
                ProbeStrategy,
                QualityMetric as MetricModel,
                SubsetProbeLength,
                SubsetProbePosition,
            },
        },
        vapoursynth::vapoursynth_filters::VapourSynthFilter,
    };
    use serial_test::serial;

    use super::*;

    #[serial]
    #[test]
    fn with_custom_options() {
        if !ffmpeg_is_available() {
            return;
        }
        let test_video = get_test_video();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let output = temp.path().join("out.mkv");
        let input_abs = path_abs::PathAbs::new(test_video.path.clone())
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let temp_abs = path_abs::PathAbs::new(temp.path().join(hash_path(&input_abs)))
            .expect("path_abs should succeed")
            .as_path()
            .to_path_buf();
        let config_path = temp.path().join("condor.json");
        let mut tq_encoder_params = EncoderBase::SVTAV1.default_parameters();
        tq_encoder_params.extend(CLIParameter::new_numbers("--", " ", &[
            ("preset", 8.0),
            ("tune", 1.0),
        ]));

        // Mock an existing config file with scenes
        let mut config = default_config(&test_video, &output, &temp_abs);
        // Allow testing with SVT Essential (does not support 8-bit)
        if let Input::VapourSynth {
            filters, ..
        } = &mut config.condor.input
        {
            *filters = vec![VapourSynthFilter::Resize {
                scaler: None,
                width:  None,
                height: None,
                format: Some(FFPixelFormat::YUV420P10LE),
            }];
        }
        config.condor.encoder.parameters_mut().insert(
            "preset".to_owned(),
            CLIParameter::new_number("--", " ", 6.0),
        );
        config.condor.scenes = test_video.mock_scenes(&config.condor.encoder);
        config.save(&config_path).expect("configuration save should succeed");

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args([
                "target-quality",
                "--input",
                path_str(&test_video.path),
                "--decoder",
                "vs-ffms2",
                "--filters",
                "resize:format=yuv420p10le;",
                "--metric",
                "xpsnr",
                "--target",
                "45",
                "--min-q",
                "20",
                "--max-q",
                "40",
                "--profile",
                "fast",
                "--params",
                "--preset 8 --tune 1",
            ])
            .assert()
            .success();

        let mut expected_config = config.clone();
        expected_config.condor.sequence_config.target_quality = Some(TargetQualityConfig {
            metric: QualityMetric::XPSNR {
                target_range: (44.0, 46.0),
                resolution:   None,
            },
            quantizer_range: (20, 40),
            input: Some(Input::VapourSynth {
                path:          input_abs,
                import_method: VapourSynthImportMethod::FFMS2 {
                    index: None
                },
                cache_path:    None,
                filters:       config.condor.input.vapoursynth_filters().to_vec(),
            }),
            probing: TargetQualityProbing {
                encoder_options: Some(tq_encoder_params),
                strategy:        ProbeStrategy::Subset {
                    position: SubsetProbePosition::Middle,
                    length:   SubsetProbeLength::Frames(11),
                },
                statistic:       ProbeStatistic::Mean,
            },
            ..Default::default()
        });
        // immutable shadow
        let expected_config = expected_config;

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        check_basic_config(&config, &expected_config);
        assert_eq!(
            config.condor.scenes.len(),
            test_video.scenes.len(),
            "scenes contains {} scenes",
            test_video.scenes.len()
        );
        let quantizer_range = config
            .condor
            .sequence_config
            .target_quality
            .as_ref()
            .expect("Target Quality sequence_config should exist")
            .quantizer_range;
        config.condor.scenes.iter().enumerate().for_each(|(index, scene)| {
            assert!(
                !scene.sequence_data.target_quality.passes.is_empty(),
                "scene {} should have Target Quality passes",
                index
            );
            let maximum_probes = config
                .condor
                .sequence_config
                .target_quality
                .as_ref()
                .expect("Target Quality sequence_config should exist")
                .maximum_probes;
            assert!(
                scene.sequence_data.target_quality.passes.len() <= maximum_probes as usize,
                "scene {} should have at most {} Target Quality passes",
                index,
                maximum_probes
            );
            let crf = scene.encoder.parameters().get("crf").expect("crf should exist");
            assert_matches!(
                crf,
                CLIParameter::Number { .. },
                "scene {} crf should be a number",
                index
            );
            let crf_value = match crf {
                CLIParameter::Number {
                    value, ..
                } => *value,
                _ => panic!("scene {} crf should be a number", index),
            };
            assert!(
                crf_value >= quantizer_range.0 as f64 && crf_value <= quantizer_range.1 as f64,
                "scene {} crf {} should be within {} and {} CRF",
                index,
                crf_value,
                quantizer_range.0,
                quantizer_range.1
            );
        });
    }

    /// `target-quality` materializes its optional section, folds the target
    /// into the metric's range, and `reset` restores the default range.
    #[serial]
    #[test]
    fn target_quality_set_target_metric_and_range() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args(["target-quality", "set", "target", "90.5"])
            .assert()
            .success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality was materialized");
        assert_eq!(
            quality.metric.target_range(),
            (89.5, 91.5),
            "the target is folded into the metric range"
        );

        condor_cmd(&temp).args(["target-quality", "reset", "target"]).assert().success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality exists");
        assert!(
            matches!(quality.metric, MetricModel::SSIMULACRA2 { .. }),
            "the metric is unchanged: {:?}",
            quality.metric
        );
        assert_eq!(
            quality.metric.target_range(),
            DEFAULT_SSIMULACRA2_TARGET_RANGE,
            "reset restores the metric's default range"
        );

        condor_cmd(&temp)
            .args(["target-quality", "set", "metric", "xpsnr"])
            .assert()
            .success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality exists");
        assert!(
            matches!(quality.metric, MetricModel::XPSNR { .. }),
            "the metric switched: {:?}",
            quality.metric
        );
        assert_eq!(
            quality.metric.target_range(),
            DEFAULT_XPSNR_TARGET_RANGE,
            "switching the metric resets its target range"
        );

        condor_cmd(&temp)
            .args(["target-quality", "set", "quantizer-range", "30-10"])
            .assert()
            .failure();
        condor_cmd(&temp)
            .args(["target-quality", "set", "quantizer-range", "18-30"])
            .assert()
            .success();
        // Out of the documented probe count.
        condor_cmd(&temp)
            .args(["target-quality", "set", "maximum-probes", "0"])
            .assert()
            .failure();
        condor_cmd(&temp)
            .args(["target-quality", "set", "maximum-probes", "12"])
            .assert()
            .success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality exists");
        assert_eq!(quality.quantizer_range, (18, 30), "range is set");
        assert_eq!(quality.maximum_probes, 12, "probes are set");

        let output = condor_cmd(&temp)
            .args(["target-quality", "info", "target"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let target: serde_json::Value =
            serde_json::from_slice(&output).expect("info prints valid JSON");
        assert_eq!(
            target,
            serde_json::to_value(DEFAULT_XPSNR_TARGET_RANGE).expect("range serializes"),
            "info target reports the metric's target range: {target}"
        );
    }

    /// `target-quality` shares the probing strategy and statistic shapes with
    /// `quality-check` (which pins their validation); here it keeps the encoder
    /// options the user does not restate, and restores them on reset.
    #[serial]
    #[test]
    fn target_quality_set_probing_strategy_statistic_and_params() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args(["target-quality", "set", "statistic", "percentile", "--percentile", "90"])
            .assert()
            .success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality was materialized");
        assert_eq!(
            quality.probing.statistic,
            ProbeStatistic::Percentile(90.0),
            "the probe statistic is set"
        );

        // The params string is the run flag's, and omitted options stay.
        condor_cmd(&temp)
            .args(["target-quality", "set", "params", "crf=16"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["target-quality", "set", "params", "tune=0"])
            .assert()
            .success();
        let config = load(&config_path);
        let quality = config.condor.sequence_config.target_quality.expect("target quality exists");
        let mut expected = config.condor.encoder.base().default_parameters();
        expected.extend(EncoderParamsParser::parse_string("crf=16").expect("parses"));
        expected.extend(EncoderParamsParser::parse_string("tune=0").expect("parses"));
        assert_eq!(
            quality.probing.encoder_options,
            Some(expected),
            "a later set params keeps the earlier pairs"
        );

        // info prints the property, reset resets it.
        let output = condor_cmd(&temp)
            .args(["target-quality", "info", "statistic"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let statistic: serde_json::Value =
            serde_json::from_slice(&output).expect("info prints valid JSON");
        assert_eq!(
            statistic,
            serde_json::to_value(ProbeStatistic::Percentile(90.0)).expect("serializes"),
            "info statistic reports the probe statistic"
        );

        condor_cmd(&temp)
            .args(["target-quality", "reset", "statistic"])
            .assert()
            .success();
        condor_cmd(&temp).args(["target-quality", "reset", "params"]).assert().success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality exists");
        assert_eq!(
            quality.probing.statistic,
            ProbeStatistic::default(),
            "reset restores the default statistic"
        );
        assert!(
            quality.probing.encoder_options.is_none(),
            "reset drops the probing encoder options"
        );
    }
}
