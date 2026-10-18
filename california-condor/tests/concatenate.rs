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
}
