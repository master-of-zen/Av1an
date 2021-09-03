use std::{
  collections::HashSet,
  fs::File,
  io::Write,
  path::Path,
  process::{Command, Stdio},
};

use super::ChunkMethod;
use path_abs::PathAbs;
use vapoursynth::prelude::*;

use anyhow::anyhow;

pub fn is_vapoursynth(s: &str) -> bool {
  [".vpy", ".py"].iter().any(|ext| s.ends_with(ext))
}

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

pub fn create_vs_file(
  temp: &str,
  source: &str,
  chunk_method: ChunkMethod,
) -> anyhow::Result<String> {
  // only for python code, remove if being called by rust
  let temp = Path::new(temp);
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

  load_script.write_all(
    // TODO should probably check if the syntax for rust strings and escaping utf and stuff like that is the same as in python
    format!(
      "from vapoursynth import core\n\
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
    .args(&["-i", "-"])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?
    .wait()?;

  Ok(load_script_path.to_string_lossy().to_string())
}
