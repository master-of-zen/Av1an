use std::path::PathBuf;

use super::*;

#[test]
fn test_chunk_name_1() {
    let ch = Chunk {
        temp:                  "none".to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        "test.mkv".into(),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::x264,
        noise_size:            (None, None),
        ignore_frame_mismatch: false,
    };
    assert_eq!("00001", ch.name());
}
#[test]
fn test_chunk_name_10000() {
    let ch = Chunk {
        temp:                  "none".to_owned(),
        index:                 10000,
        input:                 Input::Video {
            path:        "test.mkv".into(),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::x264,
        noise_size:            (None, None),
        ignore_frame_mismatch: false,
    };
    assert_eq!("10000", ch.name());
}

#[test]
fn test_chunk_output() {
    let ch = Chunk {
        temp:                  "d".to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        "test.mkv".into(),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::x264,
        noise_size:            (None, None),
        ignore_frame_mismatch: false,
    };
    assert_eq!("d/encode/00001.ivf", ch.output());
}

#[test]
fn test_chunk_frames() {
    let ch = Chunk {
        temp:                  "none".to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        "test.mkv".into(),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           10,
        end_frame:             25,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::x264,
        noise_size:            (None, None),
        ignore_frame_mismatch: false,
    };
    assert_eq!(15, ch.frames());
}

#[test]
fn test_apply_photon_noise_args_with_noise() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let mut ch = Chunk {
        temp:                  temp_dir.path().to_str().unwrap().to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("test-files/blank_1080p.mkv"),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::svt_av1,
        noise_size:            (Some(1920), Some(1080)),
        ignore_frame_mismatch: false,
    };

    ch.apply_photon_noise_args(Some(8), true)?;
    assert!(ch.video_params.iter().any(|p| p.contains("fgs-table")));
    Ok(())
}

#[test]
fn test_apply_photon_noise_args_no_noise() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let mut ch = Chunk {
        temp:                  temp_dir.path().to_str().unwrap().to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("test-files/blank_1080p.mkv"),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::svt_av1,
        noise_size:            (None, None),
        ignore_frame_mismatch: false,
    };

    ch.apply_photon_noise_args(None, false)?;
    assert!(!ch.video_params.iter().any(|p| p.contains("fgs-table")));
    Ok(())
}

#[test]
fn test_apply_photon_noise_args_unsupported_encoder() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let mut ch = Chunk {
        temp:                  temp_dir.path().to_str().unwrap().to_owned(),
        index:                 1,
        input:                 Input::Video {
            path:        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("test-files/blank_1080p.mkv"),
            script_text: None,
        },
        source_cmd:            vec!["".into()],
        output_ext:            "ivf".to_owned(),
        start_frame:           0,
        end_frame:             5,
        frame_rate:            30.0,
        tq_cq:                 None,
        passes:                1,
        video_params:          vec![],
        encoder:               Encoder::x264,
        noise_size:            (Some(1920), Some(1080)),
        ignore_frame_mismatch: false,
    };

    assert!(ch.apply_photon_noise_args(Some(8), true).is_err());
    Ok(())
}
