use crate::{ffmpeg, ref_vec, Input};
use anyhow::Error;
use plotters::prelude::*;
use serde::Deserialize;
use std::{
  cmp::Ordering,
  ffi::OsStr,
  path::Path,
  process::{Command, Stdio},
  usize,
};

#[derive(Deserialize, Debug)]
struct VmafScore {
  vmaf: f64,
}

#[derive(Deserialize, Debug)]
struct Metrics {
  metrics: VmafScore,
}

#[derive(Deserialize, Debug)]
struct VmafResult {
  frames: Vec<Metrics>,
}

#[inline]
fn printable_base10_digits(x: usize) -> u32 {
  (((x as f64).log10() + 1.0).floor() as u32).max(1)
}

pub fn plot_vmaf_score_file(
  scores_file: &Path,
  plot_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
  let scores = read_vmaf_file(scores_file).unwrap();

  let plot_width = 1600 + (printable_base10_digits(scores.len()) as u32 * 200);
  let plot_heigth = 600;

  let length = scores.len() as u32;
  let perc_1 = read_weighted_vmaf(scores_file, 0.01).unwrap();
  let perc_25 = read_weighted_vmaf(scores_file, 0.25).unwrap();
  let perc_75 = read_weighted_vmaf(scores_file, 0.75).unwrap();
  let perc_mean = read_weighted_vmaf(scores_file, 0.50).unwrap();

  let root = SVGBackend::new(plot_path.as_os_str(), (plot_width, plot_heigth)).into_drawing_area();

  root.fill(&WHITE)?;

  let mut chart = ChartBuilder::on(&root)
    .set_label_area_size(LabelAreaPosition::Bottom, (5).percent())
    .set_label_area_size(LabelAreaPosition::Left, (5).percent())
    .set_label_area_size(LabelAreaPosition::Right, (7).percent())
    .set_label_area_size(LabelAreaPosition::Top, (5).percent())
    .margin((1).percent())
    .build_cartesian_2d(0_u32..length, perc_1.floor()..100.0)?;

  chart.configure_mesh().draw()?;

  // 1%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_1)), &RED))?
    .label(format!("1%: {}", perc_1))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

  // 25%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_25)), &YELLOW))?
    .label(format!("25%: {}", perc_25))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &YELLOW));

  // 75%
  chart
    .draw_series(LineSeries::new((0..=length).map(|x| (x, perc_75)), &GREEN))?
    .label(format!("75%: {}", perc_75))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREEN));

  // Mean
  chart
    .draw_series(LineSeries::new(
      (0..=length).map(|x| (x, perc_mean)),
      &BLACK,
    ))?
    .label(format!("Mean: {}", perc_mean))
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLACK));

  // Data
  chart.draw_series(LineSeries::new(
    (0..).zip(scores.iter()).map(|(x, y)| (x, *y)),
    &BLUE,
  ))?;

  chart
    .configure_series_labels()
    .background_style(&WHITE.mix(0.8))
    .border_style(&BLACK)
    .draw()?;

  root.present().expect("Unable to write result plot to file");

  Ok(())
}

pub fn validate_libvmaf() -> Result<(), Error> {
  let mut cmd = Command::new("ffmpeg");
  cmd.arg("-h");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;

  let stdr = String::from_utf8(out.stderr)?;
  if !stdr.contains("--enable-libvmaf") {
    panic!("FFmpeg is not compiled with --enable-libvmaf");
  }
  Ok(())
}

pub fn validate_vmaf_test_run(model: &str) -> Result<(), Error> {
  let mut cmd = Command::new("ffmpeg");

  cmd.args(["-hide_banner", "-filter_complex"]);
  cmd.arg(format!("testsrc=duration=1:size=1920x1080:rate=1[B];testsrc=duration=1:size=1920x1080:rate=1[A];[B][A]libvmaf{}", model).as_str());
  cmd.args(["-t", "1", "-f", "null", "-"]);

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;

  let stderr = String::from_utf8(out.stderr)?;

  assert!(out.status.success(), "Test VMAF run failed:\n{:?}", stderr);

  Ok(())
}

pub fn validate_vmaf(vmaf_model: &str) -> Result<(), Error> {
  validate_libvmaf()?;
  validate_vmaf_test_run(vmaf_model)?;

  Ok(())
}

pub fn plot(
  encoded: &Path,
  reference: &Input,
  model: Option<impl AsRef<Path>>,
  res: &str,
  sample_rate: usize,
  filter: Option<&str>,
  threads: usize,
) -> Result<(), Error> {
  let json_file = encoded.with_extension("json");
  let plot_file = encoded.with_extension("svg");

  println!(":: VMAF Run");

  let pipe_cmd: Vec<&OsStr> = match &reference {
    Input::Video(ref path) => ref_vec![
      OsStr,
      [
        "ffmpeg",
        "-i",
        path,
        "-strict",
        "-1",
        "-f",
        "yuv4mpegpipe",
        "-"
      ]
    ],
    Input::VapourSynth(ref path) => {
      ref_vec![OsStr, ["vspipe", "-y", path, "-"]]
    }
  };

  run_vmaf(
    encoded,
    &pipe_cmd,
    &json_file,
    model,
    res,
    sample_rate,
    filter,
    threads,
  )?;

  plot_vmaf_score_file(&json_file, &plot_file).unwrap();
  Ok(())
}

pub fn run_vmaf(
  encoded: &Path,
  reference_pipe_cmd: &[impl AsRef<OsStr>],
  stat_file: impl AsRef<Path>,
  model: Option<impl AsRef<Path>>,
  res: &str,
  sample_rate: usize,
  vmaf_filter: Option<&str>,
  threads: usize,
) -> Result<(), Error> {
  let mut filter = if sample_rate > 1 {
    format!(
      "select=not(mod(n\\,{})),setpts={:.4}*PTS,",
      sample_rate,
      1.0 / sample_rate as f64,
    )
  } else {
    String::new()
  };

  if let Some(vmaf_filter) = vmaf_filter {
    filter.reserve(1 + vmaf_filter.len());
    filter.push_str(vmaf_filter);
    filter.push(',');
  }

  let vmaf = if let Some(model) = model {
    format!(
      "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}:model_path={}:n_threads={}",
      ffmpeg::escape_path_in_filter(stat_file),
      ffmpeg::escape_path_in_filter(&model),
      threads
    )
  } else {
    format!(
      "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}:n_threads={}",
      ffmpeg::escape_path_in_filter(stat_file),
      threads
    )
  };

  let mut source_pipe = if let [cmd, args @ ..] = &*reference_pipe_cmd {
    let mut source_pipe = Command::new(cmd);
    source_pipe.args(args);
    source_pipe.stdout(Stdio::piped());
    source_pipe.stderr(Stdio::piped());
    source_pipe
  } else {
    unreachable!()
  };

  let handle = source_pipe
    .stderr(Stdio::piped())
    .spawn()
    .unwrap_or_else(|e| {
      panic!(
        "Failed to execute source pipe: {}\ncommand: {:#?}",
        e, source_pipe
      )
    });

  let mut cmd = Command::new("ffmpeg");
  cmd.args([
    "-loglevel",
    "error",
    "-hide_banner",
    "-y",
    "-thread_queue_size",
    "1024",
    "-hide_banner",
    "-r",
    "60",
    "-i",
  ]);
  cmd.arg(encoded);
  cmd.args(["-r", "60", "-i", "-", "-filter_complex"]);

  let distorted = format!("[0:v]scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];", &res);
  let reference = format!(
    "[1:v]{}scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];",
    filter, &res
  );

  cmd.arg(format!("{}{}{}", distorted, reference, vmaf));
  cmd.args(["-f", "null", "-"]);
  cmd.stderr(Stdio::piped());
  cmd.stdout(Stdio::piped());

  let output = cmd
    .stdin(handle.stdout.unwrap())
    .output()
    .unwrap_or_else(|e| panic!("Failed to execute vmaf pipe: {}\ncommand: {:#?}", e, cmd));

  assert!(
    output.status.success(),
    "VMAF calculation failed:\nCommand: {:?}\nOutput: {:#?}",
    cmd,
    output
  );

  Ok(())
}

pub fn read_vmaf_file(file: impl AsRef<Path>) -> Result<Vec<f64>, serde_json::Error> {
  let json_str = crate::util::read_file_to_string(file).unwrap();
  let bazs = serde_json::from_str::<VmafResult>(&json_str)?;
  let v = bazs
    .frames
    .into_iter()
    .map(|x| x.metrics.vmaf)
    .collect::<Vec<_>>();

  Ok(v)
}

pub fn read_weighted_vmaf(
  file: impl AsRef<Path>,
  percentile: f64,
) -> Result<f64, serde_json::Error> {
  let mut scores = read_vmaf_file(file).unwrap();

  Ok(get_percentile(&mut scores, percentile))
}

/// Calculates percentile from an array of values
pub fn get_percentile(scores: &mut [f64], percentile: f64) -> f64 {
  assert!(!scores.is_empty());
  scores.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

  let k = (scores.len() - 1) as f64 * percentile;
  let f = k.floor();
  let c = k.ceil();

  if f as u64 == c as u64 {
    return scores[k as usize];
  }

  let d0 = scores[f as usize] * (c - k);
  let d1 = scores[f as usize] * (k - f);

  d0 + d1
}
