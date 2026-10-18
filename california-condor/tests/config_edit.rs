#[path = "common.rs"]
mod common;
#[path = "config_helpers.rs"]
mod config_helpers;

use andean_condor::models::sequence::{
    benchmarker::BenchmarkerConfig,
    target_quality::types::ProbeStatistic,
};
use california_condor::test_helpers::*;
use common::condor_cmd;
use config_helpers::{init_config, load};

#[cfg(test)]
mod tests {
    use andean_condor::models::input::Input;
    use serial_test::serial;

    use super::*;

    /// The noise scaler defaults stay consistent when nothing is configured
    /// yet, so `info` never fails on a fresh configuration.
    #[serial]
    #[test]
    fn info_works_on_a_fresh_configuration() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        init_config(&temp);

        for args in [
            vec!["detect-scenes", "info"],
            vec!["scale-noise", "info"],
            vec!["benchmark", "info"],
            vec!["optimize-bitrate", "info"],
            vec!["detect-noise", "info"],
            vec!["target-quality", "info"],
            vec!["scale-speed", "info"],
            vec!["concatenate", "info"],
            vec!["quality-check", "info"],
        ] {
            let output =
                condor_cmd(&temp).args(&args).assert().success().get_output().stdout.clone();
            serde_json::from_slice::<serde_json::Value>(&output)
                .unwrap_or_else(|err| panic!("{args:?} prints valid JSON: {err}"));
        }

        // Optional sections report themselves as null until they are set.
        let scaler = load(&temp.path().join("condor.json")).condor.sequence_config.noise_scaler;
        assert!(scaler.is_none(), "noise scaler is unset");
    }

    /// The whole section goes back to null, as the CLI contract promises for
    /// every optional section.
    #[serial]
    #[test]
    fn reset_section_nulls_optional_sections() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        // Nothing configured yet: resetting the section is a no-op, not an
        // error, and never materialises it.
        for command in ["detect-noise", "target-quality", "quality-check", "scale-noise"] {
            condor_cmd(&temp).args([command, "reset"]).assert().success();
        }

        condor_cmd(&temp)
            .args(["detect-noise", "set", "reference-filters", "resize:width=960;"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["target-quality", "set", "target", "80"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["quality-check", "set", "statistic", "mean"])
            .assert()
            .success();
        condor_cmd(&temp)
            .args(["scale-noise", "set", "threshold", "0.002"])
            .assert()
            .success();

        let config = load(&config_path);
        assert!(
            config.condor.sequence_config.noise_detector.is_some(),
            "noise detector is materialized"
        );
        assert!(
            config.condor.sequence_config.target_quality.is_some(),
            "target quality is materialized"
        );
        assert!(
            config.condor.sequence_config.quality_check.is_some(),
            "quality check is materialized"
        );
        assert!(
            config.condor.sequence_config.noise_scaler.is_some(),
            "noise scaler is materialized"
        );

        for command in ["detect-noise", "target-quality", "quality-check", "scale-noise"] {
            condor_cmd(&temp).args([command, "reset"]).assert().success();
        }

        let config = load(&config_path);
        assert!(
            config.condor.sequence_config.noise_detector.is_none(),
            "noise detector is null again"
        );
        assert!(
            config.condor.sequence_config.target_quality.is_none(),
            "target quality is null again"
        );
        assert!(
            config.condor.sequence_config.quality_check.is_none(),
            "quality check is null again"
        );
        assert!(
            config.condor.sequence_config.noise_scaler.is_none(),
            "noise scaler is null again"
        );
    }

    /// A write reports the property it changed, which is enough to explain the
    /// run; the value it left behind is printed only under `--verbose`.
    #[serial]
    #[test]
    fn set_and_reset_report_what_changed() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        init_config(&temp);

        let plain = condor_cmd(&temp)
            .args(["benchmark", "set", "threshold", "8"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let plain = String::from_utf8(plain).expect("stdout is UTF-8");
        assert!(plain.contains("threshold updated"), "output is {plain}");
        assert!(
            !plain.contains('8'),
            "the value waits for --verbose: {plain}"
        );

        let verbose = condor_cmd(&temp)
            .args(["--verbose", "benchmark", "set", "threshold", "8"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let verbose = String::from_utf8(verbose).expect("stdout is UTF-8");
        assert!(
            verbose.contains("threshold updated") && verbose.contains('8'),
            "--verbose adds the value: {verbose}"
        );

        let plain = condor_cmd(&temp)
            .args(["benchmark", "reset", "threshold"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let plain = String::from_utf8(plain).expect("stdout is UTF-8");
        assert!(plain.contains("reset to its default"), "output is {plain}");
        assert!(
            !plain.contains(&BenchmarkerConfig::default().threshold.to_string()),
            "the restored value waits for --verbose: {plain}"
        );

        let verbose = condor_cmd(&temp)
            .args(["--verbose", "benchmark", "reset", "threshold"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let verbose = String::from_utf8(verbose).expect("stdout is UTF-8");
        assert!(
            verbose.contains(&BenchmarkerConfig::default().threshold.to_string()),
            "--verbose adds the restored default: {verbose}"
        );
    }

    /// `statistic` accepts positional values for `percentile` and
    /// `standard-deviation-distance`, and rejects giving both positional and
    /// flag forms at once.
    #[serial]
    #[test]
    fn statistic_accepts_positional_values() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);

        condor_cmd(&temp)
            .args(["target-quality", "set", "statistic", "percentile", "90"])
            .assert()
            .success();
        let quality = load(&config_path)
            .condor
            .sequence_config
            .target_quality
            .expect("target quality exists");
        assert_eq!(
            quality.probing.statistic,
            ProbeStatistic::Percentile(90.0),
            "positional percentile is set"
        );

        condor_cmd(&temp)
            .args(["quality-check", "set", "statistic", "standard-deviation-distance", "-1.5"])
            .assert()
            .success();
        let check = load(&config_path)
            .condor
            .sequence_config
            .quality_check
            .expect("quality check exists");
        assert_eq!(
            check.statistic,
            ProbeStatistic::StandardDeviationDistance {
                sigma: -1.5
            },
            "positional sigma is set"
        );

        // Passing both positional and flag is rejected.
        condor_cmd(&temp)
            .args(["target-quality", "set", "statistic", "percentile", "90", "--percentile", "10"])
            .assert()
            .failure();
    }

    /// `--stream-concurrently`, `encode set executable`, and missing-path
    /// warnings work as configured.
    #[serial]
    #[test]
    fn stream_concurrently_executable_and_missing_path_warnings() {
        if !ffmpeg_is_available() {
            return;
        }
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let config_path = init_config(&temp);
        let script = temp.path().join("script.vpy");
        std::fs::write(
            &script,
            "import vapoursynth as vs\ncore = vs.core\nclip = core.std.BlankClip(width=16, \
             height=16, format=vs.YUV420P8)\nclip.set_output(0)\n",
        )
        .expect("script writes");

        condor_cmd(&temp)
            .args([
                "init",
                "set",
                "input",
                "--path",
                path_str(&script),
                "--stream-concurrently",
                "off",
            ])
            .assert()
            .success();
        let input = load(&config_path).condor.input;
        assert!(
            matches!(input, Input::VapourSynthScript {
                stream_concurrently: false,
                ..
            }),
            "stream_concurrently is false: {input:?}"
        );

        // Setting executable on encode warns when the binary path does not exist,
        // and still saves the path; reset clears it.
        let missing_bin = temp.path().join("no-such-encoder");
        let stderr = condor_cmd(&temp)
            .args(["encode", "set", "executable", path_str(&missing_bin)])
            .assert()
            .success()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8(stderr).expect("stderr is UTF-8");
        assert!(
            stderr.contains("does not exist"),
            "missing executable path warns: {stderr}"
        );
        assert_eq!(
            load(&config_path).condor.encoder.custom_executable(),
            Some(&missing_bin),
            "custom executable is saved"
        );

        condor_cmd(&temp).args(["encode", "reset", "executable"]).assert().success();
        assert_eq!(
            load(&config_path).condor.encoder.custom_executable(),
            None,
            "custom executable is cleared"
        );
    }
}
