use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::{absolute, Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail};
use once_cell::sync::Lazy;
use path_abs::PathAbs;
use regex::Regex;
use vapoursynth::core::CoreRef;
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

pub fn is_julek_installed() -> bool {
  static JULEK_PRESENT: Lazy<bool> = Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.julek.plugin"));

  *JULEK_PRESENT
}

pub fn is_vszip_installed() -> bool {
  static VSZIP_PRESENT: Lazy<bool> = Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.julek.vszip"));

  *VSZIP_PRESENT
}

pub fn is_vship_installed() -> bool {
  static VSHIP_PRESENT: Lazy<bool> = Lazy::new(|| VAPOURSYNTH_PLUGINS.contains("com.lumen.vship"));

  *VSHIP_PRESENT
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PluginId {
  Std,
  Resize,
  Lsmash,
  Ffms2,
  Bestsource,
  Julek,
  Vszip,
  Vship,
}

impl PluginId {
  const fn as_str(self) -> &'static str {
    match self {
      PluginId::Std => "com.vapoursynth.std",
      PluginId::Resize => "com.vapoursynth.resize",
      PluginId::Lsmash => "systems.innocent.lsmas",
      PluginId::Ffms2 => "com.vapoursynth.ffms2",
      PluginId::Bestsource => "com.vapoursynth.bestsource",
      PluginId::Julek => "com.julek.plugin",
      PluginId::Vszip => "com.julek.vszip",
      PluginId::Vship => "com.lumen.vship",
    }
  }
}

fn get_plugin(core: CoreRef, plugin_id: PluginId) -> anyhow::Result<Plugin> {
  let plugin = core.get_plugin_by_id(plugin_id.as_str())?;

  match plugin {
    Some(plugin) => Ok(plugin),
    None => bail!("Failed to get VapourSynth {} plugin", plugin_id.as_str()),
  }
}

fn import_lsmash<'core>(
  core: CoreRef<'core>,
  encoded: &Path,
  cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let lsmash = get_plugin(core, PluginId::Lsmash)?;
  let absolute_encoded_path = absolute(encoded)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set(
    "source",
    &absolute_encoded_path.as_os_str().as_encoded_bytes(),
  )?;
  // Enable cache by default.
  if let Some(cache) = cache {
    arguments.set_int(
      "cache",
      match cache {
        true => 1,
        false => 0,
      },
    )?;
  }
  arguments.set_int("prefer_hw", 3)?;

  Ok(
    lsmash
      .invoke("LWLibavSource", &arguments)?
      .get_node("clip")?,
  )
}

fn import_ffms2<'core>(
  core: CoreRef<'core>,
  encoded: &Path,
  cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let ffms2 = get_plugin(core, PluginId::Ffms2)?;
  let absolute_encoded_path = absolute(encoded)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set(
    "source",
    &absolute_encoded_path.as_os_str().as_encoded_bytes(),
  )?;

  // Enable cache by default.
  if let Some(cache) = cache {
    arguments.set_int(
      "cache",
      match cache {
        true => 1,
        false => 0,
      },
    )?;
  }

  Ok(ffms2.invoke("Source", &arguments)?.get_node("clip")?)
}

fn import_bestsource<'core>(
  core: CoreRef<'core>,
  encoded: &Path,
  cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let bestsource = get_plugin(core, PluginId::Bestsource)?;
  let absolute_encoded_path = absolute(encoded)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set(
    "source",
    &absolute_encoded_path.as_os_str().as_encoded_bytes(),
  )?;

  // Enable cache by default.
  // Always try to read index but only write index to disk when it will make a noticeable difference
  // on subsequent runs and store index files in the absolute path in *cachepath* with track number and
  // index extension appended
  if let Some(cache) = cache {
    arguments.set_int(
      "cachemode",
      match cache {
        true => 3,
        false => 0,
      },
    )?;
  }

  Ok(
    bestsource
      .invoke("VideoSource", &arguments)?
      .get_node("clip")?,
  )
}

// Attempts to import video using LSMASH, FFMS2 or BestSource in that order
fn import_video<'core>(
  core: CoreRef<'core>,
  encoded: &Path,
  cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
  import_lsmash(core, encoded, cache).or_else(|_| {
    import_ffms2(core, encoded, cache).or_else(|_| import_bestsource(core, encoded, cache))
  })
}

fn trim_node<'core>(
  core: CoreRef<'core>,
  node: &Node<'core>,
  start: u32,
  end: u32,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let std = get_plugin(core, PluginId::Std)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set("clip", node)?;
  arguments.set("first", &(start as i64))?;
  arguments.set("last", &(end as i64))?;

  Ok(std.invoke("Trim", &arguments)?.get_node("clip")?)
}

fn resize_node<'core>(
  core: CoreRef<'core>,
  node: &Node<'core>,
  width: Option<u32>,
  height: Option<u32>,
  format: Option<PresetFormat>,
  matrix_in_s: Option<&'static str>,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let std = get_plugin(core, PluginId::Resize)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set("clip", node)?;
  if let Some(width) = width {
    arguments.set_int("width", width as i64)?;
  }
  if let Some(height) = height {
    arguments.set_int("height", height as i64)?;
  }
  if let Some(format) = format {
    arguments.set_int("format", format as i64)?;
  }
  if let Some(matrix_in_s) = matrix_in_s {
    arguments.set("matrix_in_s", &matrix_in_s.as_bytes())?;
  }

  Ok(std.invoke("Bicubic", &arguments)?.get_node("clip")?)
}

fn select_every<'core>(
  core: CoreRef<'core>,
  node: &Node<'core>,
  n: usize,
) -> anyhow::Result<Node<'core>> {
  let api = API::get().expect("Failed to get VapourSynth API");
  let std = get_plugin(core, PluginId::Std)?;

  let mut arguments = vapoursynth::map::OwnedMap::new(api);
  arguments.set("clip", node)?;
  arguments.set_int("cycle", n as i64)?;
  arguments.set_int_array("offsets", &[0])?;

  Ok(std.invoke("SelectEvery", &arguments)?.get_node("clip")?)
}

fn compare_ssimulacra2<'core>(
  core: CoreRef<'core>,
  source: &Node<'core>,
  encoded: &Node<'core>,
) -> anyhow::Result<(Node<'core>, &'static str)> {
  let api = API::get().expect("Failed to get VapourSynth API");

  if is_vship_installed() {
    let vship = get_plugin(core, PluginId::Vship)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", source)?;
    arguments.set("distorted", encoded)?;
    arguments.set_int("numStream", 4)?;

    return Ok((
      vship.invoke("SSIMULACRA2", &arguments)?.get_node("clip")?,
      "_SSIMULACRA2",
    ));
  } else if is_vszip_installed() {
    let vszip = get_plugin(core, PluginId::Vszip)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", source)?;
    arguments.set("distorted", encoded)?;
    arguments.set_int("mode", 0)?;

    Ok((
      vszip.invoke("Metrics", &arguments)?.get_node("clip")?,
      "_SSIMULACRA2",
    ))
  } else {
    bail!("SSIMULACRA2 not available");
  }
}

fn compare_butteraugli<'core>(
  core: CoreRef<'core>,
  source: &Node<'core>,
  encoded: &Node<'core>,
) -> anyhow::Result<(Node<'core>, &'static str)> {
  let api = API::get().expect("Failed to get VapourSynth API");

  if is_vship_installed() {
    let vship = get_plugin(core, PluginId::Vship)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", source)?;
    arguments.set("distorted", encoded)?;
    arguments.set_float("intensity_multiplier", 80.0)?;
    arguments.set_int("distmap", 1)?;
    arguments.set_int("numStream", 4)?;

    return Ok((
      vship.invoke("BUTTERAUGLI", &arguments)?.get_node("clip")?,
      "_BUTTERAUGLI_INFNorm",
    ));
  } else if is_julek_installed() {
    let julek = get_plugin(core, PluginId::Julek)?;

    let formatted_source = resize_node(
      core,
      source,
      None,
      None,
      Some(PresetFormat::RGBS),
      Some("709"),
    )?;
    let formatted_encoded = resize_node(
      core,
      encoded,
      None,
      None,
      Some(PresetFormat::RGBS),
      Some("709"),
    )?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", &formatted_source)?;
    arguments.set("distorted", &formatted_encoded)?;
    arguments.set_float("intensity_target", 80.0)?;
    arguments.set_int("distmap", 1)?;

    return Ok((
      julek.invoke("Butteraugli", &arguments)?.get_node("clip")?,
      "_FrameButteraugli",
    ));
  } else {
    bail!("Butteraugli not available");
  }
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
      &format!("cache_file = {cache_file:?}"),
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
    let regex = Regex::new(r"(\w+).set_output\(")?;
    if let Some(captures) = regex.captures(&source_script) {
      let output_variable_name = captures.get(1).unwrap().as_str();
      let injected_script = regex
        .replace(
          &source_script,
          format!("{output_variable_name}.resize.Bicubic(width=int((({output_variable_name}.width / {output_variable_name}.height) * int({downscale_height})) // 2 * 2), height={downscale_height}).set_output(").as_str(),
        )
        .to_string();
      scd_script.write_all(injected_script.as_bytes())?;
      return Ok(scd_script_path);
    }
  }

  scd_script.write_all(source_script.as_bytes())?;
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

pub fn get_source_chunk<'core>(
  core: CoreRef<'core>,
  source_node: &Node<'core>,
  frame_range: (u32, u32),
  sample_rate: usize,
) -> anyhow::Result<Node<'core>> {
  let mut chunk_node = trim_node(core, source_node, frame_range.0, frame_range.1 - 1)?;
  if sample_rate > 1 {
    chunk_node = select_every(core, &chunk_node, sample_rate)?;
  }

  Ok(chunk_node)
}

pub fn measure_butteraugli(
  source: &Path,
  encoded: &Path,
  frame_range: (u32, u32),
  sample_rate: usize,
) -> anyhow::Result<Vec<f64>> {
  let source_is_vpy = source.extension().unwrap() == "vpy" || source.extension().unwrap() == "py";
  let mut environment = Environment::new()?;

  if source_is_vpy {
    environment.eval_file(source, EvalFlags::SetWorkingDir)?;
  }

  let core = environment.get_core()?;
  let source_node = if source_is_vpy {
    environment.get_output(0)?.0
  } else {
    import_video(core, source, Some(true))?
  };

  let chunk_node = get_source_chunk(core, &source_node, frame_range, sample_rate)?;
  let encoded_node = import_video(core, encoded, Some(false))?;

  let (compared_node, butteraugli_key) = compare_butteraugli(core, &chunk_node, &encoded_node)?;

  let mut scores = Vec::new();
  for frame_index in 0..compared_node.info().num_frames {
    let score = compared_node
      .get_frame(frame_index)?
      .props()
      .get_float(butteraugli_key)?;
    scores.push(score);
  }

  Ok(scores)
}

pub fn measure_ssimulacra2(
  source: &Path,
  encoded: &Path,
  frame_range: (u32, u32),
  sample_rate: usize,
) -> anyhow::Result<Vec<f64>> {
  // Create a new VS environment
  let source_is_vpy = source.extension().unwrap() == "vpy" || source.extension().unwrap() == "py";
  let mut environment = Environment::new()?;

  // Evaluate if source is a VapourSynth script
  if source_is_vpy {
    environment.eval_file(source, EvalFlags::SetWorkingDir)?;
  }

  let core = environment.get_core()?;
  let source_node = if source_is_vpy {
    environment.get_output(0)?.0
  } else {
    import_video(core, source, Some(true))?
  };

  let chunk_node = get_source_chunk(core, &source_node, frame_range, sample_rate)?;
  let encoded_node = import_video(core, encoded, Some(false))?;

  let (compared_node, ssimulacra_key) = compare_ssimulacra2(core, &chunk_node, &encoded_node)?;
  let mut scores = Vec::new();
  for frame_index in 0..compared_node.info().num_frames {
    let score = compared_node
      .get_frame(frame_index)?
      .props()
      .get_float(ssimulacra_key)?;
    scores.push(score);
  }

  Ok(scores)
}
