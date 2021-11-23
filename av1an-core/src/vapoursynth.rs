use std::{
  collections::HashSet,
  fs::File,
  io::Write,
  path::{Path, PathBuf},
  process::{Command, Stdio},
};
use vapoursynth::video_info::VideoInfo;

use once_cell::sync::Lazy;

use super::ChunkMethod;
use path_abs::PathAbs;
use vapoursynth::prelude::*;

use anyhow::{anyhow, bail};

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
        .and_then(|slice| std::str::from_utf8(slice).ok())
        .and_then(|s| s.split(';').nth(1))
        .map(ToOwned::to_owned)
    })
    .collect()
});

pub fn is_lsmash_installed() -> bool {
  VAPOURSYNTH_PLUGINS.contains("systems.innocent.lsmas")
}

pub fn is_ffms2_installed() -> bool {
  VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.ffms2")
}

pub fn best_available_chunk_method() -> ChunkMethod {
  if is_lsmash_installed() {
    ChunkMethod::LSMASH
  } else if is_ffms2_installed() {
    ChunkMethod::FFMS2
  } else {
    ChunkMethod::Hybrid
  }
}

fn get_clip_info(env: &mut Environment) -> VideoInfo {
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
fn get_num_frames(env: &mut Environment) -> anyhow::Result<usize> {
  let info = get_clip_info(env);

  let num_frames = {
    if let Property::Variable = info.format {
      bail!("Cannot output clips with varying format");
    }
    if let Property::Variable = info.resolution {
      bail!("Cannot output clips with varying dimensions");
    }
    if let Property::Variable = info.framerate {
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

/// Get the bit depth from an environment that has already been
/// evaluated on a script.
fn get_bit_depth(env: &mut Environment) -> anyhow::Result<usize> {
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

pub fn create_vs_file(
  temp: &str,
  source: &Path,
  chunk_method: ChunkMethod,
) -> anyhow::Result<PathBuf> {
  let temp: &Path = temp.as_ref();
  let source = Path::new(source).canonicalize()?;

  let load_script_path = temp.join("split").join("loadscript.vpy");

  if load_script_path.exists() {
    return Ok(load_script_path);
  }
  let mut load_script = File::create(&load_script_path)?;

  let cache_file = PathAbs::new(temp.join("split").join(format!(
    "cache.{}",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffindex",
      ChunkMethod::LSMASH => "lwi",
      _ => return Err(anyhow!("invalid chunk method")),
    }
  )))?;

  load_script.write_all(
    // TODO should probably check if the syntax for rust strings and escaping utf and stuff like that is the same as in python
    format!(
      "from vapoursynth import core\n\
      core.max_cache_size=1024\n\
core.{}({:?}, cachefile={:?}).set_output()",
      match chunk_method {
        ChunkMethod::FFMS2 => "ffms2.Source",
        ChunkMethod::LSMASH => "lsmas.LWLibavSource",
        _ => unreachable!(),
      },
      source,
      cache_file
    )
    .as_bytes(),
  )?;

  // TODO use vapoursynth crate instead
  Command::new("vspipe")
    .arg("-i")
    .arg(&load_script_path)
    .args(["-i", "-"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?
    .wait()?;

  Ok(load_script_path)
}

pub fn num_frames(source: &Path) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_num_frames(&mut environment)
}

pub fn bit_depth(source: &Path) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_bit_depth(&mut environment)
}
