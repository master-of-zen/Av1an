use crate::broker::EncoderCrash;
use crate::util::{printable_base10_digits, read_bytes};
use crate::{ffmpeg, ref_smallvec, Input};
use anyhow::anyhow;
use plotters::prelude::*;
use serde::Deserialize;
use smallvec::SmallVec;
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

pub fn validate_libvmaf() -> anyhow::Result<()> {
  let mut cmd = Command::new("ffmpeg");
  cmd.arg("-h");

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;

  let stdr = String::from_utf8(out.stderr)?;
  if !stdr.contains("--enable-libvmaf") {
    return Err(anyhow!("FFmpeg is not compiled with --enable-libvmaf, but target quality or VMAF plotting was enabled"));
  }
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
) -> Result<(), EncoderCrash> {
  let json_file = encoded.with_extension("json");
  let plot_file = encoded.with_extension("svg");

  println!(":: VMAF Run");

  let pipe_cmd: SmallVec<[&OsStr; 8]> = match reference {
    Input::Video(ref path) => ref_smallvec!(
      OsStr,
      8,
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
    ),
    Input::VapourSynth(ref path) => ref_smallvec!(OsStr, 8, ["vspipe", "-y", path, "-"]),
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
) -> Result<(), EncoderCrash> {
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
    source_pipe.stderr(Stdio::null());
    source_pipe.spawn().unwrap()
  } else {
    unreachable!()
  };

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
  cmd.stdin(source_pipe.stdout.take().unwrap());
  cmd.stderr(Stdio::piped());
  cmd.stdout(Stdio::null());

  let output = cmd.spawn().unwrap().wait_with_output().unwrap();

  if !output.status.success() {
    return Err(EncoderCrash {
      exit_status: output.status,
      pipe_stderr: String::new().into(),
      stderr: output.stderr.into(),
      stdout: String::new().into(),
    });
  }

  Ok(())
}

pub fn read_vmaf_file(file: impl AsRef<Path>) -> Result<Vec<f64>, bincode::Error> {
  let buffer = read_bytes(file)?;
  let bazs = bincode::deserialize::<VmafResult>(&buffer)?;
  let v = bazs
    .frames
    .into_iter()
    .map(|x| x.metrics.vmaf)
    .collect::<Vec<_>>();

  Ok(v)
}

pub fn read_weighted_vmaf(file: impl AsRef<Path>, percentile: f64) -> Result<f64, bincode::Error> {
  let mut scores = read_vmaf_file(file).unwrap();

  Ok(get_percentile(&mut scores, percentile))
}

/// Calculates percentile from an array of values
pub fn get_percentile(scores: &mut [f64], percentile: f64) -> f64 {
  assert!(!scores.is_empty());

  let k = ((scores.len() - 1) as f64 * percentile) as usize;

  let (_, kth_element, _) =
    scores.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

  *kth_element
}
