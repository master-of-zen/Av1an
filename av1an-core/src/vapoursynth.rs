use std::{
    collections::HashSet,
    fs::File,
    io::Write,
    path::{absolute, Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, bail};
use av_format::rational::Rational64;
use once_cell::sync::Lazy;
use path_abs::PathAbs;
use regex::Regex;
use tracing::info;
use vapoursynth::{
    core::CoreRef,
    prelude::*,
    video_info::{Resolution, VideoInfo},
};

use super::ChunkMethod;
use crate::{
    metrics::{butteraugli::ButteraugliSubMetric, xpsnr::XPSNRSubMetric},
    util::to_absolute_path,
    Input,
};

static VAPOURSYNTH_PLUGINS: Lazy<HashSet<String>> = Lazy::new(|| {
    let environment = Environment::new().expect("Failed to initialize VapourSynth environment");
    let core = environment.get_core().expect("Failed to get VapourSynth core");

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

#[inline]
pub fn is_lsmash_installed() -> bool {
    static LSMASH_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::Lsmash.as_str()));

    *LSMASH_PRESENT
}

#[inline]
pub fn is_ffms2_installed() -> bool {
    static FFMS2_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::Ffms2.as_str()));

    *FFMS2_PRESENT
}

#[inline]
pub fn is_dgdecnv_installed() -> bool {
    static DGDECNV_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::DGDecNV.as_str()));

    *DGDECNV_PRESENT
}

#[inline]
pub fn is_bestsource_installed() -> bool {
    static BESTSOURCE_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::BestSource.as_str()));

    *BESTSOURCE_PRESENT
}

#[inline]
pub fn is_julek_installed() -> bool {
    static JULEK_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::Julek.as_str()));

    *JULEK_PRESENT
}

#[inline]
pub fn is_vszip_installed() -> bool {
    static VSZIP_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::Vszip.as_str()));

    *VSZIP_PRESENT
}

#[inline]
pub fn is_vship_installed() -> bool {
    static VSHIP_PRESENT: Lazy<bool> =
        Lazy::new(|| VAPOURSYNTH_PLUGINS.contains(PluginId::Vship.as_str()));

    *VSHIP_PRESENT
}

// There is no way to get the version of a plugin
// so check for a function signature instead
#[inline]
pub fn is_vszip_r7_or_newer() -> bool {
    static VSZIP_R7_OR_NEWER: Lazy<bool> = Lazy::new(|| {
        if !is_vszip_installed() {
            return false;
        }
        let environment = Environment::new().expect("Failed to initialize VapourSynth environment");
        let core = environment.get_core().expect("Failed to get VapourSynth core");

        let vszip = get_plugin(core, PluginId::Vszip).expect("Failed to get vszip plugin");
        let functions_map = vszip.functions();
        let functions: Vec<(String, Vec<String>)> = functions_map
            .keys()
            .filter_map(|name| {
                functions_map
                    .get::<&[u8]>(name)
                    .ok()
                    .and_then(|slice| simdutf8::basic::from_utf8(slice).ok())
                    .map(|f| {
                        let mut split = f.split(';');
                        (
                            split.next().expect("Function name is missing").to_string(),
                            split
                                .filter(|s| !s.is_empty())
                                .map(ToOwned::to_owned)
                                .collect::<Vec<String>>(),
                        )
                    })
            })
            .collect();

        // R7 adds XPSNR and also introduces breaking changes the API
        functions.iter().any(|(name, _)| name == "XPSNR")
    });

    *VSZIP_R7_OR_NEWER
}

#[inline]
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

fn get_clip_info(env: &Environment) -> VideoInfo<'_> {
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
                },
                Property::Constant(x) => x,
            }
        };

        num_frames
    };

    assert!(num_frames != 0, "vapoursynth reported 0 frames");

    Ok(num_frames)
}

fn get_frame_rate(env: &Environment) -> anyhow::Result<Rational64> {
    let info = get_clip_info(env);

    match info.framerate {
        Property::Variable => bail!("Cannot output clips with varying framerate"),
        Property::Constant(fps) => Ok(Rational64::new(
            fps.numerator as i64,
            fps.denominator as i64,
        )),
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
            },
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
            },
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
    BestSource,
    DGDecNV,
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
            PluginId::BestSource => "com.vapoursynth.bestsource",
            PluginId::DGDecNV => "com.vapoursynth.dgdecodenv",
            PluginId::Julek => "com.julek.plugin",
            PluginId::Vszip => "com.julek.vszip",
            PluginId::Vship => "com.lumen.vship",
        }
    }
}

fn get_plugin(core: CoreRef, plugin_id: PluginId) -> anyhow::Result<Plugin> {
    let plugin = core.get_plugin_by_id(plugin_id.as_str())?;

    plugin.ok_or(anyhow::anyhow!(
        "Failed to get VapourSynth {plugin_id} plugin",
        plugin_id = plugin_id.as_str()
    ))
}

fn import_lsmash<'core>(
    core: CoreRef<'core>,
    encoded: &Path,
    cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let lsmash = get_plugin(core, PluginId::Lsmash)?;
    let absolute_encoded_path = absolute(encoded)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set(
        "source",
        &absolute_encoded_path.as_os_str().as_encoded_bytes(),
    )?;
    // Enable cache by default.
    if let Some(cache) = cache {
        arguments.set_int("cache", match cache {
            true => 1,
            false => 0,
        })?;
    }
    // Allow hardware acceleration, falls back to software decoding.
    arguments.set_int("prefer_hw", 3)?;

    let error_message = format!(
        "Failed to import {video_path} with lsmash",
        video_path = encoded.display()
    );

    lsmash
        .invoke("LWLibavSource", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

fn import_ffms2<'core>(
    core: CoreRef<'core>,
    encoded: &Path,
    cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let ffms2 = get_plugin(core, PluginId::Ffms2)?;
    let absolute_encoded_path = absolute(encoded)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set(
        "source",
        &absolute_encoded_path.as_os_str().as_encoded_bytes(),
    )?;

    // Enable cache by default.
    if let Some(cache) = cache {
        arguments.set_int("cache", match cache {
            true => 1,
            false => 0,
        })?;
    }

    let error_message = format!(
        "Failed to import {video_path} with ffms2",
        video_path = encoded.display()
    );

    ffms2
        .invoke("Source", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

fn import_bestsource<'core>(
    core: CoreRef<'core>,
    encoded: &Path,
    cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let bestsource = get_plugin(core, PluginId::BestSource)?;
    let absolute_encoded_path = absolute(encoded)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set(
        "source",
        &absolute_encoded_path.as_os_str().as_encoded_bytes(),
    )?;

    // Enable cache by default.
    // Always try to read index but only write index to disk when it will make a
    // noticeable difference on subsequent runs and store index files in the
    // absolute path in *cachepath* with track number and index extension
    // appended
    if let Some(cache) = cache {
        arguments.set_int("cachemode", match cache {
            true => 3,
            false => 0,
        })?;
    }

    let error_message = format!(
        "Failed to import {video_path} with bestsource",
        video_path = encoded.display()
    );

    bestsource
        .invoke("VideoSource", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

// Attempts to import video using FFMS2, BestSource, or LSMASH in that order
fn import_video<'core>(
    core: CoreRef<'core>,
    encoded: &Path,
    cache: Option<bool>,
) -> anyhow::Result<Node<'core>> {
    import_ffms2(core, encoded, cache)
        .or_else(|_| {
            import_bestsource(core, encoded, cache).or_else(|_| import_lsmash(core, encoded, cache))
        })
        .map_err(|_| {
            anyhow::anyhow!(
                "Failed to import {video_path} with any decoder",
                video_path = encoded.display()
            )
        })
}

fn trim_node<'core>(
    core: CoreRef<'core>,
    node: &Node<'core>,
    start: u32,
    end: u32,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let std = get_plugin(core, PluginId::Std)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("clip", node)?;
    arguments.set("first", &(start as i64))?;
    arguments.set("last", &(end as i64))?;

    let error_message = format!("Failed to trim video from {start} to {end}");

    std.invoke("Trim", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

fn resize_node<'core>(
    core: CoreRef<'core>,
    node: &Node<'core>,
    width: Option<u32>,
    height: Option<u32>,
    format: Option<PresetFormat>,
    matrix_in_s: Option<&'static str>,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
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

    let error_message = format!(
        "Failed to resize video to {width}x{height}",
        width = width.unwrap_or(0),
        height = height.unwrap_or(0)
    );

    std.invoke("Bicubic", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

fn select_every<'core>(
    core: CoreRef<'core>,
    node: &Node<'core>,
    n: usize,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let std = get_plugin(core, PluginId::Std)?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("clip", node)?;
    arguments.set_int("cycle", n as i64)?;
    arguments.set_int_array("offsets", &[0])?;

    let error_message = format!("Failed to select 1 of every {n} frames");

    std.invoke("SelectEvery", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

fn compare_ssimulacra2<'core>(
    core: CoreRef<'core>,
    source: &Node<'core>,
    encoded: &Node<'core>,
) -> anyhow::Result<(Node<'core>, &'static str)> {
    if !is_vship_installed() && !is_vszip_installed() {
        return Err(anyhow::anyhow!("SSIMULACRA2 not available"));
    }

    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let plugin = get_plugin(
        core,
        if is_vship_installed() {
            PluginId::Vship
        } else {
            PluginId::Vszip
        },
    )?;

    let error_message = format!(
        "Failed to calculate SSIMULACRA2 with {plugin_id} plugin",
        plugin_id = if is_vship_installed() {
            PluginId::Vship.as_str()
        } else {
            PluginId::Vszip.as_str()
        }
    );

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", source)?;
    arguments.set("distorted", encoded)?;

    if is_vship_installed() {
        arguments.set_int("numStream", 4)?;
    } else if is_vszip_installed() && !is_vszip_r7_or_newer() {
        // Handle older vszip API
        arguments.set_int("mode", 0)?;
    }

    let output = plugin
        .invoke(
            if is_vship_installed() || (is_vszip_installed() && is_vszip_r7_or_newer()) {
                "SSIMULACRA2"
            } else {
                // Handle older vszip API
                "Metrics"
            },
            &arguments,
        )
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?;

    Ok((
        output,
        if is_vship_installed() || (is_vszip_installed() && !is_vszip_r7_or_newer()) {
            "_SSIMULACRA2"
        } else {
            // Handle newer vszip API
            "SSIMULACRA2"
        },
    ))
}

fn compare_butteraugli<'core>(
    core: CoreRef<'core>,
    source: &Node<'core>,
    encoded: &Node<'core>,
    submetric: ButteraugliSubMetric,
) -> anyhow::Result<(Node<'core>, &'static str)> {
    if !is_vship_installed() && !is_julek_installed() {
        return Err(anyhow::anyhow!("butteraugli not available"));
    }

    const INTENSITY: f64 = 80.0;
    let error_message = format!(
        "Failed to calculate butteraugli with {plugin_id} plugin",
        plugin_id = if is_vship_installed() {
            PluginId::Vship.as_str()
        } else {
            PluginId::Julek.as_str()
        }
    );

    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;
    let plugin = get_plugin(
        core,
        if is_vship_installed() {
            PluginId::Vship
        } else {
            PluginId::Julek
        },
    )?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set_int("distmap", 1)?;

    if is_vship_installed() {
        arguments.set("reference", source)?;
        arguments.set("distorted", encoded)?;
        arguments.set_float("intensity_multiplier", INTENSITY)?;
        arguments.set_int("numStream", 4)?;
    } else if is_julek_installed() {
        // Inputs must be in RGBS format
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

        arguments.set("reference", &formatted_source)?;
        arguments.set("distorted", &formatted_encoded)?;
        arguments.set_float("intensity_target", INTENSITY)?;
    }

    let output = plugin
        .invoke(
            if is_vship_installed() {
                "BUTTERAUGLI"
            } else {
                "butteraugli"
            },
            &arguments,
        )
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?;

    Ok((
        output,
        if is_vship_installed() {
            if submetric == ButteraugliSubMetric::InfiniteNorm {
                "_BUTTERAUGLI_INFNorm"
            } else {
                "_BUTTERAUGLI_3Norm"
            }
        } else {
            "_FrameButteraugli"
        },
    ))
}

fn compare_xpsnr<'core>(
    core: CoreRef<'core>,
    source: &Node<'core>,
    encoded: &Node<'core>,
) -> anyhow::Result<Node<'core>> {
    let api = API::get().ok_or(anyhow::anyhow!("Failed to get VapourSynth API"))?;

    if !is_vszip_installed() || !is_vszip_r7_or_newer() {
        return Err(anyhow::anyhow!("XPSNR not available"));
    }

    let plugin = get_plugin(core, PluginId::Vszip)?;

    // XPSNR requires YUV input and a maximum bit depth of 10
    let formatted_source = resize_node(
        core,
        source,
        None,
        None,
        Some(PresetFormat::YUV444P10),
        None,
    )?;
    let formatted_encoded = resize_node(
        core,
        encoded,
        None,
        None,
        Some(PresetFormat::YUV444P10),
        None,
    )?;

    let mut arguments = vapoursynth::map::OwnedMap::new(api);
    arguments.set("reference", &formatted_source)?;
    arguments.set("distorted", &formatted_encoded)?;

    let error_message = format!(
        "Failed to calculate XPSNR with {plugin_id} plugin",
        plugin_id = PluginId::Vszip.as_str()
    );

    plugin
        .invoke("XPSNR", &arguments)
        .map_err(|_| anyhow::anyhow!(error_message.clone()))?
        .get_node("clip")
        .map_err(|_| anyhow::anyhow!(error_message.clone()))
}

#[inline]
pub fn create_vs_file(
    temp: &str,
    source: &Path,
    chunk_method: ChunkMethod,
    scene_detection_downscale_height: Option<usize>,
    scene_detection_pixel_format: Option<ffmpeg::format::Pixel>,
    scene_detection_scaler: String,
) -> anyhow::Result<PathBuf> {
    let load_script_text = generate_loadscript_text(
        temp,
        source,
        chunk_method,
        scene_detection_downscale_height,
        scene_detection_pixel_format,
        scene_detection_scaler,
    )?;

    if chunk_method == ChunkMethod::DGDECNV {
        let absolute_source = to_absolute_path(source)?;
        let temp: &Path = temp.as_ref();
        let dgindexnv_output = temp.join("split").join("index.dgi");

        if !dgindexnv_output.exists() {
            info!("Indexing input with DGDecNV");

            // Run dgindexnv to generate the .dgi index file
            Command::new("dgindexnv")
                .arg("-h")
                .arg("-i")
                .arg(&absolute_source)
                .arg("-o")
                .arg(&dgindexnv_output)
                .output()?;
        }
    }

    let temp: &Path = temp.as_ref();
    let load_script_path = temp.join("split").join("loadscript.vpy");
    let mut load_script = File::create(&load_script_path)?;

    load_script.write_all(load_script_text.as_bytes())?;

    Ok(load_script_path)
}

#[inline]
pub fn generate_loadscript_text(
    temp: &str,
    source: &Path,
    chunk_method: ChunkMethod,
    scene_detection_downscale_height: Option<usize>,
    scene_detection_pixel_format: Option<ffmpeg::format::Pixel>,
    scene_detection_scaler: String,
) -> anyhow::Result<String> {
    let temp: &Path = temp.as_ref();
    let source = to_absolute_path(source)?;

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
            &to_absolute_path(&dgindexnv_output)?
        },
        _ => &source,
    };

    // Include rich loadscript.vpy and specify source, chunk_method, and cache_file
    // Also specify downscale_height, pixel_format, and scaler for Scene Detection
    // TODO should probably check if the syntax for rust strings and escaping utf
    // and stuff like that is the same as in python
    let mut load_script_text = include_str!("loadscript.vpy")
        .replace(
            "source = os.environ.get('AV1AN_SOURCE', None)",
            &format!("source = r\"{}\"", match chunk_method {
                ChunkMethod::DGDECNV => dgindex_path.display(),
                _ => source.display(),
            }),
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

    Ok(load_script_text)
}

#[inline]
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
                    format!(
                        "{output_variable_name}.resize.Bicubic(width=int((({output_variable_name}.\
                         width / {output_variable_name}.height) * int({downscale_height})) // 2 * \
                         2), height={downscale_height}).set_output("
                    )
                    .as_str(),
                )
                .to_string();
            scd_script.write_all(injected_script.as_bytes())?;
            return Ok(scd_script_path);
        }
    }

    scd_script.write_all(source_script.as_bytes())?;
    Ok(scd_script_path)
}

#[inline]
pub fn num_frames(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<usize> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    get_num_frames(&environment)
}

#[inline]
pub fn bit_depth(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<usize> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    get_bit_depth(&environment)
}

#[inline]
pub fn frame_rate(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<Rational64> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    get_frame_rate(&environment)
}

#[inline]
pub fn resolution(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<(u32, u32)> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    get_resolution(&environment)
}

/// Transfer characteristics as specified in ITU-T H.265 Table E.4.
#[inline]
pub fn transfer_characteristics(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<u8> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    get_transfer(&environment)
}

#[inline]
pub fn pixel_format(source: &Path, vspipe_args_map: OwnedMap) -> anyhow::Result<String> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    if environment.set_variables(&vspipe_args_map).is_err() {
        bail!("Failed to set vspipe arguments");
    };

    // Evaluate the script.
    environment.eval_file(source, EvalFlags::SetWorkingDir).unwrap();

    let info = get_clip_info(&environment);
    match info.format {
        Property::Variable => bail!("Variable pixel format not supported"),
        Property::Constant(x) => Ok(x.name().to_string()),
    }
}

#[inline]
pub fn get_source_chunk<'core>(
    core: CoreRef<'core>,
    source_node: &Node<'core>,
    frame_range: (u32, u32),
    probe_res: Option<(u32, u32)>,
    sample_rate: usize,
) -> anyhow::Result<Node<'core>> {
    let mut chunk_node = trim_node(core, source_node, frame_range.0, frame_range.1 - 1)?;

    if let Some((width, height)) = probe_res {
        chunk_node = resize_node(core, &chunk_node, Some(width), Some(height), None, None)?;
    }

    if sample_rate > 1 {
        chunk_node = select_every(core, &chunk_node, sample_rate)?;
    }

    Ok(chunk_node)
}

#[inline]
pub fn get_comparands<'core>(
    core: CoreRef<'core>,
    source_node: &Node<'core>,
    encoded: &Path,
    frame_range: (u32, u32),
    probe_res: Option<&String>,
    sample_rate: usize,
) -> anyhow::Result<(Node<'core>, Node<'core>)> {
    let mut probe_resolution: Option<(u32, u32)> = None;
    if let Some(res) = probe_res {
        let mut parts = res.split('x');
        let width = parts
            .next()
            .and_then(|x| x.parse::<u32>().ok())
            .expect("Invalid probe resolution");
        let height = parts
            .next()
            .and_then(|x| x.parse::<u32>().ok())
            .expect("Invalid probe resolution");
        probe_resolution = Some((width, height));
    }

    let chunk_node = get_source_chunk(
        core,
        source_node,
        frame_range,
        probe_resolution,
        sample_rate,
    )?;
    let encoded_node = import_video(core, encoded, Some(false))?;
    let resized_encoded_node = if let Some((width, height)) = probe_resolution {
        resize_node(core, &encoded_node, Some(width), Some(height), None, None)?
    } else {
        let chunk_node_resolution = chunk_node.info().resolution;
        let (width, height) = match chunk_node_resolution {
            Property::Variable => (0, 0),
            Property::Constant(Resolution {
                width,
                height,
            }) => (width as u32, height as u32),
        };
        resize_node(core, &encoded_node, Some(width), Some(height), None, None)?
    };

    Ok((chunk_node, resized_encoded_node))
}

#[inline]
pub fn measure_butteraugli(
    submetric: ButteraugliSubMetric,
    source: &Input,
    encoded: &Path,
    frame_range: (u32, u32),
    probe_res: Option<&String>,
    sample_rate: usize,
) -> anyhow::Result<Vec<f64>> {
    let mut environment = Environment::new()?;
    let args = source.as_vspipe_args_map()?;
    environment.set_variables(&args)?;
    environment.eval_script(source.as_script_text())?;
    let core = environment.get_core()?;

    let source_node = environment.get_output(0)?.0;
    let (chunk_node, encoded_node) = get_comparands(
        core,
        &source_node,
        encoded,
        frame_range,
        probe_res,
        sample_rate,
    )?;
    let (compared_node, butteraugli_key) =
        compare_butteraugli(core, &chunk_node, &encoded_node, submetric)?;

    let mut scores = Vec::new();
    for frame_index in 0..compared_node.info().num_frames {
        let score = compared_node.get_frame(frame_index)?.props().get_float(butteraugli_key)?;
        scores.push(score);
    }

    Ok(scores)
}

#[inline]
pub fn measure_ssimulacra2(
    source: &Input,
    encoded: &Path,
    frame_range: (u32, u32),
    probe_res: Option<&String>,
    sample_rate: usize,
) -> anyhow::Result<Vec<f64>> {
    let mut environment = Environment::new()?;
    let args = source.as_vspipe_args_map()?;
    environment.set_variables(&args)?;
    environment.eval_script(source.as_script_text())?;
    let core = environment.get_core()?;

    let source_node = environment.get_output(0)?.0;
    let (chunk_node, encoded_node) = get_comparands(
        core,
        &source_node,
        encoded,
        frame_range,
        probe_res,
        sample_rate,
    )?;
    let (compared_node, ssimulacra_key) = compare_ssimulacra2(core, &chunk_node, &encoded_node)?;

    let mut scores = Vec::new();
    for frame_index in 0..compared_node.info().num_frames {
        let score = compared_node.get_frame(frame_index)?.props().get_float(ssimulacra_key)?;
        scores.push(score);
    }

    Ok(scores)
}

#[inline]
pub fn measure_xpsnr(
    submetric: XPSNRSubMetric,
    source: &Input,
    encoded: &Path,
    frame_range: (u32, u32),
    probe_res: Option<&String>,
    sample_rate: usize,
) -> anyhow::Result<Vec<f64>> {
    let mut environment = Environment::new()?;
    let args = source.as_vspipe_args_map()?;
    environment.set_variables(&args)?;
    environment.eval_script(source.as_script_text())?;
    let core = environment.get_core()?;

    let source_node = environment.get_output(0)?.0;
    let (chunk_node, encoded_node) = get_comparands(
        core,
        &source_node,
        encoded,
        frame_range,
        probe_res,
        sample_rate,
    )?;
    let compared_node = compare_xpsnr(core, &chunk_node, &encoded_node)?;

    let mut scores = Vec::new();
    for frame_index in 0..compared_node.info().num_frames {
        let frame = compared_node.get_frame(frame_index)?;
        let xpsnr_y = frame
            .props()
            .get_float("XPSNR_Y")
            .or(Ok::<f64, std::convert::Infallible>(f64::INFINITY))?;
        let xpsnr_u = frame
            .props()
            .get_float("XPSNR_U")
            .or(Ok::<f64, std::convert::Infallible>(f64::INFINITY))?;
        let xpsnr_v = frame
            .props()
            .get_float("XPSNR_V")
            .or(Ok::<f64, std::convert::Infallible>(f64::INFINITY))?;

        match submetric {
            XPSNRSubMetric::Minimum => {
                let minimum = f64::min(xpsnr_y, f64::min(xpsnr_u, xpsnr_v));
                scores.push(minimum);
            },
            XPSNRSubMetric::Weighted => {
                let weighted = -10.0
                    * f64::log10(
                        (4.0 * f64::powf(10.0, -xpsnr_y / 10.0))
                            + f64::powf(10.0, -xpsnr_u / 10.0)
                            + f64::powf(10.0, -xpsnr_v / 10.0),
                    )
                    / 6.0;
                scores.push(weighted);
            },
        }
    }

    Ok(scores)
}
