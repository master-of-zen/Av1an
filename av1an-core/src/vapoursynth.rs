use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use crate::ffmpeg;

use super::ChunkMethod;

use once_cell::sync::Lazy;
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

pub fn best_available_chunk_method() -> ChunkMethod {
  if VAPOURSYNTH_PLUGINS.contains("systems.innocent.lsmas") {
    ChunkMethod::LSMASH
  } else if VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.ffms2") {
    ChunkMethod::FFMS2
  } else {
    ChunkMethod::Hybrid
  }
}

/// Get the number of frames from an environment that has already been
/// evaluated on a script.
fn get_num_frames(env: &mut Environment) -> anyhow::Result<usize> {
  // Get the output node.
  let output_index = 0;

  #[cfg(feature = "gte-vsscript-api-31")]
  let (node, alpha_node) = environment.get_output(output_index).context(format!(
    "Couldn't get the output node at index {}",
    output_index
  ))?;
  #[cfg(not(feature = "gte-vsscript-api-31"))]
  let (node, _) = (env.get_output(output_index).unwrap(), None::<Node>);

  let num_frames = {
    let info = node.info();

    if let Property::Variable = info.format {
      bail!("Cannot output clips with varying format");
    }
    if let Property::Variable = info.resolution {
      bail!("Cannot output clips with varying dimensions");
    }
    if let Property::Variable = info.framerate {
      bail!("Cannot output clips with varying framerate");
    }

    #[cfg(feature = "gte-vapoursynth-api-32")]
    let num_frames = info.num_frames;

    #[cfg(not(feature = "gte-vapoursynth-api-32"))]
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

  Ok(num_frames)
}

pub static FRAME_COUNT_FN: Lazy<fn(&Path) -> anyhow::Result<usize>> = Lazy::new(|| {
  macro_rules! create_eval_fn {
    ($filter:expr) => {
      |source| {
        let mut environment =
          Environment::new().expect("Failed to initialize VapourSynth environment");

        environment
          .eval_script(&format!(
            concat!(
              "from vapoursynth import core\n",
              "core.",
              $filter,
              "({:?}, cache=False).set_output()"
            ),
            source
          ))
          .unwrap();

        get_num_frames(&mut environment)
      }
    };
  }

  if VAPOURSYNTH_PLUGINS.contains("systems.innocent.lsmas") {
    create_eval_fn!("lsmas.LWLibavSource")
  } else if VAPOURSYNTH_PLUGINS.contains("com.vapoursynth.ffms2") {
    create_eval_fn!("ffms2.Source")
  } else {
    ffmpeg::get_frame_count
  }
});

/// Generates vapoursynth script for either FFMS2 or L-SMASH
fn generate_script(source: &Path, cache_file: &Path, chunk_method: ChunkMethod) -> String {
  format!(
    "from vapoursynth import core
core.{}({:?}, cachefile={:?}).set_output()",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffms2.Source",
      ChunkMethod::LSMASH => "lsmas.LWLibavSource",
      _ => panic!(
        "Chunk method {:?} is not a vapoursynth filter!",
        chunk_method
      ),
    },
    source,
    cache_file
  )
}

pub fn create_vs_file(
  temp: &str,
  source: &str,
  chunk_method: ChunkMethod,
) -> anyhow::Result<String> {
  let temp: &Path = temp.as_ref();
  let source = Path::new(source).canonicalize()?;

  let load_script_path = temp.join("split").join("loadscript.vpy");

  if load_script_path.exists() {
    return Ok(load_script_path.to_string_lossy().to_string());
  }
  let mut load_script = File::create(&load_script_path)?;

  let cache_file = PathAbs::new(temp.join("split").join(format!(
    "cache.{}",
    match chunk_method {
      ChunkMethod::FFMS2 => "ffindex",
      ChunkMethod::LSMASH => "lwi",
      _ => return Err(anyhow!("invalid chunk method")),
    }
  )))
  .unwrap();

  load_script
    .write_all(generate_script(&source, &cache_file.as_path(), chunk_method).as_bytes())?;

  // TODO use vapoursynth crate instead
  Command::new("vspipe")
    .arg("-i")
    .arg(&load_script_path)
    .args(&["-i", "-"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?
    .wait()?;

  Ok(load_script_path.to_string_lossy().to_string())
}

pub fn num_frames_script(source: &Path) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  // Evaluate the script.
  environment
    .eval_file(source, EvalFlags::SetWorkingDir)
    .unwrap();

  get_num_frames(&mut environment)
}
