use av_format::rational::Rational64;

use super::*;

#[test]
fn test_mkvmerge_options_json_no_audio() {
    let result = mkvmerge_options_json(
        2,
        Encoder::aom,
        "output.mkv",
        None,
        Some(Rational64::new(30, 1)),
    );
    assert_eq!(
        result,
        r#"["-o", "output.mkv", "--default-duration", "0:30/1fps", "[", "00000.ivf", "00001.ivf","]"]"#
    );
}

#[test]
fn test_mkvmerge_options_json_with_audio() {
    let result = mkvmerge_options_json(
        2,
        Encoder::aom,
        "output.mkv",
        Some("audio.mkv"),
        Some(Rational64::new(30, 1)),
    );
    assert_eq!(
        result,
        r#"["-o", "output.mkv", "audio.mkv", "--default-duration", "0:30/1fps", "[", "00000.ivf", "00001.ivf","]"]"#
    );
}
