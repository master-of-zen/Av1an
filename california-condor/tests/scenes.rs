#[path = "common.rs"]
mod common;

use andean_condor::ffmpeg::FFPixelFormat;
use california_condor::{
    commands::handlers::load_configuration,
    test_helpers::{TestVideo, default_config},
};
use common::condor_cmd;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::Value;
    use serial_test::serial;

    use super::*;

    /// The five scenes `get_test_video` describes, without generating the
    /// clip the scenes command never opens: (0, 121), (121, 240), (240, 480),
    /// (480, 528), (528, 720).
    fn test_video() -> TestVideo {
        TestVideo {
            path:         PathBuf::from("video.mkv"),
            width:        1920,
            height:       1080,
            frames:       720,
            fps_rational: (24, 1),
            format:       FFPixelFormat::YUV420P,
            scenes:       vec![(0, 121), (121, 240), (240, 480), (480, 528), (528, 720)],
        }
    }

    /// Writes a configuration whose scenes a detection run produced. The
    /// `scenes` command never opens the input, so the paths only need to
    /// load.
    fn scenes_config(temp: &tempfile::TempDir) -> PathBuf {
        let video = test_video();
        let mut configuration = default_config(&video, &temp.path().join("out.mkv"), temp.path());
        configuration.condor.scenes = video.mock_scenes(&configuration.condor.encoder);
        let config_path = temp.path().join("condor.json");
        configuration.save(&config_path).expect("the config saves");

        config_path
    }

    fn load(config_path: &Path) -> california_condor::configuration::Configuration {
        load_configuration(Some(config_path))
            .expect("load_configuration should succeed")
            .0
    }

    fn ranges(
        configuration: &california_condor::configuration::Configuration,
    ) -> Vec<(usize, usize)> {
        configuration
            .condor
            .scenes
            .iter()
            .map(|scene| (scene.start_frame, scene.end_frame))
            .collect()
    }

    fn stdout_json(output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("stdout is JSON")
    }

    fn stderr(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    #[serial]
    #[test]
    fn scenes_print_and_info_address_the_selection() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let mut configuration = load(&config_path);
        configuration.condor.scenes[0].sequence_data.parallel_encoder.bytes = Some(123);
        configuration.condor.scenes[1].sub_scenes = Some(vec![
            andean_condor::models::scene::SubScene {
                start_frame: 121,
                end_frame:   180,
            },
            andean_condor::models::scene::SubScene {
                start_frame: 180,
                end_frame:   240,
            },
        ]);
        configuration.save(&config_path).expect("the config saves");

        // Without an action the whole list is printed, with all their values,
        // and the totals follow on stderr.
        let bare = condor_cmd(&temp).arg("scenes").assert().success();
        let all = stdout_json(bare.get_output());
        let all = all.as_array().expect("an array of scenes");
        assert_eq!(all.len(), 5);
        assert_eq!(all[0]["start_frame"], 0);
        assert_eq!(all[0]["end_frame"], 121);
        assert_eq!(
            all[0]["sequence_data"]["parallel_encoder"]["bytes"], 123,
            "saved results are part of the report"
        );
        assert_eq!(all[1]["sub_scenes"][1]["end_frame"], 240);
        assert!(
            stderr(bare.get_output()).contains("Scenes: 5 Frames: 0-720"),
            "the totals name the count and the covered frames: {}",
            stderr(bare.get_output())
        );

        let info = condor_cmd(&temp).args(["scenes", "info"]).assert().success();
        assert_eq!(stdout_json(info.get_output()), serde_json::json!(all));

        // An index addresses one scene, which is reported on its own, and the
        // summary that follows describes that scene rather than the list.
        let scene = condor_cmd(&temp).args(["scenes", "2", "info"]).assert().success();
        assert_eq!(stdout_json(scene.get_output())["start_frame"], 240);
        assert!(
            stderr(scene.get_output()).contains("Scenes: 1 Frames: 240-480"),
            "an indexed read reports the scenes it addressed: {}",
            stderr(scene.get_output())
        );
        let frames = condor_cmd(&temp).args(["scenes", "2", "info", "frames"]).assert().success();
        assert_eq!(
            stdout_json(frames.get_output()),
            serde_json::json!({
                "start_frame": 240,
                "end_frame":   480,
            })
        );
        assert!(
            stderr(frames.get_output()).contains("Scenes: 1 Frames: 240-480"),
            "a key read reports the scene it addressed: {}",
            stderr(frames.get_output())
        );

        // Several indices report an array, in ascending order.
        let many = condor_cmd(&temp).args(["scenes", "1,4", "info", "frames"]).assert().success();
        assert_eq!(
            stdout_json(many.get_output()),
            serde_json::json!([
                {"start_frame": 121, "end_frame": 240},
                {"start_frame": 528, "end_frame": 720},
            ])
        );
        assert!(
            stderr(many.get_output()).contains("Scenes: 2 Frames: 121-720"),
            "a list of indices reports just those scenes: {}",
            stderr(many.get_output())
        );

        let sub_scenes =
            condor_cmd(&temp).args(["scenes", "1", "info", "sub-scenes"]).assert().success();
        assert_eq!(
            stdout_json(sub_scenes.get_output()).as_array().expect("two sub-scenes").len(),
            2
        );

        let missing = condor_cmd(&temp).args(["scenes", "9", "info"]).assert().failure();
        assert!(
            stderr(missing.get_output()).contains("scene index 9 does not exist"),
            "a missing index is named"
        );
        let partial = condor_cmd(&temp).args(["scenes", "1-2,8-10,16", "info"]).assert().failure();
        assert!(
            stderr(partial.get_output()).contains("scene index 8 does not exist"),
            "the lowest missing index is named"
        );
        let bad = condor_cmd(&temp).args(["scenes", "scene1", "info"]).assert().failure();
        assert!(
            stderr(bad.get_output()).contains("is not a scene index"),
            "a bad index explains what an index looks like"
        );
    }

    #[serial]
    #[test]
    fn scenes_set_moves_a_frame_and_warns_about_stale_results() {
        use andean_condor::core::sequence::parallel_encoder::ParallelEncoder;

        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let mut configuration = load(&config_path);
        configuration.condor.scenes[0].sequence_data.parallel_encoder.bytes = Some(123);
        let scenes_directory =
            &configuration.condor.sequence_config.parallel_encoder.scenes_directory;
        std::fs::create_dir_all(scenes_directory).expect("scenes directory");
        let encode = scenes_directory.join(format!("{}.ivf", ParallelEncoder::scene_id(0)));
        std::fs::write(&encode, "existing encode").expect("encode fixture");
        configuration.save(&config_path).expect("the config saves");
        let original = std::fs::read(&config_path).expect("config bytes");

        let moved = condor_cmd(&temp)
            .env("RUST_LOG", "california_condor=info")
            .args(["scenes", "0", "set", "end", "100"])
            .assert()
            .success();
        assert!(
            String::from_utf8_lossy(&moved.get_output().stdout)
                .contains("Scenes scene 0 end_frame updated"),
            "the change is reported: {}",
            String::from_utf8_lossy(&moved.get_output().stdout)
        );
        let warnings = stderr(moved.get_output());
        assert!(
            warnings.contains("scene index 0")
                && warnings.contains("may be stale after scene edits"),
            "saved results and files warn as stale: {warnings}"
        );

        let configuration = load(&config_path);
        assert_eq!(
            ranges(&configuration)[0],
            (0, 100),
            "only the addressed frame range moved"
        );
        assert_eq!(
            std::fs::read_to_string(&encode).expect("encode file"),
            "existing encode",
            "stale files are never touched"
        );

        // A frame that would empty the scene is rejected and never saves.
        let rejected = condor_cmd(&temp)
            .args(["scenes", "0", "set", "start", "100"])
            .assert()
            .failure();
        assert!(
            stderr(rejected.get_output()).contains("would be empty"),
            "the error explains the frame"
        );
        assert_ne!(
            std::fs::read(&config_path).expect("config bytes"),
            original,
            "the earlier edit saved"
        );
        let unchanged = std::fs::read(&config_path).expect("config bytes");
        condor_cmd(&temp)
            .args(["scenes", "0", "set", "start", "100"])
            .assert()
            .failure();
        assert_eq!(
            std::fs::read(&config_path).expect("config bytes"),
            unchanged,
            "a rejected edit never saves"
        );

        // A frame that reaches into a neighbour warns and names it.
        let overlapping =
            condor_cmd(&temp).args(["scenes", "1", "set", "start", "50"]).assert().success();
        let warnings = stderr(overlapping.get_output());
        assert!(
            warnings.contains("scene 1") && warnings.contains("overlaps scene 0"),
            "the overlap is named: {warnings}"
        );
        assert_eq!(
            ranges(&load(&config_path))[1],
            (50, 240),
            "the edit itself is accepted"
        );

        // A write reports the scenes it addressed too, with the frames they
        // were left holding.
        let narrowed =
            condor_cmd(&temp).args(["scenes", "0", "set", "start", "60"]).assert().success();
        assert!(
            stderr(narrowed.get_output()).contains("Scenes: 1 Frames: 60-100"),
            "an edited selection reports itself, with the frames it was left holding: {}",
            stderr(narrowed.get_output())
        );

        // The reported span covers the frames the scenes actually describe,
        // even once edits leave the list unsorted: scene 0 now starts after
        // scene 1 and ends after the last scene does.
        condor_cmd(&temp).args(["scenes", "0", "set", "end", "900"]).assert().success();
        let stretched = condor_cmd(&temp).arg("scenes").assert().success();
        assert!(
            stderr(stretched.get_output()).contains("Scenes: 5 Frames: 50-900"),
            "an unindexed read reports the whole list, spanning past the last scene's end and \
             below the first scene's start: {}",
            stderr(stretched.get_output())
        );
    }

    #[serial]
    #[test]
    fn scenes_set_edits_only_the_named_scene() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let global = load(&config_path).condor.encoder;

        condor_cmd(&temp)
            .args(["scenes", "1", "set", "encoder", "x265"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "1", "set", "params", "crf=18"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "4", "set", "photon-noise", "800", "--chroma-noise", "200"])
            .assert()
            .success();
        condor_cmd(&temp).args(["scenes", "3", "set", "passes", "2"]).assert().success();
        condor_cmd(&temp)
            .args(["scenes", "0", "set", "ffmpeg-filter", "hflip"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "2", "set", "sub-scenes", "240-300", "300-480"])
            .assert()
            .success();

        let configuration = load(&config_path);
        assert_eq!(
            configuration.condor.scenes[1].encoder.base().to_string(),
            "x265"
        );
        assert_eq!(
            configuration.condor.scenes[1]
                .encoder
                .parameters()
                .get("crf")
                .expect("crf merged"),
            &andean_condor::models::encoder::cli_parameter::CLIParameter::new_number("", "=", 18.0)
        );
        let photon_noise = configuration.condor.scenes[4]
            .encoder
            .photon_noise()
            .expect("scene 4 carries Photon Noise");
        assert_eq!(photon_noise.iso, 800);
        assert_eq!(photon_noise.chroma_iso, Some(200));
        assert_eq!(
            configuration.condor.scenes[3].encoder.passes(),
            andean_condor::models::encoder::EncoderPasses::All(2)
        );
        assert_eq!(
            configuration.condor.scenes[0].encoder.ffmpeg_filter(),
            Some("hflip")
        );
        assert_eq!(
            configuration.condor.scenes[2].sub_scenes.as_ref().map(Vec::len),
            Some(2)
        );
        assert_eq!(
            configuration.condor.scenes[0].encoder.base(),
            global.base(),
            "a scene that only took a filtergraph keeps its encoder"
        );

        // One line can switch an encoder and set it up at once.
        condor_cmd(&temp)
            .args([
                "scenes",
                "4",
                "set",
                "encoder",
                "svt-av1",
                "--passes",
                "3",
                "--params",
                "crf=16",
                "--iso",
                "1200",
                "--chroma-iso",
                "300",
                "--ffmpeg-filter",
                "crop=iw-16:ih-16",
            ])
            .assert()
            .success();
        let encoder = &load(&config_path).condor.scenes[4].encoder;
        assert_eq!(encoder.base().to_string(), "svt-av1");
        assert_eq!(
            encoder.passes(),
            andean_condor::models::encoder::EncoderPasses::All(3)
        );
        let photon_noise = encoder.photon_noise().expect("Photon Noise was set");
        assert_eq!(photon_noise.iso, 1200);
        assert_eq!(photon_noise.chroma_iso, Some(300));
        assert_eq!(encoder.ffmpeg_filter(), Some("crop=iw-16:ih-16"));

        // A whole selection takes the same change, and reports the array.
        let all = condor_cmd(&temp)
            .args(["scenes", "-1", "set", "passes", "4"])
            .assert()
            .success();
        let printed = String::from_utf8_lossy(&all.get_output().stdout).into_owned();
        assert!(
            printed.contains("Scenes scenes 0-4 passes updated"),
            "a whole-selection change is reported once: {printed}"
        );
        assert!(
            !printed.contains('['),
            "the value waits for --verbose: {printed}"
        );

        // `--verbose` prints every addressed scene's new value.
        let all = condor_cmd(&temp)
            .args(["--verbose", "scenes", "-1", "set", "passes", "4"])
            .assert()
            .success();
        let printed = String::from_utf8_lossy(&all.get_output().stdout).into_owned();
        let (_, values) = printed.split_once('\n').expect("a value line");
        let values: Value = serde_json::from_str(values).expect("the reported value is JSON");
        assert_eq!(
            values.as_array().expect("five values").len(),
            5,
            "every addressed scene reports its new value: {printed}"
        );
        assert!(
            load(&config_path).condor.scenes.iter().all(|scene| scene.encoder.passes()
                == andean_condor::models::encoder::EncoderPasses::All(4)),
            "the whole list took the pass count"
        );

        // Capability errors name the encoder and never reach the file.
        let photon_noise = condor_cmd(&temp)
            .args(["scenes", "1", "set", "encoder", "x265", "--iso", "800"])
            .assert()
            .failure();
        assert!(
            stderr(photon_noise.get_output()).contains("does not support Photon Noise"),
            "x265 cannot carry Photon Noise"
        );
        let chroma = condor_cmd(&temp)
            .args(["scenes", "1", "set", "encoder", "svt-av1", "--chroma-iso", "200"])
            .assert()
            .failure();
        assert!(
            stderr(chroma.get_output()).contains("--chroma-iso needs --iso"),
            "a chroma ISO alone is rejected"
        );
        let outside = condor_cmd(&temp)
            .args(["scenes", "2", "set", "sub-scenes", "0-999"])
            .assert()
            .failure();
        assert!(
            stderr(outside.get_output()).contains("lies outside scene"),
            "sub-scenes must stay inside their scene"
        );
    }

    #[serial]
    #[test]
    fn scenes_reset_restores_what_detection_would_have_written() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let global = load(&config_path).condor.encoder;

        condor_cmd(&temp)
            .args(["scenes", "2", "set", "encoder", "x265"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "2", "set", "sub-scenes", "240-300"])
            .assert()
            .success();

        let encoder =
            condor_cmd(&temp).args(["scenes", "2", "reset", "encoder"]).assert().success();
        assert!(
            String::from_utf8_lossy(&encoder.get_output().stdout)
                .contains("Scenes scene 2 encoder reset to its default"),
            "the reset is reported"
        );
        let mut configuration = load(&config_path);
        assert_eq!(configuration.condor.scenes[2].encoder, global);

        condor_cmd(&temp)
            .args(["scenes", "2", "reset", "sub-scenes"])
            .assert()
            .success();
        configuration = load(&config_path);
        assert_eq!(configuration.condor.scenes[2].sub_scenes, None);

        // A keyless reset restores the scene itself and names it once.
        let scene = condor_cmd(&temp).args(["scenes", "2", "reset"]).assert().success();
        let printed = String::from_utf8_lossy(&scene.get_output().stdout).into_owned();
        assert!(
            printed.contains("Scenes scene 2 reset to its default"),
            "the scene reset is named once: {printed}"
        );

        let frames = condor_cmd(&temp).args(["scenes", "0", "reset", "frames"]).assert().failure();
        assert!(
            stderr(frames.get_output()).contains("has no default"),
            "frame ranges come from detection"
        );

        let reset = condor_cmd(&temp).args(["scenes", "reset"]).assert().success();
        assert!(
            String::from_utf8_lossy(&reset.get_output().stdout).contains("condor detect-scenes"),
            "the reset hints at rebuilding the scenes"
        );
        assert!(
            load(&config_path).condor.scenes.is_empty(),
            "the list is empty"
        );
    }

    #[serial]
    #[test]
    fn scenes_info_and_reset_reach_the_sequence_data() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let mut configuration = load(&config_path);
        configuration.condor.scenes[2].sequence_data.parallel_encoder.bytes = Some(456);
        configuration.save(&config_path).expect("the config saves");

        // The saved step results are readable per scene and per selection.
        let one = condor_cmd(&temp)
            .args(["scenes", "2", "info", "parallel-encoder"])
            .assert()
            .success();
        assert_eq!(stdout_json(one.get_output())["bytes"], 456);

        let many = condor_cmd(&temp)
            .args(["scenes", "43-46", "info", "quality-check"])
            .assert()
            .failure();
        assert!(
            stderr(many.get_output()).contains("scene index 43 does not exist"),
            "a selection can be out of range: {}",
            stderr(many.get_output())
        );
        let pair = condor_cmd(&temp)
            .args(["scenes", "1-2", "info", "quality-check"])
            .assert()
            .success();
        assert_eq!(
            stdout_json(pair.get_output()).as_array().expect("two scenes").len(),
            2
        );

        // A reset clears the saved results of every addressed scene.
        condor_cmd(&temp)
            .args(["scenes", "-1", "reset", "target-quality"])
            .assert()
            .success();
        let reset = condor_cmd(&temp)
            .args(["scenes", "-1", "reset", "parallel-encoder"])
            .assert()
            .success();
        assert!(
            String::from_utf8_lossy(&reset.get_output().stdout)
                .contains("Scenes scenes 0-4 parallel-encoder reset to its default"),
            "a whole-list reset is reported once: {}",
            String::from_utf8_lossy(&reset.get_output().stdout)
        );
        let configuration = load(&config_path);
        assert!(configuration.condor.scenes.iter().all(|scene| {
            scene.sequence_data.parallel_encoder.bytes.is_none()
                && scene.sequence_data.target_quality.passes.is_empty()
        }));
        assert_eq!(
            configuration.condor.scenes[2].start_frame, 240,
            "resetting results leaves the frame range alone"
        );
    }

    #[serial]
    #[test]
    fn scenes_delete_and_add_change_the_list() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);

        let deleted = condor_cmd(&temp).args(["scenes", "1,3", "delete"]).assert().success();
        assert!(
            String::from_utf8_lossy(&deleted.get_output().stdout)
                .contains("Deleted scenes 1,3: 121-240, 480-528"),
            "the deletion names the scenes and the frames they held: {}",
            String::from_utf8_lossy(&deleted.get_output().stdout)
        );
        assert!(
            stderr(deleted.get_output()).contains("Scenes: 2 Frames: 121-528"),
            "the summary covers the scenes that were removed: {}",
            stderr(deleted.get_output())
        );
        assert_eq!(
            ranges(&load(&config_path)),
            vec![(0, 121), (240, 480), (528, 720)],
            "the remaining scenes keep their order"
        );

        // A new scene sorts by start frame unless told where to go.
        let added = condor_cmd(&temp).args(["scenes", "add", "240", "400"]).assert().success();
        assert!(
            String::from_utf8_lossy(&added.get_output().stdout).contains("Added scene 1: 240-400"),
            "the addition names the scene and the frames it covers: {}",
            String::from_utf8_lossy(&added.get_output().stdout)
        );
        assert!(
            stderr(added.get_output()).contains("Scenes: 1 Frames: 240-400"),
            "the summary covers the scene that was added: {}",
            stderr(added.get_output())
        );
        assert_eq!(
            ranges(&load(&config_path)),
            vec![(0, 121), (240, 400), (240, 480), (528, 720)],
            "an equal start goes before the existing scene"
        );

        condor_cmd(&temp)
            .args(["scenes", "add", "900", "1000", "--append"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "add", "5", "10", "--prepend"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scenes", "add", "700", "800", "--after", "1"])
            .assert()
            .success();
        assert_eq!(
            ranges(&load(&config_path)),
            vec![(5, 10), (0, 121), (700, 800), (240, 400), (240, 480), (528, 720), (900, 1000),],
            "every placement landed where it was asked to"
        );

        let empty = condor_cmd(&temp).args(["scenes", "add", "10", "10"]).assert().failure();
        assert!(
            stderr(empty.get_output()).contains("is empty"),
            "an empty range is rejected"
        );
        let indexed = condor_cmd(&temp).args(["scenes", "1", "add", "10", "20"]).assert().failure();
        assert!(
            stderr(indexed.get_output()).contains("takes no index"),
            "`add` takes no index"
        );
    }

    #[serial]
    #[test]
    fn scenes_set_and_delete_need_an_index() {
        let temp = tempfile::tempdir().expect("temp directory");
        let config_path = scenes_config(&temp);
        let before = std::fs::read(&config_path).expect("config bytes");

        let set = condor_cmd(&temp).args(["scenes", "set", "start", "23"]).assert().failure();
        assert!(
            stderr(set.get_output()).contains("which scenes?"),
            "set names the missing index: {}",
            stderr(set.get_output())
        );
        let deleted = condor_cmd(&temp).args(["scenes", "delete"]).assert().failure();
        assert!(
            stderr(deleted.get_output()).contains("which scenes?"),
            "delete names the missing index: {}",
            stderr(deleted.get_output())
        );
        assert_eq!(
            std::fs::read(&config_path).expect("config bytes"),
            before,
            "nothing was written"
        );
    }
}
