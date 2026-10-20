#[path = "common.rs"]
mod common;
#[path = "config_helpers.rs"]
mod config_helpers;

use std::path::Path;

use andean_condor::models::sequence::scene_concatenator::ConcatMethod;
use california_condor::test_helpers::*;
use common::condor_cmd;
use config_helpers::{init_config, load};

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    /// `concatenate` switches tools the way the run flag does: keeping a tool
    /// keeps its settings, switching starts from the new tool's defaults.
    #[serial]
    #[test]
    fn concatenate_set_method_and_output() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args(["concatenate", "set", "method", "ffmpeg"])
            .assert()
            .success();
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        assert!(
            matches!(concat.method, ConcatMethod::FFmpeg(_)),
            "method is {}",
            concat.method
        );

        condor_cmd(&temp)
            .args(["concatenate", "set", "output", "final.mkv"])
            .assert()
            .success();
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        assert_eq!(
            concat.output.as_deref(),
            Some(Path::new("final.mkv")),
            "output is set"
        );

        condor_cmd(&temp).args(["concatenate", "reset", "output"]).assert().success();
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        assert!(concat.output.is_none(), "output was cleared");

        condor_cmd(&temp)
            .args(["concatenate", "set", "method", "mkvmerge"])
            .assert()
            .success();
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        assert!(
            matches!(concat.method, ConcatMethod::MKVMerge(_)),
            "method is {}",
            concat.method
        );
    }

    /// `--discard` probes the input and writes `copy: false` entries for the
    /// tracks it names, so `condor.json` describes the drop explicitly.
    #[serial]
    #[test]
    fn init_discard_audio_and_subtitle() {
        if !ffmpeg_is_available() || !mkvmerge_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let input = get_test_multitrack_video();
        let output = temp.path().join("out.mkv");

        condor_cmd(&temp)
            .args([
                "init",
                path_str(&input),
                path_str(&output),
                "--concat",
                "mkvmerge",
                "--discard",
                "audio,subtitle,chapters",
            ])
            .assert()
            .success();

        let config_path = temp.path().join("condor.json");
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        let ConcatMethod::MKVMerge(mkvmerge) = concat.method else {
            panic!("the method stays mkvmerge, but is {}", concat.method);
        };
        let tracks = mkvmerge.tracks.expect("a track map is written");
        assert_eq!(
            tracks.len(),
            3,
            "only the named tracks are written, not the whole map: {tracks:?}"
        );
        for (id, track) in &tracks {
            assert!(!track.copy(), "track {id} is dropped");
        }
        let ids = tracks.keys().copied().collect::<Vec<_>>();
        assert!(
            ids.contains(&1) && ids.contains(&2) && ids.contains(&3),
            "both audio tracks and the subtitle are dropped: {ids:?}"
        );
        assert!(!mkvmerge.chapters.expect("chapters are written").copy);
    }

    /// The FFmpeg selection map has to list every kept stream, because a stream
    /// that is absent from the map is not copied.
    #[serial]
    #[test]
    fn init_discard_audio_keeps_the_other_ffmpeg_streams() {
        if !ffmpeg_is_available() || !mkvmerge_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let input = get_test_multitrack_video();
        let output = temp.path().join("out.mkv");

        condor_cmd(&temp)
            .args([
                "init",
                path_str(&input),
                path_str(&output),
                "--concat",
                "ffmpeg",
                "--discard",
                "audio",
            ])
            .assert()
            .success();

        let config_path = temp.path().join("condor.json");
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        let ConcatMethod::FFmpeg(ffmpeg) = concat.method else {
            panic!("the method stays ffmpeg, but is {}", concat.method);
        };
        let tracks = ffmpeg.tracks.expect("a stream map is written");
        assert!(
            tracks.iter().any(|(_, track)| track.copy()),
            "the subtitle and attachment stay selected: {tracks:?}"
        );
        assert_eq!(
            tracks.values().filter(|track| !track.copy()).count(),
            2,
            "both audio streams are dropped: {tracks:?}"
        );
    }

    /// A video-only input has nothing to drop, so the flag leaves the settings
    /// unset rather than writing an empty map.
    #[serial]
    #[test]
    fn init_discard_without_matching_tracks_writes_nothing() {
        if !ffmpeg_is_available() || !mkvmerge_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let video = get_test_video();
        let output = temp.path().join("out.mkv");

        condor_cmd(&temp)
            .args([
                "init",
                path_str(&video.path),
                path_str(&output),
                "--concat",
                "mkvmerge",
                "--discard",
                "audio",
            ])
            .assert()
            .success();

        let config_path = temp.path().join("condor.json");
        let concat = load(&config_path).condor.sequence_config.scene_concatenator;
        let ConcatMethod::MKVMerge(mkvmerge) = concat.method else {
            panic!("the method stays mkvmerge, but is {}", concat.method);
        };
        assert!(
            mkvmerge.tracks.is_none(),
            "no map is written for a source without audio: {:?}",
            mkvmerge.tracks
        );
    }
}
