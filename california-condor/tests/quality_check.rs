#[path = "common.rs"]
mod common;
#[path = "config_helpers.rs"]
mod config_helpers;

use andean_condor::models::sequence::quality_check::QualityCheckConfig;
use california_condor::{
    commands::handlers::load_configuration,
    test_helpers::*,
    utils::hash_path::hash_path,
};
use common::condor_cmd;
use config_helpers::{init_config, load};

#[cfg(test)]
mod tests {
    use andean_condor::{
        ffmpeg::FFPixelFormat,
        models::{
            encoder::cli_parameter::CLIParameter,
            input::{Input, VapourSynthImportMethod},
            sequence::target_quality::types::{
                DEFAULT_XPSNR_TARGET_RANGE,
                ProbeStatistic,
                ProbeStrategy,
                QualityMetric,
                QualityMetric as MetricModel,
                SubsetProbeLength,
                SubsetProbePosition,
            },
        },
        vapoursynth::{plugins::resize::Scaler, vapoursynth_filters::VapourSynthFilter},
    };
    use serial_test::serial;

    use super::*;

    /// Downscaling the clip to 540p cuts the encode and metric cost roughly
    /// fourfold, and nothing here depends on the source resolution.
    const WIDTH: usize = 960;
    const HEIGHT: usize = 540;
    /// The subset profile scores this many frames per scene.
    const SCORED_FRAMES: u32 = 11;

    /// One encode, then both quality-check paths run against it.
    ///
    /// Encoding dominates this file's runtime, so the full run and the resume
    /// run share a single encode. The resume case rewrites the config to look
    /// partially scored and re-runs the same command.
    #[serial]
    #[test]
    fn scores_every_scene_and_resumes_partial_scenes() {
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

        // Mock an existing config file with scenes
        let mut config = default_config(&test_video, &output, &temp_abs);
        if let Input::VapourSynth {
            filters, ..
        } = &mut config.condor.input
        {
            *filters = vec![VapourSynthFilter::Resize {
                scaler: None,
                width:  Some(WIDTH),
                height: Some(HEIGHT),
                format: Some(FFPixelFormat::YUV420P10LE),
            }];
        }
        config.condor.encoder.parameters_mut().insert(
            "preset".to_owned(),
            CLIParameter::new_number("--", " ", 10.0),
        );
        config.condor.sequence_config.parallel_encoder.workers = Some(2);
        config.condor.scenes = test_video.mock_scenes(&config.condor.encoder);
        config.save(&config_path).expect("configuration save should succeed");

        // Encode and concatenate first so the output file exists
        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args(["encode"])
            .assert()
            .success();

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args(["concatenate", "--method", "mkvmerge"])
            .assert()
            .success();

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args([
                "quality-check",
                "--input",
                path_str(&test_video.path),
                "--decoder",
                "vs-ffms2",
                "--filters",
                &format!("resize:scaler=bicubic;width={WIDTH};height={HEIGHT};format=yuv420p10le;"),
                "--metric",
                "xpsnr",
                "--profile",
                "fast",
            ])
            .assert()
            .success();

        let mut expected_config = config;
        expected_config.condor.sequence_config.quality_check = Some(QualityCheckConfig {
            metric:    QualityMetric::XPSNR {
                target_range: DEFAULT_XPSNR_TARGET_RANGE,
                resolution:   None,
            },
            strategy:  ProbeStrategy::Subset {
                position: SubsetProbePosition::Middle,
                length:   SubsetProbeLength::Frames(SCORED_FRAMES),
            },
            statistic: ProbeStatistic::Mean,
            // The metric input records the filters it was given, scaler included.
            input:     Some(Input::VapourSynth {
                path:          input_abs,
                import_method: VapourSynthImportMethod::FFMS2 {
                    index: None
                },
                cache_path:    None,
                filters:       vec![VapourSynthFilter::Resize {
                    scaler: Some(Scaler::Bicubic),
                    width:  Some(WIDTH),
                    height: Some(HEIGHT),
                    format: Some(FFPixelFormat::YUV420P10LE),
                }],
            }),
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
        config.condor.scenes.iter().enumerate().for_each(|(index, scene)| {
            assert_eq!(
                scene.sequence_data.quality_check.quality.scores.len(),
                SCORED_FRAMES as usize,
                "scene {} should have {SCORED_FRAMES} quality check scores",
                index,
            );
        });

        // Now the resume path: mark two scenes as already scored and re-run the
        // same command. Those scenes keep their existing scores, so this also
        // covers that a partial run is not redone from scratch.
        let mut config = load_configuration(Some(&config_path))
            .expect("load_configuration should succeed")
            .0;
        let already_scored = vec![45.0; SCORED_FRAMES as usize];
        config.condor.scenes[0].sequence_data.quality_check.quality.scores = already_scored.clone();
        config.condor.scenes[1].sequence_data.quality_check.quality.scores = already_scored;
        config.save(&config_path).expect("configuration save should succeed");

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args([
                "quality-check",
                "--input",
                path_str(&test_video.path),
                "--decoder",
                "vs-ffms2",
                "--filters",
                &format!("resize:scaler=bicubic;width={WIDTH};height={HEIGHT};format=yuv420p10le;"),
                "--metric",
                "xpsnr",
                "--profile",
                "fast",
            ])
            .assert()
            .success();

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        config.condor.scenes.iter().enumerate().for_each(|(index, scene)| {
            assert_eq!(
                scene.sequence_data.quality_check.quality.scores.len(),
                SCORED_FRAMES as usize,
                "scene {} should have {SCORED_FRAMES} quality check scores after resuming",
                index,
            );
        });
    }

    /// `quality-check` probes and reduces exactly as configured, and refuses
    /// flags that do not belong to the chosen kind.
    #[serial]
    #[test]
    fn quality_check_set_strategy_and_statistic() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args([
                "quality-check",
                "set",
                "strategy",
                "subset",
                "--position",
                "end",
                "--length-percent",
                "25",
            ])
            .assert()
            .success();
        let check = load(&config_path)
            .condor
            .sequence_config
            .quality_check
            .expect("quality check was materialized");
        assert_eq!(
            check.strategy,
            ProbeStrategy::Subset {
                position: SubsetProbePosition::End,
                length:   SubsetProbeLength::Percentage(25.0),
            },
            "strategy is set"
        );

        // Flags that do not belong to the kind are refused.
        condor_cmd(&temp)
            .args(["quality-check", "set", "strategy", "whole", "--skip", "4"])
            .assert()
            .failure();
        condor_cmd(&temp)
            .args(["quality-check", "set", "strategy", "skip"])
            .assert()
            .failure();
        condor_cmd(&temp)
            .args(["quality-check", "set", "statistic", "percentile"])
            .assert()
            .failure();
        condor_cmd(&temp)
            .args(["quality-check", "set", "statistic", "percentile", "--percentile", "150"])
            .assert()
            .failure();

        condor_cmd(&temp)
            .args(["quality-check", "set", "statistic", "percentile", "--percentile", "10"])
            .assert()
            .success();
        let check = load(&config_path)
            .condor
            .sequence_config
            .quality_check
            .expect("quality check exists");
        assert_eq!(
            check.statistic,
            ProbeStatistic::Percentile(10.0),
            "statistic is set"
        );

        condor_cmd(&temp)
            .args(["quality-check", "set", "metric", "xpsnr"])
            .assert()
            .success();
        let check = load(&config_path)
            .condor
            .sequence_config
            .quality_check
            .expect("quality check exists");
        assert!(
            matches!(check.metric, MetricModel::XPSNR { .. }),
            "metric is {:?}",
            check.metric
        );

        condor_cmd(&temp)
            .args(["quality-check", "reset", "strategy"])
            .assert()
            .success();
        let check = load(&config_path)
            .condor
            .sequence_config
            .quality_check
            .expect("quality check exists");
        assert_eq!(
            check.strategy,
            ProbeStrategy::default(),
            "reset restores the default strategy"
        );
    }
}
