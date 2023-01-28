use std::collections::HashMap;
use std::process::{exit, Command};
use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use itertools::Itertools;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_till, take_while};
use nom::character::complete::{char, digit1, space1};
use nom::combinator::{map, map_res, opt, recognize, rest};
use nom::multi::{many1, separated_list0};
use nom::sequence::{preceded, tuple};
use serde::{Deserialize, Serialize};

use crate::parse::valid_params;
use crate::settings::{invalid_params, suggest_fix, EncodeArgs};
use crate::Encoder;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Scene {
  pub start_frame: usize,
  // Reminding again that end_frame is *exclusive*
  pub end_frame: usize,
  pub zone_overrides: Option<ZoneOptions>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ZoneOptions {
  pub encoder: Encoder,
  pub passes: u8,
  pub video_params: Vec<String>,
  pub photon_noise: Option<u8>,
  pub extra_splits_len: Option<usize>,
  pub min_scene_len: usize,
}

impl Scene {
  pub fn parse_from_zone(input: &str, encode_args: &EncodeArgs) -> Result<Self> {
    let (_, (start, _, end, _, encoder, reset, zone_args)): (
      _,
      (usize, _, usize, _, Encoder, bool, &str),
    ) = tuple::<_, _, nom::error::Error<&str>, _>((
      map_res(digit1, str::parse),
      many1(char(' ')),
      map_res(alt((tag("-1"), digit1)), |res: &str| {
        if res == "-1" {
          Ok(encode_args.frames)
        } else {
          res.parse::<usize>()
        }
      }),
      many1(char(' ')),
      map_res(
        alt((
          tag("aom"),
          tag("rav1e"),
          tag("x264"),
          tag("x265"),
          tag("vpx"),
          tag("svt-av1"),
        )),
        Encoder::from_str,
      ),
      map(
        opt(preceded(many1(char(' ')), tag("reset"))),
        |res: Option<&str>| res.is_some(),
      ),
      map(
        opt(preceded(many1(char(' ')), rest)),
        |res: Option<&str>| res.unwrap_or_default().trim(),
      ),
    ))(input)
    .map_err(|e| anyhow!("Invalid zone file syntax: {}", e))?;
    if start >= end {
      bail!("Start frame must be earlier than the end frame");
    }
    if start >= encode_args.frames || end > encode_args.frames {
      bail!("Start and end frames must not be past the end of the video");
    }
    if encoder.format() != encode_args.encoder.format() {
      bail!(
        "Zone specifies using {}, but this cannot be used in the same file as {}",
        encoder,
        encode_args.encoder,
      );
    }
    if encoder != encode_args.encoder {
      if encoder
        .get_format_bit_depth(encode_args.output_pix_format.format)
        .is_err()
      {
        bail!(
          "Output pixel format {:?} is not supported by {} (used in zones file)",
          encode_args.output_pix_format.format,
          encoder
        );
      }
      if !reset {
        bail!("Zone includes encoder change but previous args were kept. You probably meant to specify \"reset\".");
      }
    }

    // Inherit from encode args or reset to defaults
    let mut video_params = if reset {
      Vec::new()
    } else {
      encode_args.video_params.clone()
    };
    let mut passes = if reset {
      encoder.get_default_pass()
    } else {
      encode_args.passes
    };
    let mut photon_noise = if reset {
      None
    } else {
      encode_args.photon_noise
    };
    let mut extra_splits_len = encode_args.extra_splits_len;
    let mut min_scene_len = encode_args.min_scene_len;

    // Parse overrides
    let zone_args: (&str, Vec<(&str, Option<&str>)>) =
      separated_list0::<_, _, _, nom::error::Error<&str>, _, _>(
        space1,
        tuple((
          recognize(tuple((
            alt((tag("--"), tag("-"))),
            take_till(|c| c == '=' || c == ' '),
          ))),
          opt(preceded(alt((space1, tag("="))), take_while(|c| c != ' '))),
        )),
      )(zone_args)
      .map_err(|e| anyhow!("Invalid zone file syntax: {}", e))?;
    let mut zone_args = zone_args.1.into_iter().collect::<HashMap<_, _>>();
    if let Some(zone_passes) = zone_args.remove("--passes") {
      passes = zone_passes.unwrap().parse().unwrap();
    } else if [Encoder::aom, Encoder::vpx].contains(&encoder) && zone_args.contains_key("--rt") {
      passes = 1;
    }
    if let Some(zone_photon_noise) = zone_args.remove("--photon-noise") {
      photon_noise = Some(zone_photon_noise.unwrap().parse().unwrap());
    }
    if let Some(zone_xs) = zone_args
      .remove("-x")
      .or_else(|| zone_args.remove("--extra-split"))
    {
      extra_splits_len = Some(zone_xs.unwrap().parse().unwrap());
    }
    if let Some(zone_min_scene_len) = zone_args.remove("--min-scene-len") {
      min_scene_len = zone_min_scene_len.unwrap().parse().unwrap();
    }
    let raw_zone_args = if [Encoder::aom, Encoder::vpx].contains(&encoder) {
      zone_args
        .into_iter()
        .map(|(key, value)| value.map_or_else(|| key.to_string(), |value| format!("{key}={value}")))
        .collect::<Vec<String>>()
    } else {
      zone_args
        .keys()
        .map(|&k| Some(k.to_string()))
        .interleave(
          zone_args
            .values()
            .map(|v| v.map(std::string::ToString::to_string)),
        )
        .flatten()
        .collect::<Vec<String>>()
    };

    if !encode_args.force {
      let help_text = {
        let [cmd, arg] = encoder.help_command();
        String::from_utf8(Command::new(cmd).arg(arg).output().unwrap().stdout).unwrap()
      };
      let valid_params = valid_params(&help_text, encoder);
      let interleaved_args: Vec<&str> = raw_zone_args
        .iter()
        .filter_map(|param| {
          if param.starts_with('-') && [Encoder::aom, Encoder::vpx].contains(&encoder) {
            // These encoders require args to be passed using an equal sign,
            // e.g. `--cq-level=30`
            param.split('=').next()
          } else {
            // The other encoders use a space, so we don't need to do extra splitting,
            // e.g. `--crf 30`
            None
          }
        })
        .collect();
      let invalid_params = invalid_params(&interleaved_args, &valid_params);

      for wrong_param in &invalid_params {
        eprintln!("'{wrong_param}' isn't a valid parameter for {encoder}");
        if let Some(suggestion) = suggest_fix(wrong_param, &valid_params) {
          eprintln!("\tDid you mean '{suggestion}'?");
        }
      }

      if !invalid_params.is_empty() {
        println!("\nTo continue anyway, run av1an with '--force'");
        exit(1);
      }
    }

    for arg in raw_zone_args {
      if arg.starts_with("--")
        || (arg.starts_with('-') && arg.chars().nth(1).map_or(false, char::is_alphabetic))
      {
        let key = arg.split_once('=').map_or(arg.as_str(), |split| split.0);
        if let Some(pos) = video_params
          .iter()
          .position(|param| param == key || param.starts_with(&format!("{key}=")))
        {
          video_params.remove(pos);
          if let Some(next) = video_params.get(pos) {
            if !([Encoder::aom, Encoder::vpx].contains(&encoder)
              || next.starts_with("--")
              || (next.starts_with('-') && next.chars().nth(1).map_or(false, char::is_alphabetic)))
            {
              video_params.remove(pos);
            }
          }
        }
      }
      video_params.push(arg);
    }

    Ok(Self {
      start_frame: start,
      end_frame: end,
      zone_overrides: Some(ZoneOptions {
        encoder,
        passes,
        video_params,
        photon_noise,
        extra_splits_len,
        min_scene_len,
      }),
    })
  }
}

#[cfg(test)]
fn get_test_args() -> EncodeArgs {
  use std::path::PathBuf;

  use ffmpeg::format::Pixel;

  use crate::concat::ConcatMethod;
  use crate::settings::{InputPixelFormat, PixelFormat};
  use crate::{
    into_vec, ChunkMethod, ChunkOrdering, Input, ScenecutMethod, SplitMethod, Verbosity,
  };

  EncodeArgs {
    frames: 6900,
    log_file: PathBuf::new(),
    ffmpeg_filter_args: Vec::new(),
    temp: String::new(),
    force: false,
    passes: 2,
    video_params: into_vec!["--cq-level=40", "--cpu-used=0", "--aq-mode=1"],
    output_file: String::new(),
    audio_params: Vec::new(),
    chunk_method: ChunkMethod::LSMASH,
    chunk_order: ChunkOrdering::Random,
    concat: ConcatMethod::FFmpeg,
    encoder: Encoder::aom,
    extra_splits_len: Some(100),
    photon_noise: Some(10),
    chroma_noise: false,
    sc_pix_format: None,
    keep: false,
    max_tries: 3,
    min_scene_len: 10,
    input_pix_format: InputPixelFormat::FFmpeg {
      format: Pixel::YUV420P10LE,
    },
    input: Input::Video(PathBuf::new()),
    output_pix_format: PixelFormat {
      format: Pixel::YUV420P10LE,
      bit_depth: 10,
    },
    resume: false,
    scenes: None,
    split_method: SplitMethod::AvScenechange,
    sc_method: ScenecutMethod::Standard,
    sc_only: false,
    sc_downscale_height: None,
    force_keyframes: Vec::new(),
    target_quality: None,
    verbosity: Verbosity::Normal,
    vmaf: false,
    vmaf_filter: None,
    vmaf_path: None,
    vmaf_res: String::new(),
    vmaf_threads: None,
    workers: 1,
    set_thread_affinity: None,
    vs_script: None,
    zones: None,
  }
}

#[test]
fn validate_zones_args() {
  let input = "45 729 aom --cq-level=20 --photon-noise 4 -x 60 --min-scene-len 12";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args).unwrap();
  assert_eq!(result.start_frame, 45);
  assert_eq!(result.end_frame, 729);

  let zone_overrides = result.zone_overrides.unwrap();
  assert_eq!(zone_overrides.encoder, Encoder::aom);
  assert_eq!(zone_overrides.extra_splits_len, Some(60));
  assert_eq!(zone_overrides.min_scene_len, 12);
  assert_eq!(zone_overrides.photon_noise, Some(4));
  assert!(!zone_overrides
    .video_params
    .contains(&"--cq-level=40".to_owned()));
  assert!(zone_overrides
    .video_params
    .contains(&"--cq-level=20".to_owned()));
  assert!(zone_overrides
    .video_params
    .contains(&"--cpu-used=0".to_owned()));
  assert!(zone_overrides
    .video_params
    .contains(&"--aq-mode=1".to_owned()));
}

#[test]
fn validate_rav1e_zone_with_photon_noise() {
  let input = "45 729 rav1e reset --speed 6 --photon-noise 4";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args).unwrap();
  assert_eq!(result.start_frame, 45);
  assert_eq!(result.end_frame, 729);

  let zone_overrides = result.zone_overrides.unwrap();
  assert_eq!(zone_overrides.encoder, Encoder::rav1e);
  assert_eq!(zone_overrides.photon_noise, Some(4));
  assert!(zone_overrides
    .video_params
    .windows(2)
    .any(|window| window[0] == "--speed" && window[1] == "6"));
}

#[test]
fn validate_zones_reset() {
  let input = "729 1337 aom reset --cq-level=20 --cpu-used=5";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args).unwrap();
  assert_eq!(result.start_frame, 729);
  assert_eq!(result.end_frame, 1337);

  let zone_overrides = result.zone_overrides.unwrap();
  assert_eq!(zone_overrides.encoder, Encoder::aom);
  // In the current implementation, scenecut settings should be preserved
  // unless manually overridden. Settings which affect the encoder,
  // including photon noise, should be reset.
  assert_eq!(zone_overrides.extra_splits_len, Some(100));
  assert_eq!(zone_overrides.min_scene_len, 10);
  assert_eq!(zone_overrides.photon_noise, None);
  assert!(!zone_overrides
    .video_params
    .contains(&"--cq-level=40".to_owned()));
  assert!(!zone_overrides
    .video_params
    .contains(&"--cpu-used=0".to_owned()));
  assert!(!zone_overrides
    .video_params
    .contains(&"--aq-mode=1".to_owned()));
  assert!(zone_overrides
    .video_params
    .contains(&"--cq-level=20".to_owned()));
  assert!(zone_overrides
    .video_params
    .contains(&"--cpu-used=5".to_owned()));
}

#[test]
fn validate_zones_encoder_changed() {
  let input = "729 1337 rav1e reset -s 3 -q 45";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args).unwrap();
  assert_eq!(result.start_frame, 729);
  assert_eq!(result.end_frame, 1337);

  let zone_overrides = result.zone_overrides.unwrap();
  assert_eq!(zone_overrides.encoder, Encoder::rav1e);
  assert_eq!(zone_overrides.extra_splits_len, Some(100));
  assert_eq!(zone_overrides.min_scene_len, 10);
  assert_eq!(zone_overrides.photon_noise, None);
  assert!(!zone_overrides
    .video_params
    .contains(&"--cq-level=40".to_owned()));
  assert!(!zone_overrides
    .video_params
    .contains(&"--cpu-used=0".to_owned()));
  assert!(!zone_overrides
    .video_params
    .contains(&"--aq-mode=1".to_owned()));
  assert!(zone_overrides
    .video_params
    .windows(2)
    .any(|window| window[0] == "-s" && window[1] == "3"));
  assert!(zone_overrides
    .video_params
    .windows(2)
    .any(|window| window[0] == "-q" && window[1] == "45"));
}

#[test]
fn validate_zones_encoder_changed_no_reset() {
  let input = "729 1337 rav1e -s 3 -q 45";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args);
  assert_eq!(
    result.err().unwrap().to_string(),
    "Zone includes encoder change but previous args were kept. You probably meant to specify \"reset\"."
  );
}

#[test]
fn validate_zones_no_args() {
  let input = "2459 5000 rav1e";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args);
  assert_eq!(
    result.err().unwrap().to_string(),
    "Zone includes encoder change but previous args were kept. You probably meant to specify \"reset\"."
  );
}

#[test]
fn validate_zones_format_mismatch() {
  let input = "5000 -1 x264 reset";
  let args = get_test_args();
  let result = Scene::parse_from_zone(input, &args);
  assert_eq!(
    result.err().unwrap().to_string(),
    "Zone specifies using x264, but this cannot be used in the same file as aom"
  );
}

#[test]
fn validate_zones_no_args_reset() {
  let input = "5000 -1 rav1e reset";
  let args = get_test_args();

  // This is weird, but can technically work for some encoders so we'll allow it.
  let result = Scene::parse_from_zone(input, &args).unwrap();
  assert_eq!(result.start_frame, 5000);
  assert_eq!(result.end_frame, 6900);

  let zone_overrides = result.zone_overrides.unwrap();
  assert_eq!(zone_overrides.encoder, Encoder::rav1e);
  assert_eq!(zone_overrides.extra_splits_len, Some(100));
  assert_eq!(zone_overrides.min_scene_len, 10);
  assert_eq!(zone_overrides.photon_noise, None);
  assert!(zone_overrides.video_params.is_empty());
}
