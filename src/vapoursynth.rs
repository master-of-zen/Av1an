// This is a mostly drop-in reimplementation of vspipe.
// The main difference is what the errors look like.

use anyhow::Context;
use anyhow::*;
use std::path::Path;
use thiserror::Error;

use pyo3::prelude::*;

pub mod inner {
  #![allow(clippy::cast_lossless, clippy::mutex_atomic)]
  //   extern crate clap;
  extern crate num_rational;
  extern crate vapoursynth;

  use std::collections::HashMap;
  use std::ffi::OsStr;
  use std::fmt::Debug;
  use std::fs::File;
  use std::io::{self, stdout, Stdout, Write};
  use std::ops::Deref;
  use std::sync::{Arc, Condvar, Mutex};
  use std::time::Instant;
  use std::{
    cmp,
    path::{Path, PathBuf},
  };

  use self::num_rational::Ratio;
  use self::vapoursynth::prelude::*;
  use super::*;

  enum OutputTarget {
    File(File),
    Stdout(Stdout),
    Empty,
  }

  struct OutputParameters<'core> {
    node: Node<'core>,
    alpha_node: Option<Node<'core>>,
    start_frame: usize,
    end_frame: usize,
    requests: usize,
    y4m: bool,
    progress: bool,
  }

  struct OutputState<'core> {
    output_target: OutputTarget,
    timecodes_file: Option<File>,
    error: Option<(usize, anyhow::Error)>,
    reorder_map: HashMap<usize, (Option<FrameRef<'core>>, Option<FrameRef<'core>>)>,
    last_requested_frame: usize,
    next_output_frame: usize,
    current_timecode: Ratio<i64>,
    callbacks_fired: usize,
    callbacks_fired_alpha: usize,
    last_fps_report_time: Instant,
    last_fps_report_frames: usize,
    fps: Option<f64>,
  }

  struct SharedData<'core> {
    output_done_pair: (Mutex<bool>, Condvar),
    output_parameters: OutputParameters<'core>,
    output_state: Mutex<OutputState<'core>>,
  }

  impl Write for OutputTarget {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      match *self {
        OutputTarget::File(ref mut file) => file.write(buf),
        OutputTarget::Stdout(ref mut out) => out.write(buf),
        OutputTarget::Empty => Ok(buf.len()),
      }
    }

    fn flush(&mut self) -> io::Result<()> {
      match *self {
        OutputTarget::File(ref mut file) => file.flush(),
        OutputTarget::Stdout(ref mut out) => out.flush(),
        OutputTarget::Empty => Ok(()),
      }
    }
  }

  fn print_version() -> Result<(), Error> {
    // let environment = Environment::new().context("Couldn't create the VSScript environment")?;
    let environment = Environment::new().unwrap();
    let core = environment.get_core().unwrap();
    print!("{}", core.info().version_string);

    Ok(())
  }

  // Returns "Variable" or the value of the property passed through a function.
  fn map_or_variable<T, F>(x: &Property<T>, f: F) -> String
  where
    T: Debug + Clone + Copy + Eq + PartialEq,
    F: FnOnce(&T) -> String,
  {
    match *x {
      Property::Variable => "Variable".to_owned(),
      Property::Constant(ref x) => f(x),
    }
  }

  fn print_info(writer: &mut dyn Write, node: &Node, alpha: Option<&Node>) -> Result<(), Error> {
    let info = node.info();

    writeln!(
      writer,
      "Width: {}",
      map_or_variable(&info.resolution, |x| format!("{}", x.width))
    )?;
    writeln!(
      writer,
      "Height: {}",
      map_or_variable(&info.resolution, |x| format!("{}", x.height))
    )?;

    writeln!(writer, "Frames: {}", info.num_frames)?;

    writeln!(
      writer,
      "FPS: {}",
      map_or_variable(&info.framerate, |x| format!(
        "{}/{} ({:.3} fps)",
        x.numerator,
        x.denominator,
        x.numerator as f64 / x.denominator as f64
      ))
    )?;

    match info.format {
      Property::Variable => writeln!(writer, "Format Name: Variable")?,
      Property::Constant(f) => {
        writeln!(writer, "Format Name: {}", f.name())?;
        writeln!(writer, "Color Family: {}", f.color_family())?;
        writeln!(
          writer,
          "Alpha: {}",
          if alpha.is_some() { "Yes" } else { "No" }
        )?;
        writeln!(writer, "Sample Type: {}", f.sample_type())?;
        writeln!(writer, "Bits: {}", f.bits_per_sample())?;
        writeln!(writer, "SubSampling W: {}", f.sub_sampling_w())?;
        writeln!(writer, "SubSampling H: {}", f.sub_sampling_h())?;
      }
    }

    Ok(())
  }

  fn print_y4m_header<W: Write>(writer: &mut W, node: &Node) -> Result<(), Error> {
    let info = node.info();

    if let Property::Constant(format) = info.format {
      write!(writer, "YUV4MPEG2 C")?;

      match format.color_family() {
        ColorFamily::Gray => {
          write!(writer, "mono")?;
          if format.bits_per_sample() > 8 {
            write!(writer, "{}", format.bits_per_sample())?;
          }
        }
        ColorFamily::YUV => {
          write!(
            writer,
            "{}",
            match (format.sub_sampling_w(), format.sub_sampling_h()) {
              (1, 1) => "420",
              (1, 0) => "422",
              (0, 0) => "444",
              (2, 2) => "410",
              (2, 0) => "411",
              (0, 1) => "440",
              _ => bail!("No y4m identifier exists for the current format"),
            }
          )?;

          if format.bits_per_sample() > 8 && format.sample_type() == SampleType::Integer {
            write!(writer, "p{}", format.bits_per_sample())?;
          } else if format.sample_type() == SampleType::Float {
            write!(
              writer,
              "p{}",
              match format.bits_per_sample() {
                16 => "h",
                32 => "s",
                64 => "d",
                _ => unreachable!(),
              }
            )?;
          }
        }
        _ => bail!("No y4m identifier exists for the current format"),
      }

      if let Property::Constant(resolution) = info.resolution {
        write!(writer, " W{} H{}", resolution.width, resolution.height)?;
      } else {
        unreachable!();
      }

      if let Property::Constant(framerate) = info.framerate {
        write!(
          writer,
          " F{}:{}",
          framerate.numerator, framerate.denominator
        )?;
      } else {
        unreachable!();
      }

      let num_frames = info.num_frames;

      writeln!(writer, " Ip A0:0 XLENGTH={}", num_frames)?;

      Ok(())
    } else {
      unreachable!();
    }
  }

  // Checks if the frame is completed, that is, we have the frame and, if needed, its alpha part.
  fn is_completed(entry: &(Option<FrameRef>, Option<FrameRef>), have_alpha: bool) -> bool {
    entry.0.is_some() && (!have_alpha || entry.1.is_some())
  }

  fn print_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<(), Error> {
    const RGB_REMAP: [usize; 3] = [1, 2, 0];

    let format = frame.format();
    #[allow(clippy::needless_range_loop)]
    for plane in 0..format.plane_count() {
      let plane = if format.color_family() == ColorFamily::RGB {
        RGB_REMAP[plane]
      } else {
        plane
      };

      if let Ok(data) = frame.data(plane) {
        writer.write_all(data)?;
      } else {
        for row in 0..frame.height(plane) {
          writer.write_all(frame.data_row(plane, row))?;
        }
      }
    }

    Ok(())
  }

  fn print_frames<W: Write>(
    writer: &mut W,
    parameters: &OutputParameters,
    frame: &Frame,
    alpha_frame: Option<&Frame>,
  ) -> Result<(), Error> {
    if parameters.y4m {
      writeln!(writer, "FRAME").context("Couldn't output the frame header")?;
    }

    print_frame(writer, frame).context("Couldn't output the frame")?;
    if let Some(alpha_frame) = alpha_frame {
      print_frame(writer, alpha_frame).context("Couldn't output the alpha frame")?;
    }

    Ok(())
  }

  fn update_timecodes(frame: &Frame, state: &mut OutputState) -> Result<(), Error> {
    let props = frame.props();
    // TODO fix error handling
    let duration_num = props.get_int("_DurationNum").unwrap();
    let duration_den = props.get_int("_DurationDen").unwrap();

    if duration_den == 0 {
      bail!("The duration denominator is zero");
    }

    state.current_timecode += Ratio::new(duration_num, duration_den);

    Ok(())
  }

  fn frame_done_callback<'core>(
    frame: Result<FrameRef<'core>, GetFrameError>,
    n: usize,
    _node: &Node<'core>,
    shared_data: &Arc<SharedData<'core>>,
    alpha: bool,
  ) {
    let parameters = &shared_data.output_parameters;
    let mut state = shared_data.output_state.lock().unwrap();

    // Increase the progress counter.
    if !alpha {
      state.callbacks_fired += 1;
      if parameters.alpha_node.is_none() {
        state.callbacks_fired_alpha += 1;
      }
    } else {
      state.callbacks_fired_alpha += 1;
    }

    // Figure out the FPS.
    if parameters.progress {
      let current = Instant::now();
      let elapsed = current.duration_since(state.last_fps_report_time);
      let elapsed_seconds = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9;

      if elapsed.as_secs() > 10 {
        state.fps =
          Some((state.callbacks_fired - state.last_fps_report_frames) as f64 / elapsed_seconds);
        state.last_fps_report_time = current;
        state.last_fps_report_frames = state.callbacks_fired;
      }
    }

    match frame {
      Err(error) => {
        if state.error.is_none() {
          state.error = Some((
            n,
            anyhow::Error::msg(error.into_inner().to_string_lossy().into_owned()),
          ))
        }
      }
      Ok(frame) => {
        // Store the frame in the reorder map.
        {
          let entry = state.reorder_map.entry(n).or_insert((None, None));
          if alpha {
            entry.1 = Some(frame);
          } else {
            entry.0 = Some(frame);
          }
        }

        // If we got both a frame and its alpha frame, request one more.
        if is_completed(&state.reorder_map[&n], parameters.alpha_node.is_some())
          && state.last_requested_frame < parameters.end_frame
          && state.error.is_none()
        {
          let shared_data_2 = shared_data.clone();
          parameters
            .node
            .get_frame_async(state.last_requested_frame + 1, move |frame, n, node| {
              frame_done_callback(frame, n, &node, &shared_data_2, false)
            });

          if let Some(ref alpha_node) = parameters.alpha_node {
            let shared_data_2 = shared_data.clone();
            alpha_node.get_frame_async(state.last_requested_frame + 1, move |frame, n, node| {
              frame_done_callback(frame, n, &node, &shared_data_2, true)
            });
          }

          state.last_requested_frame += 1;
        }

        // Output all completed frames.
        while state
          .reorder_map
          .get(&state.next_output_frame)
          .map(|entry| is_completed(entry, parameters.alpha_node.is_some()))
          .unwrap_or(false)
        {
          let next_output_frame = state.next_output_frame;
          let (frame, alpha_frame) = state.reorder_map.remove(&next_output_frame).unwrap();

          let frame = frame.unwrap();
          if state.error.is_none() {
            if let Err(error) = print_frames(
              &mut state.output_target,
              parameters,
              &frame,
              alpha_frame.as_deref(),
            ) {
              state.error = Some((n, error));
            }
          }

          if state.timecodes_file.is_some() && state.error.is_none() {
            let timecode = (*state.current_timecode.numer() as f64 * 1000f64)
              / *state.current_timecode.denom() as f64;
            match writeln!(state.timecodes_file.as_mut().unwrap(), "{:.6}", timecode)
              .context("Couldn't output the timecode")
            {
              Err(error) => state.error = Some((n, error)),
              Ok(()) => {
                if let Err(error) =
                  update_timecodes(&frame, &mut state).context("Couldn't update the timecodes")
                {
                  state.error = Some((n, error));
                }
              }
            }
          }

          state.next_output_frame += 1;
        }
      }
    }

    // Output the progress info.
    if parameters.progress {
      eprint!(
        "Frame: {}/{}",
        state.callbacks_fired,
        parameters.end_frame - parameters.start_frame + 1
      );

      if let Some(fps) = state.fps {
        eprint!(" ({:.2} fps)", fps);
      }

      eprint!("\r");
    }

    // if state.next_output_frame == parameters.end_frame + 1 {
    // This condition works with error handling:
    let frames_requested = state.last_requested_frame - parameters.start_frame + 1;
    if state.callbacks_fired == frames_requested && state.callbacks_fired_alpha == frames_requested
    {
      *shared_data.output_done_pair.0.lock().unwrap() = true;
      shared_data.output_done_pair.1.notify_one();
    }
  }

  fn output(
    mut output_target: OutputTarget,
    mut timecodes_file: Option<File>,
    parameters: OutputParameters,
  ) -> Result<(), Error> {
    // Print the y4m header.
    if parameters.y4m {
      if parameters.alpha_node.is_some() {
        bail!("Can't apply y4m headers to a clip with alpha");
      }

      print_y4m_header(&mut output_target, &parameters.node)
        .context("Couldn't write the y4m header")?;
    }

    // Print the timecodes header.
    if let Some(ref mut timecodes_file) = timecodes_file {
      writeln!(timecodes_file, "# timecode format v2")?;
    }

    let initial_requests = cmp::min(
      parameters.requests,
      parameters.end_frame - parameters.start_frame + 1,
    );

    let output_done_pair = (Mutex::new(false), Condvar::new());
    let output_state = Mutex::new(OutputState {
      output_target,
      timecodes_file,
      error: None,
      reorder_map: HashMap::new(),
      last_requested_frame: parameters.start_frame + initial_requests - 1,
      next_output_frame: 0,
      current_timecode: Ratio::from_integer(0),
      callbacks_fired: 0,
      callbacks_fired_alpha: 0,
      last_fps_report_time: Instant::now(),
      last_fps_report_frames: 0,
      fps: None,
    });
    let shared_data = Arc::new(SharedData {
      output_done_pair,
      output_parameters: parameters,
      output_state,
    });

    // Record the start time.
    let start_time = Instant::now();

    // Start off by requesting some frames.
    {
      let parameters = &shared_data.output_parameters;
      for n in 0..initial_requests {
        let shared_data_2 = shared_data.clone();
        parameters.node.get_frame_async(n, move |frame, n, node| {
          frame_done_callback(frame, n, &node, &shared_data_2, false)
        });

        if let Some(ref alpha_node) = parameters.alpha_node {
          let shared_data_2 = shared_data.clone();
          alpha_node.get_frame_async(n, move |frame, n, node| {
            frame_done_callback(frame, n, &node, &shared_data_2, true)
          });
        }
      }
    }

    let &(ref lock, ref cvar) = &shared_data.output_done_pair;
    let mut done = lock.lock().unwrap();
    while !*done {
      done = cvar.wait(done).unwrap();
    }

    let elapsed = start_time.elapsed();
    let elapsed_seconds = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9;

    let mut state = shared_data.output_state.lock().unwrap();
    eprintln!(
      "Output {} frames in {:.2} seconds ({:.2} fps)",
      state.next_output_frame,
      elapsed_seconds,
      state.next_output_frame as f64 / elapsed_seconds
    );

    if let Some((n, ref msg)) = state.error {
      bail!("Failed to retrieve frame {} with error: {}", n, msg);
    }

    // Flush the output file.
    state
      .output_target
      .flush()
      .context("Failed to flush the output file")?;

    Ok(())
  }

  pub fn num_frames(path: &Path) -> Result<usize, Error> {
    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    // Start time more similar to vspipe's.
    let start_time = Instant::now();

    // Evaluate the script.
    environment.eval_file(path, EvalFlags::Nothing).unwrap();

    // Get the output node.
    let output_index = 0;

    let (node, alpha_node) = environment.get_output(output_index).unwrap();

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

      info.num_frames
    };

    Ok(num_frames)
  }

  pub fn run(path: &Path) -> Result<(), Error> {
    // Open the output files.
    let mut output_target = OutputTarget::File(File::create("test_output")?);

    let timecodes_file = None;

    // Create a new VSScript environment.
    let mut environment = Environment::new().unwrap();

    // Start time more similar to vspipe's.
    let start_time = Instant::now();

    // Evaluate the script.
    environment.eval_file(path, EvalFlags::Nothing).unwrap();

    // Get the output node.
    let output_index = 0;

    let (node, alpha_node) = environment.get_output(output_index).unwrap();

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

      info.num_frames
    };

    let start_frame = 0;
    let end_frame = num_frames - 1;

    let requests = environment.get_core().unwrap().info().num_threads;

    let y4m = true;
    let progress = true;

    output(
      output_target,
      timecodes_file,
      OutputParameters {
        node,
        alpha_node,
        start_frame: start_frame as usize,
        end_frame: end_frame as usize,
        requests,
        y4m,
        progress,
      },
    )
    .context("Couldn't output the frames")?;

    // This is still not a very valid comparison since vspipe does all argument validation
    // before it starts the time.
    let elapsed = start_time.elapsed();
    let elapsed_seconds = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9;
    eprintln!("vspipe time: {:.2} seconds", elapsed_seconds);

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  #[test]
  fn num_frames() {
    assert_eq!(
      crate::vapoursynth::inner::num_frames(Path::new("input.vpy")).unwrap(),
      4
    );
  }
}

#[pyfunction]
pub fn vspipe_get_num_frames(path: &str) -> PyResult<usize> {
  println!("it's working");
  // println!("{:?}", inner::num_frames(Path::new(path)));
  // inner::num_frames(Path::new(path))
  //   .map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("{}", e)))
  Ok(0)
}

#[pyfunction]
pub fn vspipe(path: &str) -> PyResult<()> {
  inner::run(Path::new(path))
    .map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("{}", e)))?;
  Ok(())
}
