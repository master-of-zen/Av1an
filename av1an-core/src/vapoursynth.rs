use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail};
use once_cell::sync::Lazy;
use path_abs::PathAbs;
use regex::Regex;
use vapoursynth::prelude::*;
use vapoursynth::video_info::VideoInfo;

use super::ChunkMethod;
use crate::util::to_absolute_path;

static VAPOURSYNTH_PLUGINS: Lazy<HashSet<String>> = Lazy::new(|| {
  let environment = Environment::new().expect("Failed to initialize VapourSynth environment");
  let core = environment
    .get_core()
    .expect("Failed to get VapourSynth core");

  let plugins = core.plugins();
  plugins
    .keys()
    .filter_map(|plugin| {
      plugins
        .get::<&[u8]>(plugin)
        .ok()
        .and_then(|slice| simdutf8::basic::from_utf8(slice).ok())
        .and_then(|s| s.split(';').nth(1))
        .map(ToOwned::to_owned)
    })
    .collect()
});

pub fn is_lsmash_installed() -> bool {
  static LSMASH_PRESENT: Lazy<bool> =
    Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("systems.innocent.lsmas"));

  *LSMASH_PRESENT
}

pub fn is_ffms2_installed() -> bool {
  static FFMS2_PRESENT: Lazy<bool> =
    Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.ffms2"));

  *FFMS2_PRESENT
}

pub fn is_dgdecnv_installed() -> bool {
  static DGDECNV_PRESENT: Lazy<bool> =
    Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.dgdecodenv"));

  *DGDECNV_PRESENT
}

pub fn is_bestsource_installed() -> bool {
  static BESTSOURCE_PRESENT: Lazy<bool> =
    Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.bestsource"));

  *BESTSOURCE_PRESENT
}

pub fn best_available_chunk_method() -> ChunkMethod {
  if is_lsmash_installed() {
    ChunkMethod::LSMASH
  } else if is_ffms2_installed() {
    ChunkMethod::FFMS2
  } else if is_dgdecnv_installed() {
    ChunkMethod::DGDECNV
  } else if is_bestsource_installed() {
    ChunkMethod::BESTSOURCE
  } else {
    ChunkMethod::Hybrid
  }
}

fn get_clip_info(env: &Environment) -> VideoInfo {
  // Get the output node.
  const OUTPUT_INDEX: i32 = 0;

  #[cfg(feature = "vapoursynth_new_api")]
  let (node, _) = env.get_output(OUTPUT_INDEX).unwrap();
  #[cfg(not(feature = "vapoursynth_new_api"))]
  let node = env.get_output(OUTPUT_INDEX).unwrap();

  node.info()
}

/// Get the number of frames from an environment that has already been
/// evaluated on a script.
fn get_num_frames(env: &Environment) -> anyhow::Result<usize> {
  let info = get_clip_info(env);

  let num_frames = {
    if Property::Variable == info.format {
      bail!("Cannot output clips with varying format");
    }
    if Property::Variable == info.resolution {
      bail!("Cannot output clips with varying dimensions");
    }
    if Property::Variable == info.framerate {
      bail!("Cannot output clips with varying framerate");
    }

    #[cfg(feature = "vapoursynth_new_api")]
    let num_frames = info.num_frames;

    #[cfg(not(feature = "vapoursynth_new_api"))]
    let num_frames = {
      match info.num_frames {
        Property::Variable => {
          bail!("Cannot output clips with unknown length");
        }
        Property::Constant(x) => x,
      }
    };

    num_frames
  };

  assert!(num_frames != 0, "vapoursynth reported 0 frames");

  Ok(num_frames)
}

fn get_frame_rate(env: &Environment) -> anyhow::Result<f64> {
  let info = get_clip_info(env);

  match info.framerate {
    Property::Variable => bail!("Cannot output clips with varying framerate"),
    Property::Constant(fps) => Ok(fps.numerator as f64 / fps.denominator as f64),
  }
}

/// Get the bit depth from an environment that has already been
/// evaluated on a script.
fn get_bit_depth(env: &Environment) -> anyhow::Result<usize> {
  let info = get_clip_info(env);

  let bits_per_sample = {
    match info.format {
      Property::Variable => {
        bail!("Cannot output clips with variable format");
      }
      Property::Constant(x) => x.bits_per_sample(),
    }
  };

  Ok(bits_per_sample as usize)
}

/// Get the resolution from an environment that has already been
/// evaluated on a script.
fn get_resolution(env: &Environment) -> anyhow::Result<(u32, u32)> {
  let info = get_clip_info(env);

  let resolution = {
    match info.resolution {
      Property::Variable => {
        bail!("Cannot output clips with variable resolution");
      }
      Property::Constant(x) => x,
    }
  };

  Ok((resolution.width as u32, resolution.height as u32))
}

/// Get the transfer characteristics from an environment that has already been
/// evaluated on a script.
fn get_transfer(env: &Environment) -> anyhow::Result<u8> {
  // Get the output node.
  const OUTPUT_INDEX: i32 = 0;

  #[cfg(feature = "vapoursynth_new_api")]
  let (node, _) = env.get_output(OUTPUT_INDEX).unwrap();
  #[cfg(not(feature = "vapoursynth_new_api"))]
  let node = env.get_output(OUTPUT_INDEX).unwrap();

  let frame = node.get_frame(0)?;
  let transfer = frame
    .props()
    .get::<i64>("_Transfer")
    .map_err(|_| anyhow::anyhow!("Failed to get transfer characteristics from VS script"))?
    as u8;

  Ok(transfer)
}

pub fn create_vs_file(
  temp: &str,
  source: &Path,
  chunk_method: ChunkMethod,
  scene_detection_downscale_height: Option<usize>,
  scene_detection_pixel_format: Option<ffmpeg::format::Pixel>,
  scene_detection_scaler: String,
) -> anyhow::Result<PathBuf> {
  let temp: &Path = temp.as_ref();
  let source = to_absolute_path(source)?;

  let load_script_path = temp.join("split").join("loadscript.vpy");
  let mut load_script = File::create(&load_script_path)?;

  let cache_file = PathAbs::new(temp.join("split").join(format!(
    "cache.{}",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffindex",
      ChunkMethod::LSMASH => "lwi",
      ChunkMethod::DGDECNV => "dgi",
      ChunkMethod::BESTSOURCE => "bsindex",
      _ => return Err(anyhow!("invalid chunk method")),
    }
  )))?;
  let chunk_method_lower = match chunk_method {
    ChunkMethod::FFMS2 => "ffms2",
    ChunkMethod::LSMASH => "lsmash",
    ChunkMethod::DGDECNV => "dgdecnv",
    ChunkMethod::BESTSOURCE => "bestsource",
    _ => return Err(anyhow!("invalid chunk method")),
  };

  // Only used for DGDECNV
  let dgindex_path = match chunk_method {
    ChunkMethod::DGDECNV => {
      let dgindexnv_output = temp.join("split").join("index.dgi");

      // Run dgindexnv to generate the .dgi index file
      Command::new("dgindexnv")
        .arg("-h")
        .arg("-i")
        .arg(&source)
        .arg("-o")
        .arg(&dgindexnv_output)
        .output()?;

      &to_absolute_path(&dgindexnv_output)?
    }
    _ => &source,
  };

  // Include rich loadscript.vpy and specify source, chunk_method, and cache_file
  // Also specify downscale_height, pixel_format, and scaler for Scene Detection
  // TODO should probably check if the syntax for rust strings and escaping utf and stuff like that is the same as in python
  let mut load_script_text = include_str!("loadscript.vpy")
    .replace(
      "source = os.environ.get('AV1AN_SOURCE', None)",
      &format!(
        "source = r\"{}\"",
        match chunk_method {
          ChunkMethod::DGDECNV => dgindex_path.display(),
          _ => source.display(),
        }
      ),
    )
    .replace(
      "chunk_method = os.environ.get('AV1AN_CHUNK_METHOD', None)",
      &format!("chunk_method = {chunk_method_lower:?}"),
    )
    .replace(
      "cache_file = os.environ.get('AV1AN_CACHE_FILE', None)",
      &format!("cache_file = {:?}", cache_file),
    );

  if let Some(scene_detection_downscale_height) = scene_detection_downscale_height {
    load_script_text = load_script_text.replace(
      "downscale_height = os.environ.get('AV1AN_DOWNSCALE_HEIGHT', None)",
      &format!("downscale_height = {scene_detection_downscale_height}"),
    );
  }
  if let Some(scene_detection_pixel_format) = scene_detection_pixel_format {
    load_script_text = load_script_text.replace(
      "sc_pix_format = os.environ.get('AV1AN_PIXEL_FORMAT', None)",
      &format!("pixel_format = \"{scene_detection_pixel_format:?}\""),
    );
  }
  load_script_text = load_script_text.replace(
    "scaler = os.environ.get('AV1AN_SCALER', None)",
    &format!("scaler = {scene_detection_scaler:?}"),
  );

  load_script.write_all(load_script_text.as_bytes())?;

  Ok(load_script_path)
}

pub fn copy_vs_file(
  temp: &str,
  source: &Path,
  downscale_height: Option<usize>,
) -> anyhow::Result<PathBuf> {
  let temp: &Path = temp.as_ref();
  let scd_script_path = temp.join("split").join("scene_detection.vpy");
  let mut scd_script = File::create(&scd_script_path)?;

  let source_script = std::fs::read_to_string(source)?;
  if let Some(downscale_height) = downscale_height {
    let regex = Regex::new(r"(\w+).set_output\(").unwrap();
    if let Some(captures) = regex.captures(&source_script) {
      let output_variable_name = captures.get(1).unwrap().as_str();
      let injected_script = regex
        .replace(
          &source_script,
          format!("{output_variable_name}.resize.Bicubic(width=int((({output_variable_name}.width / {output_variable_name}.height) * int({downscale_height})) // 2 * 2), height={downscale_height}).set_output(").as_str(),
        )
        .to_string();
      scd_script.write_all(injected_script.as_bytes())?;
    }
  }

  Ok(scd_script_path)
}

pub fn num_frames(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_num_frames(&environment)
}

pub fn bit_depth(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_bit_depth(&environment)
}

pub fn frame_rate(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<f64> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_frame_rate(&environment)
}

pub fn resolution(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<(u32, u32)> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_resolution(&environment)
}

/// Transfer characteristics as specified in ITU-T H.265 Table E.4.
pub fn transfer_characteristics(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<u8> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_transfer(&environment)
}

pub fn pixel_format(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<String> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  if environment.set_variables(&vspipe_args_map).is_err() {
    bail!("Failed to set vspipe arguments");
  };

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  let info = get_clip_info(&environment);
  match info.format {
    Property::Variable => bail!("Variable pixel format not supported"),
    Property::Constant(x) => Ok(x.name().to_string()),
  }
}
