use std::thread;

use ansi_term::Style;
use anyhow::bail;
use av_metrics_decoders::{Decoder2, FfmpegDecoder, VapoursynthDecoder};
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};
use ffmpeg::format::Pixel;
use itertools::Itertools;
use vapoursynth::prelude::*;

use crate::scenes::Scene;
use crate::{progress_bar, Encoder, Input, ScenecutMethod, Verbosity};

pub fn av_scenechange_detect(
  input: &Input,
  encoder: Encoder,
  total_frames: usize,
  min_scene_len: usize,
  verbosity: Verbosity,
  sc_pix_format: Option<Pixel>,
  sc_method: ScenecutMethod,
  sc_downscale_height: Option<usize>,
  zones: &[Scene],
) -> anyhow::Result<(Vec<Scene>, usize)> {
  if verbosity != Verbosity::Quiet {
    if atty::is(atty::Stream::Stderr) {
      eprintln!("{}", Style::default().bold().paint("Scene detection"));
    } else {
      eprintln!("Scene detection");
    }
    progress_bar::init_progress_bar(total_frames as u64);
  }

  let input2 = input.clone();
  let frame_thread = thread::spawn(move || {
    let frames = input2.frames().unwrap();
    if verbosity != Verbosity::Quiet {
      progress_bar::convert_to_progress();
      progress_bar::set_len(frames as u64);
    }
    frames
  });

  let mut ff_ctx;
  let mut ff_decoder;

  let mut vs_ctx;
  let mut vs_decoder;

  let scenes = match input {
    Input::Video(path) => {
      ff_ctx = FfmpegDecoder::get_ctx(path).unwrap();
      ff_decoder = FfmpegDecoder::new(&mut ff_ctx).unwrap();

      scene_detect(
        &mut ff_decoder,
        encoder,
        total_frames,
        if verbosity == Verbosity::Quiet {
          None
        } else {
          Some(&|frames, _| {
            progress_bar::set_pos(frames as u64);
          })
        },
        min_scene_len,
        sc_pix_format,
        sc_method,
        sc_downscale_height,
        zones,
      )?
    }
    Input::VapourSynth(path) => {
      // TODO make helper function for creating VS Environment
      vs_ctx = Environment::new().unwrap();

      // Evaluate the script.
      vs_ctx.eval_file(path, EvalFlags::SetWorkingDir).unwrap();

      vs_decoder = VapoursynthDecoder::new(&vs_ctx).unwrap();

      scene_detect(
        &mut vs_decoder,
        encoder,
        total_frames,
        if verbosity == Verbosity::Quiet {
          None
        } else {
          Some(&|frames, _| {
            progress_bar::set_pos(frames as u64);
          })
        },
        min_scene_len,
        sc_pix_format,
        sc_method,
        sc_downscale_height,
        zones,
      )?
    }
  };

  let frames = frame_thread.join().unwrap();

  progress_bar::finish_progress_bar();

  Ok((scenes, frames))
}

/// Detect scene changes using rav1e scene detector.
#[allow(clippy::option_if_let_else)]
pub fn scene_detect<F, D: Decoder2<F>>(
  decoder: &mut D,
  // TODO use these fields
  _encoder: Encoder,
  total_frames: usize,
  callback: Option<&dyn Fn(usize, usize)>,
  min_scene_len: usize,
  // TODO use these fields
  _sc_pix_format: Option<Pixel>,
  sc_method: ScenecutMethod,
  _sc_downscale_height: Option<usize>,
  zones: &[Scene],
) -> anyhow::Result<Vec<Scene>> {
  let bit_depth = decoder.get_bit_depth();

  let mut scenes = Vec::new();
  let mut cur_zone = zones.first().filter(|frame| frame.start_frame == 0);
  let mut next_zone_idx = if zones.is_empty() {
    None
  } else if cur_zone.is_some() {
    if zones.len() == 1 {
      None
    } else {
      Some(1)
    }
  } else {
    Some(0)
  };
  let mut frames_read = 0;
  loop {
    let mut min_scene_len = min_scene_len;
    if let Some(zone) = cur_zone {
      if let Some(ref overrides) = zone.zone_overrides {
        min_scene_len = overrides.min_scene_len;
      }
    };
    let options = DetectionOptions {
      min_scenecut_distance: Some(min_scene_len),
      analysis_speed: match sc_method {
        ScenecutMethod::Fast => SceneDetectionSpeed::Fast,
        ScenecutMethod::Standard => SceneDetectionSpeed::Standard,
      },
      ..DetectionOptions::default()
    };
    let frame_limit = if let Some(zone) = cur_zone {
      Some(zone.end_frame - zone.start_frame)
    } else if let Some(next_idx) = next_zone_idx {
      let zone = &zones[next_idx];
      Some(zone.start_frame - frames_read)
    } else {
      None
    };
    let callback = callback.map(|cb| {
      |frames, _keyframes| {
        cb(frames + frames_read, 0);
      }
    });
    let sc_result = if bit_depth > 8 {
      detect_scene_changes::<F, D, u16>(
        decoder,
        options,
        frame_limit,
        callback.as_ref().map(|cb| cb as &dyn Fn(usize, usize)),
      )
    } else {
      detect_scene_changes::<F, D, u8>(
        decoder,
        options,
        frame_limit,
        callback.as_ref().map(|cb| cb as &dyn Fn(usize, usize)),
      )
    };
    if let Some(limit) = frame_limit {
      if limit != sc_result.frame_count {
        bail!(
          "Scene change: Expected {} frames but saw {}. This may indicate an issue with the input or filters.",
          limit,
          sc_result.frame_count
        );
      }
    }
    let scene_changes = sc_result.scene_changes;
    for (start, end) in scene_changes.iter().copied().tuple_windows() {
      scenes.push(Scene {
        start_frame: start + frames_read,
        end_frame: end + frames_read,
        zone_overrides: cur_zone.and_then(|zone| zone.zone_overrides.clone()),
      });
    }

    scenes.push(Scene {
      start_frame: scenes
        .last()
        .map(|scene| scene.end_frame)
        .unwrap_or_default(),
      end_frame: if let Some(limit) = frame_limit {
        frames_read += limit;
        frames_read
      } else {
        total_frames
      },
      zone_overrides: cur_zone.and_then(|zone| zone.zone_overrides.clone()),
    });
    if let Some(next_idx) = next_zone_idx {
      if cur_zone.map_or(true, |zone| zone.end_frame == zones[next_idx].start_frame) {
        cur_zone = Some(&zones[next_idx]);
        next_zone_idx = if next_idx + 1 == zones.len() {
          None
        } else {
          Some(next_idx + 1)
        };
      } else {
        cur_zone = None;
      }
    } else if cur_zone.map_or(true, |zone| zone.end_frame == total_frames) {
      // End of video
      break;
    } else {
      cur_zone = None;
    }
  }
  Ok(scenes)
}
