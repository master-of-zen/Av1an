#[path = "common.rs"]
mod common;
#[path = "config_helpers.rs"]
mod config_helpers;

use std::time::SystemTime;

use andean_condor::models::{
    encoder::photon_noise::PhotonNoise,
    sequence::{noise_detector::NoiseDetectorData, noise_scaler::NoiseScalerConfig},
};
use california_condor::{
    commands::handlers::load_configuration,
    test_helpers::*,
    utils::hash_path::hash_path,
};
use common::condor_cmd;
use config_helpers::{init_config, load};

#[cfg(test)]
mod tests {
    use andean_condor::models::sequence::noise_detector::NoiseDetectorConfig;
    use serial_test::serial;

    use super::*;

    #[serial]
    #[test]
    fn default_and_no_noise_detector_data() {
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

        condor_cmd(&temp)
            .args(["init", path_str(&test_video.path), path_str(&output)])
            .assert()
            .success();

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args(["scale-noise"])
            .assert()
            .success();

        let mut expected_config = default_config(&test_video, &output, &temp_abs);
        expected_config.condor.sequence_config.noise_scaler = Some(NoiseScalerConfig::default());
        // immutable shadow
        let expected_config = expected_config;

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        check_basic_config(&config, &expected_config);
    }

    #[serial]
    #[test]
    fn with_custom_options_and_iso_100() {
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

        // Mock an existing config file with Noise Detector data
        let mut config = default_config(&test_video, &output, &temp_abs);
        config.condor.sequence_config.noise_detector = Some(NoiseDetectorConfig::default());
        config.condor.encoder.set_photon_noise(Some(PhotonNoise {
            iso:        100,
            chroma_iso: None,
            width:      None,
            height:     None,
            c_y:        None,
            ccb:        None,
            ccr:        None,
        }));
        let expected_noise = |index| match index {
            0 => 0.0,
            1 => 0.0003550650245139927,
            2 => 0.0002258349829248295,
            3 => 0.0001817853846724563,
            4 => 0.0000033027436124321854,
            _ => 0.0,
        };
        config.condor.scenes = test_video.mock_scenes(&config.condor.encoder);
        config.condor.scenes.iter_mut().enumerate().for_each(|(index, scene)| {
            scene.encoder.set_photon_noise(Some(PhotonNoise {
                iso:        100,
                chroma_iso: None,
                width:      None,
                height:     None,
                c_y:        None,
                ccb:        None,
                ccr:        None,
            }));
            scene.sequence_data.noise_detection = Some(NoiseDetectorData {
                created_on: SystemTime::now(),
                noise:      expected_noise(index),
                luminance:  0.0,
            });
        });
        config.save(&config_path).expect("configuration save should succeed");

        condor_cmd(&temp)
            .env("CONDOR_TEST_MODE", "1")
            .args([
                "scale-noise",
                "--threshold",
                "0.0002",
                "--minimum-scaler",
                "1.0",
                "--maximum-scaler",
                "2.0",
                "--scale-chroma",
            ])
            .assert()
            .success();

        let mut expected_config = config;
        let expected_isos = |index| match index {
            0 => 100,
            1 => 200,
            2 => 117,
            3 => 100,
            4 => 100,
            _ => 100,
        };
        expected_config.condor.sequence_config.noise_scaler = Some(NoiseScalerConfig {
            threshold:      0.0002,
            minimum_scaler: 1.0,
            maximum_scaler: 2.0,
            scale_chroma:   true,
        });
        expected_config.condor.scenes.iter_mut().enumerate().for_each(|(index, scene)| {
            scene.encoder.set_photon_noise(Some(PhotonNoise {
                iso:        expected_isos(index),
                chroma_iso: None,
                width:      None,
                height:     None,
                c_y:        None,
                ccb:        None,
                ccr:        None,
            }));
            scene.sequence_data.noise_detection = Some(NoiseDetectorData {
                created_on: SystemTime::now(),
                noise:      expected_noise(index),
                luminance:  0.0,
            });
        });

        let (config, _) =
            load_configuration(Some(&config_path)).expect("load_configuration should succeed");

        check_basic_config(&config, &expected_config);
        assert_eq!(
            config.condor.scenes.len(),
            test_video.scenes.len(),
            "scenes contains {} scenes",
            test_video.scenes.len()
        );
        config
            .condor
            .scenes
            .iter()
            .zip(expected_config.condor.scenes.iter())
            .enumerate()
            .for_each(|(index, (scene, expected_scene))| {
                check_encoder(
                    &scene.encoder,
                    &expected_scene.encoder,
                    &format!("scene {} encoder", index),
                );
            });
    }

    /// `scale-noise` materializes its optional section, validates the scaler
    /// bounds, and `reset` resets single settings.
    #[serial]
    #[test]
    fn scale_noise_set_reset_and_validation() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args(["scale-noise", "set", "threshold", "0.002"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scale-noise", "set", "scale-chroma", "on"])
            .assert()
            .success();

        // A minimum above the default maximum (0) is refused.
        condor_cmd(&temp)
            .args(["scale-noise", "set", "minimum-scaler", "1.5"])
            .assert()
            .failure();

        condor_cmd(&temp)
            .args(["scale-noise", "set", "maximum-scaler", "2"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scale-noise", "set", "minimum-scaler", "1.5"])
            .assert()
            .success();

        let scaler = load(&config_path)
            .condor
            .sequence_config
            .noise_scaler
            .expect("noise scaler is set");
        assert_eq!(scaler.threshold, 0.002, "threshold is set");
        assert_eq!(scaler.minimum_scaler, 1.5, "minimum scaler is set");
        assert_eq!(scaler.maximum_scaler, 2.0, "maximum scaler is set");
        assert!(scaler.scale_chroma, "chroma scaling is on");

        condor_cmd(&temp)
            .args(["scale-noise", "reset", "scale-chroma"])
            .assert()
            .success();
        let scaler = load(&config_path)
            .condor
            .sequence_config
            .noise_scaler
            .expect("noise scaler still exists");
        assert!(!scaler.scale_chroma, "chroma scaling is off again");
        assert_eq!(scaler.threshold, 0.002, "the other settings are untouched");
    }
}
