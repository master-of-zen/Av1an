#[path = "common.rs"]
mod common;

use andean_condor::models::encoder::{Encoder, EncoderBase, cli_parameter::CLIParameter};
use california_condor::{
    commands::handlers::load_configuration,
    test_helpers::{check_encoder, ffmpeg_is_available, get_test_video, path_str},
};
use common::condor_cmd;

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn assert_info_logs(output: &std::process::Output, messages: &[&str]) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        for message in messages {
            assert!(
                stderr.lines().any(|line| line.contains("INFO") && line.contains(message)),
                "expected INFO log {message:?} in console output:\n{stderr}"
            );
        }
    }

    /// `x265` with the zone's CRF over the base's defaults.
    fn x265(crf: f64) -> Encoder {
        let mut encoder = Encoder::default_from_base(&EncoderBase::X265, false);
        encoder
            .parameters_mut()
            .insert("crf".to_owned(), CLIParameter::new_number("--", " ", crf));

        encoder
    }

    /// `svt-av1` with `preset` merged over the caller's encoder.
    fn with_preset(mut encoder: Encoder, preset: f64) -> Encoder {
        encoder.parameters_mut().insert(
            "preset".to_owned(),
            CLIParameter::new_number("--", " ", preset),
        );

        encoder
    }

    #[serial]
    #[test]
    fn zones_replace_and_merge_scene_encoders() {
        if !ffmpeg_is_available() {
            return;
        }
        let test_video = get_test_video();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let output = temp.path().join("out.mkv");
        let config_path = temp.path().join("condor.json");

        condor_cmd(&temp)
            .args(["init", path_str(&test_video.path), path_str(&output)])
            .assert()
            .success();

        // Mock scenes and save to config file (simulates Scene Detector)
        let (mut config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");
        let original = config.condor.encoder.clone();
        config.condor.scenes = test_video.mock_scenes(&original);
        config.save(&config_path).expect("save should succeed");

        let result = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .args(["zone", "0 240 replace x265 \"--crf 18\"", "480 -1 svt-av1 \"--preset 3\""])
            .assert()
            .success();
        assert_info_logs(result.get_output(), &[
            "Zoner: applying 2 zones to 5 scenes",
            "zone 1 (0..240) modified encoder settings for scene 0..121 (svt-av1 -> x265)",
            "zone 1 (0..240) modified encoder settings for scene 121..240",
            "zone 2 (480..720) modified encoder settings for scene 480..528",
            "zone 2 (480..720) modified encoder settings for scene 528..720",
            "zone 1 (0..240) matched 2 scenes, modified 2, split 0, added 0 (0 unchanged)",
            "Zoner finished: 2 zones, 4 scene changes across zones, 0 scenes added (5 -> 5 \
             total), 0 warnings",
        ]);
        assert!(
            !String::from_utf8_lossy(&result.get_output().stderr)
                .contains("modified encoder settings for scene 240..480"),
            "untouched scenes are not reported as modified"
        );

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        let zoner = config.condor.sequence_config.zoner.as_ref().expect("the zones are configured");
        assert_eq!(zoner.zones.len(), 2, "both zones are configured");
        assert_eq!(
            config.condor.scenes.len(),
            test_video.scenes.len(),
            "no scene is split"
        );

        // The test video's scenes are (0, 121), (121, 240), (240, 480), (480, 528),
        // (528, 720).
        let expected = [
            x265(18.0),
            x265(18.0),
            original.clone(),
            with_preset(original.clone(), 3.0),
            with_preset(original, 3.0),
        ];
        config.condor.scenes.iter().zip(expected.iter()).enumerate().for_each(
            |(index, (scene, expected_encoder))| {
                check_encoder(
                    &scene.encoder,
                    expected_encoder,
                    &format!("scene {index} encoder"),
                );
            },
        );

        // The zoning pass records itself on every scene, which is the
        // evidence later runs read.
        assert!(
            config.condor.scenes.iter().all(|scene| scene.sequence_data.zoner.zoned),
            "every scene records that the zones were applied"
        );

        let saved = std::fs::read(&config_path).expect("config bytes");
        let repeated = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .arg("zone")
            .assert()
            .success();
        assert_info_logs(repeated.get_output(), &["Zoner: the scenes already \
                                                   record a zoning pass; nothing \
                                                   to apply"]);
        let stderr = String::from_utf8_lossy(&repeated.get_output().stderr);
        assert!(
            !stderr.contains("Zoner finished") && !stderr.contains("applying"),
            "the recorded pass keeps the second run from applying anything"
        );
        assert_eq!(
            std::fs::read(&config_path).expect("config bytes"),
            saved,
            "a skipped run saves nothing"
        );

        // Re-applying needs the record cleared, which `condor scenes reset
        // zoner` does; the zones themselves were rewritten in between.
        let (mut reloaded, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");
        for scene in &mut reloaded.condor.scenes {
            scene.sequence_data.zoner.zoned = false;
        }
        reloaded.save(&config_path).expect("save should succeed");

        let mismatched = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .args(["zone", "0 240 x264 \"--crf 20\""])
            .assert()
            .success();
        assert_info_logs(mismatched.get_output(), &[
            "zone 1 (0..240) matched 2 scenes, modified 0, split 0, added 0 (2 unchanged)",
            "Zoner finished: 1 zones, 0 scene changes across zones, 0 scenes added (5 -> 5 \
             total), 2 warnings",
        ]);
    }

    #[serial]
    #[test]
    fn failed_zoning_never_saves_definitions_or_scenes() {
        let video = get_test_video();
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = temp.path().join("condor.json");
        condor_cmd(&temp)
            .args(["init", path_str(&video.path), "out.mkv"])
            .assert()
            .success();
        let (mut config, _) = load_configuration(Some(&config_path)).expect("config loads");
        config.condor.scenes = video.mock_scenes(&config.condor.encoder);
        config.save(&config_path).expect("config saves");
        let original = std::fs::read(&config_path).expect("config bytes");
        std::fs::write(temp.path().join("empty.json"), "[]").expect("empty zone file");
        std::fs::write(temp.path().join("invalid-filter.json"),
            r#"[{"start_frame":0,"end_frame":100,"encoder":{"Merge":{"base":"FFmpeg","ffmpeg_filter":"hflip"}}}]"#
        ).expect("invalid filter zone file");
        for zones in [
            vec!["0 721 svt-av1"],
            vec!["100 100 svt-av1"],
            vec!["0 100 --maximum-frames 0"],
            vec!["0 200 svt-av1", "100 300 svt-av1"],
            vec!["0 100 ffmpeg --ffmpeg-filter hflip"],
            vec!["empty.json"],
            vec!["invalid-filter.json"],
            vec!["0 100 av2"],
            vec![],
        ] {
            for dry_run in [false, true] {
                let mut command = condor_cmd(&temp);
                command.env("CONDOR_TEST_MODE", "1").arg("zone");
                if dry_run {
                    command.arg("--dry-run");
                }
                command.args(&zones).assert().failure();
                assert_eq!(std::fs::read(&config_path).expect("config bytes"), original);
                assert!(!config_path.with_extension("temp").exists());
            }
        }
        config.condor.scenes.clear();
        config.save(&config_path).expect("empty scenes save");
        let original = std::fs::read(&config_path).expect("config bytes");
        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args(["zone", "0 100 svt-av1"])
            .assert()
            .failure();
        assert_eq!(std::fs::read(&config_path).expect("config bytes"), original);
    }

    #[serial]
    #[test]
    fn dry_run_and_normal_zoning_preserve_stale_results_and_warn_about_shifted_files() {
        use andean_condor::core::sequence::parallel_encoder::ParallelEncoder;
        use california_condor::zoner::{ZonerConfig, parse_zone};

        let video = get_test_video();
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = temp.path().join("condor.json");
        condor_cmd(&temp)
            .args(["init", path_str(&video.path), "out.mkv"])
            .assert()
            .success();
        let (mut config, _) = load_configuration(Some(&config_path)).expect("config loads");
        config.condor.scenes = video.mock_scenes(&config.condor.encoder);
        config.condor.scenes[0].sequence_data.parallel_encoder.bytes = Some(123);
        config.condor.scenes[1].sequence_data.quality_check.quality.scores = vec![42.0];
        let definition = "-1 121 svt-av1 \"--crf 23\" --maximum-frames 96";
        config.condor.sequence_config.zoner = Some(ZonerConfig {
            zones: vec![parse_zone(definition).expect("zone parses")],
        });
        let scenes_dir = &config.condor.sequence_config.parallel_encoder.scenes_directory;
        let probe_dir = scenes_dir.join("Target Quality").join("1");
        std::fs::create_dir_all(&probe_dir).expect("probe directory");
        let encode = scenes_dir.join(format!("{}.ivf", ParallelEncoder::scene_id(4)));
        let probe = probe_dir.join(format!("{}.ivf", ParallelEncoder::scene_id(1)));
        std::fs::write(&encode, "existing encode").expect("encode fixture");
        std::fs::write(&probe, "existing probe").expect("probe fixture");
        let zone_file = temp.path().join("zones.txt");
        std::fs::write(&zone_file, definition).expect("zone file");
        let zone =
            &config.condor.sequence_config.zoner.as_ref().expect("configured zones").zones[0];
        std::fs::write(
            temp.path().join("zones.json"),
            serde_json::to_vec(&vec![zone]).expect("zone serializes"),
        )
        .expect("JSON zone file");
        std::fs::write(
            temp.path().join("zones.jsonl"),
            serde_json::to_string(zone).expect("zone serializes"),
        )
        .expect("JSONL zone file");
        config.save(&config_path).expect("config saves");
        let original = std::fs::read(&config_path).expect("config bytes");

        for args in [
            vec!["zone", "--dry-run", definition],
            vec!["zone", "zones.txt", "--dry-run"],
            vec!["zone", "--dry-run", "zones.json"],
            vec!["zone", "--dry-run", "zones.jsonl"],
            vec!["zone", "--dry-run"],
        ] {
            let result = condor_cmd(&temp)
                .env("CONDOR_TEST_MODE", "1")
                .env("RUST_LOG", "california_condor=info")
                .args(args)
                .assert()
                .success();
            assert_info_logs(result.get_output(), &[
                "Zoner dry-run:",
                "modified encoder settings for scene 0..121",
                "split scene 0..121 into 2 scenes (1 added)",
                "matched 1 scenes, modified 1, split 1, added 1 (0 unchanged)",
                "no configuration changes were saved",
            ]);
            let stderr = String::from_utf8_lossy(&result.get_output().stderr);
            assert!(stderr.contains("scene index 0") && stderr.contains("saved encode results"));
            assert!(stderr.contains("scene index 4 has an existing encode/probe file"));
            assert!(stderr.contains(path_str(&probe)));
            assert_eq!(std::fs::read(&config_path).expect("config bytes"), original);
            assert!(!config_path.with_extension("temp").exists());
            assert_eq!(
                std::fs::read_to_string(&encode).expect("encode file"),
                "existing encode"
            );
            assert_eq!(
                std::fs::read_to_string(&probe).expect("probe file"),
                "existing probe"
            );
        }

        let result = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .arg("zone")
            .assert()
            .success();
        assert!(String::from_utf8_lossy(&result.get_output().stderr).contains("may be stale"));
        let (updated, _) = load_configuration(Some(&config_path)).expect("config loads");
        assert_eq!(updated.condor.scenes.len(), 6);
        assert_eq!(
            updated.condor.scenes[0].sequence_data.parallel_encoder.bytes,
            Some(123)
        );
        assert_eq!(
            updated.condor.scenes[2].sequence_data.quality_check.quality.scores,
            vec![42.0]
        );
        assert_eq!(
            std::fs::read_to_string(&encode).expect("encode file"),
            "existing encode"
        );
        assert_eq!(
            std::fs::read_to_string(&probe).expect("probe file"),
            "existing probe"
        );

        let no_op = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .args(["zone", "--dry-run"])
            .assert()
            .success();
        assert!(
            !String::from_utf8_lossy(&no_op.get_output().stderr).contains("may be stale"),
            "no-op zoning does not warn about cleanup"
        );
    }

    #[serial]
    #[test]
    fn a_zone_file_caps_scene_length() {
        if !ffmpeg_is_available() {
            return;
        }
        let test_video = get_test_video();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let output = temp.path().join("out.mkv");
        let config_path = temp.path().join("condor.json");
        let zones_path = temp.path().join("zones.json");

        condor_cmd(&temp)
            .args(["init", path_str(&test_video.path), path_str(&output)])
            .assert()
            .success();

        let (mut config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");
        let original = config.condor.encoder.clone();
        config.condor.scenes = test_video.mock_scenes(&original);
        config.save(&config_path).expect("save should succeed");

        std::fs::write(
            &zones_path,
            r#"{"zones": [{"start_frame": 0, "end_frame": 240, "maximum_frames": 96, "encoder": null}]}"#,
        )
        .expect("the zone file is written");

        let result = condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .env("RUST_LOG", "california_condor=info")
            .args(["zone", "zones.json"])
            .assert()
            .success();
        assert_info_logs(result.get_output(), &[
            "zone 1 (0..240) split scene 0..121 into 2 scenes (1 added), frame ranges: [(0, 60), \
             (60, 121)]",
            "zone 1 (0..240) split scene 121..240 into 2 scenes (1 added), frame ranges: [(121, \
             180), (180, 240)]",
            "zone 1 (0..240) matched 2 scenes, modified 2, split 2, added 2 (0 unchanged)",
            "Zoner finished: 1 zones, 2 scene changes across zones, 2 scenes added (5 -> 7 \
             total), 0 warnings",
        ]);

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        let ranges = config
            .condor
            .scenes
            .iter()
            .map(|scene| (scene.start_frame, scene.end_frame))
            .collect::<Vec<_>>();
        assert_eq!(
            ranges,
            vec![(0, 60), (60, 121), (121, 180), (180, 240), (240, 480), (480, 528), (528, 720)],
            "the zoned scenes are split and the rest are untouched"
        );
        assert!(
            config.condor.scenes.iter().all(|scene| scene.encoder == original),
            "a length cap changes no encoder"
        );
    }
}
