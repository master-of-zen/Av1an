#![allow(clippy::mutex_atomic)]
// This is a mostly drop-in reimplementation of vspipe.
// The main difference is what the errors look like.

// Modified from vspipe example in vapoursynth crate
// https://github.com/YaLTeR/vapoursynth-rs/blob/master/vapoursynth/examples/vspipe.rs
extern crate vapoursynth;

use std::collections::HashSet;
use std::path::Path;

use self::vapoursynth::prelude::*;
use super::ChunkMethod;

use anyhow::anyhow;

pub fn select_chunk_method() -> anyhow::Result<ChunkMethod> {
  // Create a new VSScript environment.
  let environment = Environment::new().map_err(|e| anyhow!("{}", e))?;
  let core = environment.get_core().map_err(|e| anyhow!("{}", e))?;

  let plugins = core.plugins();
  let plugins: HashSet<&str> = plugins
    .keys()
    .filter_map(|plugin| {
      plugins
        .get::<&[u8]>(plugin)
        .ok()
        .and_then(|slice| std::str::from_utf8(slice).ok())
        .and_then(|s| s.split(';').nth(1))
    })
    .collect();

  if plugins.contains("systems.innocent.lsmas") {
    Ok(ChunkMethod::LSMASH)
  } else if plugins.contains("com.vapoursynth.ffms2") {
    Ok(ChunkMethod::FFMS2)
  } else {
    Ok(ChunkMethod::Hybrid)
  }
}

pub fn num_frames(path: &Path) -> anyhow::Result<usize> {
  // Create a new VSScript environment.
  let mut environment = Environment::new().unwrap();

  // Evaluate the script.
  environment
    .eval_file(path, EvalFlags::SetWorkingDir)
    .unwrap();

  // Get the output node.
  let output_index = 0;

  #[cfg(feature = "gte-vsscript-api-31")]
  let (node, alpha_node) = environment.get_output(output_index).context(format!(
    "Couldn't get the output node at index {}",
    output_index
  ))?;
  #[cfg(not(feature = "gte-vsscript-api-31"))]
  let (node, _) = (environment.get_output(output_index).unwrap(), None::<Node>);

  let num_frames = {
    let info = node.info();

    if let Property::Variable = info.format {
      panic!("Cannot output clips with varying format");
    }
    if let Property::Variable = info.resolution {
      panic!("Cannot output clips with varying dimensions");
    }
    if let Property::Variable = info.framerate {
      panic!("Cannot output clips with varying framerate");
    }

    #[cfg(feature = "gte-vapoursynth-api-32")]
    let num_frames = info.num_frames;

    #[cfg(not(feature = "gte-vapoursynth-api-32"))]
    let num_frames = {
      match info.num_frames {
        Property::Variable => {
          // TODO: make it possible?
          panic!("Cannot output clips with unknown length");
        }
        Property::Constant(x) => x,
      }
    };

    num_frames
  };

  Ok(num_frames)
}
