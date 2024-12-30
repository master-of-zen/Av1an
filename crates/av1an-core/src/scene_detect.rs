use std::{
    io::{IsTerminal, Read},
    process::{Command, Stdio},
    thread,
};

use ansi_term::Style;
use anyhow::bail;
use av_scenechange::{
    decoder::Decoder,
    detect_scene_changes,
    ffmpeg::FfmpegDecoder,
    vapoursynth::VapoursynthDecoder,
    DetectionOptions,
    SceneDetectionSpeed,
};
use ffmpeg::format::Pixel;
use itertools::Itertools;
use smallvec::{smallvec, SmallVec};

use crate::{
    into_smallvec,
    progress_bar,
    scenes::Scene,
    Encoder,
    Input,
    ScenecutMethod,
    Verbosity,
};

#[tracing::instrument]
pub fn av_scenechange_detect(
    input: &Input,
    encoder: Encoder,
    total_frames: usize,
    min_scene_len: usize,
    verbosity: Verbosity,
    sc_pix_format: Option<Pixel>,
    sc_method: ScenecutMethod,
    sc_downscale_height: Option<usize>,
) -> anyhow::Result<(Vec<Scene>, usize)> {
    if verbosity != Verbosity::Quiet {
        if std::io::stderr().is_terminal() {
            eprintln!("{}", Style::default().bold().paint("Scene detection"));
        } else {
            eprintln!("Scene detection");
        }
        progress_bar::init_progress_bar(total_frames as u64, 0);
    }

    let input2 = input.clone();
    let frame_thread = thread::spawn(move || {
        let frames = input2.frames().unwrap();
        if verbosity != Verbosity::Quiet {
            progress_bar::convert_to_progress(0);
            progress_bar::set_len(frames as u64);
        }
        frames
    });

    let scenes = scene_detect(
        input,
        encoder,
        total_frames,
        if verbosity == Verbosity::Quiet {
            None
        } else {
            Some(&|frames| {
                progress_bar::set_pos(frames as u64);
            })
        },
        min_scene_len,
        sc_pix_format,
        sc_method,
        sc_downscale_height,
    )?;

    let frames = frame_thread.join().unwrap();

    progress_bar::finish_progress_bar();

    Ok((scenes, frames))
}

pub fn scene_detect(
    input: &Input,
    encoder: Encoder,
    total_frames: usize,
    callback: Option<&dyn Fn(usize)>,
    min_scene_len: usize,
    sc_pix_format: Option<Pixel>,
    sc_method: ScenecutMethod,
    sc_downscale_height: Option<usize>,
) -> anyhow::Result<Vec<Scene>> {
    let (mut decoder, bit_depth) =
        build_decoder(input, encoder, sc_pix_format, sc_downscale_height)?;

    let mut scenes = Vec::new();
    let frames_read = 0;

    let options = DetectionOptions {
        min_scenecut_distance: Some(min_scene_len),
        analysis_speed: match sc_method {
            ScenecutMethod::Fast => SceneDetectionSpeed::Fast,
            ScenecutMethod::Standard => SceneDetectionSpeed::Standard,
        },
        ..DetectionOptions::default()
    };

    let callback = callback.map(|cb| {
        move |frames, _keyframes| {
            cb(frames + frames_read);
        }
    });

    let sc_result = if bit_depth > 8 {
        detect_scene_changes::<_, u16>(
            &mut decoder,
            options,
            None,
            callback
                .as_ref()
                .map(|cb| cb as &dyn Fn(usize, usize)),
        )
    } else {
        detect_scene_changes::<_, u8>(
            &mut decoder,
            options,
            None,
            callback
                .as_ref()
                .map(|cb| cb as &dyn Fn(usize, usize)),
        )
    }?;

    let scene_changes = sc_result.scene_changes;
    for (start, end) in scene_changes.iter().copied().tuple_windows() {
        scenes.push(Scene {
            start_frame: start + frames_read,
            end_frame:   end + frames_read,
        });
    }

    scenes.push(Scene {
        start_frame: scenes
            .last()
            .map(|scene| scene.end_frame)
            .unwrap_or_default(),
        end_frame:   total_frames,
    });

    Ok(scenes)
}

#[tracing::instrument]
fn build_decoder(
    input: &Input,
    encoder: Encoder,
    sc_pix_format: Option<Pixel>,
    sc_downscale_height: Option<usize>,
) -> anyhow::Result<(Decoder<impl Read>, usize)> {
    let bit_depth;
    let filters: SmallVec<[String; 4]> =
        match (sc_downscale_height, sc_pix_format) {
            (Some(sdh), Some(spf)) => into_smallvec![
                "-vf",
                format!(
                    "format={},scale=-2:'min({},ih)'",
                    spf.descriptor().unwrap().name(),
                    sdh,
                )
            ],
            (Some(sdh), None) => {
                into_smallvec![
                    "-vf",
                    format!("scale=-2:'min({sdh},ih)':flags=bicubic")
                ]
            },
            (None, Some(spf)) => {
                into_smallvec!["-pix_fmt", spf.descriptor().unwrap().name()]
            },
            (None, None) => smallvec![],
        };

    let decoder = match input {
        Input::VapourSynth {
            path, ..
        } => {
            bit_depth = crate::vapoursynth::bit_depth(
                path.as_ref(),
                input.as_vspipe_args_map()?,
            )?;
            let vspipe_args = input.as_vspipe_args_vec()?;

            if !filters.is_empty() || !vspipe_args.is_empty() {
                let mut command = Command::new("vspipe");
                command
                    .arg("-c")
                    .arg("y4m")
                    .arg(path)
                    .arg("-")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                // Append vspipe python arguments to the environment if there
                // are any
                for arg in vspipe_args {
                    command.args(["-a", &arg]);
                }
                let vspipe = command.spawn()?.stdout.unwrap();
                Decoder::Y4m(y4m::Decoder::new(
                    Command::new("ffmpeg")
                        .stdin(vspipe)
                        .args([
                            "-i",
                            "pipe:",
                            "-f",
                            "yuv4mpegpipe",
                            "-strict",
                            "-1",
                        ])
                        .args(filters)
                        .arg("-")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()?
                        .stdout
                        .unwrap(),
                )?)
            } else {
                Decoder::Vapoursynth(VapoursynthDecoder::new(path.as_ref())?)
            }
        },
        Input::Video {
            path,
        } => {
            let input_pix_format = av1an_ffmpeg::get_pixel_format(
                path.as_ref(),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "FFmpeg failed to get pixel format for input video: {e:?}"
                )
            });
            bit_depth = encoder.get_format_bit_depth(
                sc_pix_format.unwrap_or(input_pix_format),
            )?;
            if !filters.is_empty() {
                Decoder::Y4m(y4m::Decoder::new(
                    Command::new("ffmpeg")
                        .args(["-r", "1", "-i"])
                        .arg(path)
                        .args(filters.as_ref())
                        .args(["-f", "yuv4mpegpipe", "-strict", "-1", "-"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()?
                        .stdout
                        .unwrap(),
                )?)
            } else {
                Decoder::Ffmpeg(FfmpegDecoder::new(path)?)
            }
        },
    };

    Ok((decoder, bit_depth))
}
