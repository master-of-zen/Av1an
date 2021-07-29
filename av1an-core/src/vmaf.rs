use crate::ffmpeg;
use crate::{read_vmaf_file, read_weighted_vmaf};
use anyhow::Error;
use plotters::prelude::*;

use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::usize;

#[inline]
fn printable_base10_digits(x: usize) -> u32 {
  (((x as f64).log10() + 1.0).floor() as u32).max(1)
}

pub fn plot_vmaf_score_file(
  scores_file: &Path,
  plot_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
  let scores = read_vmaf_file(scores_file).unwrap();

  let plot_width = 2560 + (printable_base10_digits(scores.len()) as u32 * 200);
  let plot_heigth = 1440;

  let length = scores.len() as u32;
  let perc_1 = read_weighted_vmaf(scores_file, 0.01).unwrap();
  let perc_25 = read_weighted_vmaf(scores_file, 0.25).unwrap();
  let perc_75 = read_weighted_vmaf(scores_file, 0.75).unwrap();
  let perc_mean = read_weighted_vmaf(scores_file, 0.50).unwrap();

  let root =
    BitMapBackend::new(plot_path.as_os_str(), (plot_width, plot_heigth)).into_drawing_area();

  root.fill(&WHITE)?;

  let mut chart = ChartBuilder::on(&root)
    .set_label_area_size(LabelAreaPosition::Bottom, (8).percent())
    .set_label_area_size(LabelAreaPosition::Left, (5).percent())
    .set_label_area_size(LabelAreaPosition::Left, (5).percent())
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
  let lib_check = "--enable-libvmaf";
  let mut cmd = Command::new("ffmpeg");
  cmd.args(["-h"]);

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;

  let stdr = String::from_utf8(out.stderr)?;
  if !stdr.contains(lib_check) {
    panic!("FFmpeg doesn't have --enable-libvmaf");
  }
  Ok(())
}

pub fn validate_vmaf_test_run(model: &str) -> Result<(), Error> {
  let mut cmd = Command::new("ffmpeg");

  cmd.args(["-hide_banner", "-filter_complex"]);
  cmd.args([format!("testsrc=duration=1:size=1920x1080:rate=1[B];testsrc=duration=1:size=1920x1080:rate=1[A];[B][A]libvmaf{}", model).as_str()]);
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

pub fn run_vmaf_on_files(
  source: &Path,
  output: &Path,
  model: Option<&Path>,
) -> Result<PathBuf, Error> {
  let mut cmd = Command::new("ffmpeg");

  cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
  cmd.args(["-r", "60", "-i", output.as_os_str().to_str().unwrap()]);
  cmd.args(["-r", "60", "-i", source.as_os_str().to_str().unwrap()]);
  cmd.args(["-filter_complex"]);

  let res = "1920x1080";
  let vmaf_filter = "";
  let file_path = output.with_extension("json");
  let threads = "";

  cmd.arg(
    format!(
      "[0:v]scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];[1:v]{}scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}{}{}",
      res, 
      vmaf_filter,
      res,
      file_path.as_os_str().to_str().unwrap(),
      if let Some(model) = model {
        format!(":model_path={}", ffmpeg::escape_path_in_filter(model))
      } else {
        "".into()
      },
      threads
    )
  );

  cmd.args(["-f", "null", "-"]);

  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::piped());

  let out = cmd.output()?;

  let stdr = String::from_utf8(out.stderr)?;

  assert!(
    out.status.success(),
    "VMAF calculation failed:\n:{:#?}",
    stdr
  );

  Ok(file_path)
}

pub fn plot_vmaf(
  source: impl AsRef<Path>,
  output: impl AsRef<Path>,
  model: Option<impl AsRef<Path>>,
) -> Result<(), Error> {
  let source = source.as_ref();
  let output = output.as_ref();
  let model = model.as_ref().map(|path| path.as_ref());

  println!(":: VMAF Run");

  let json_file = run_vmaf_on_files(source, output, model)?;
  let plot_path = output.with_extension("png");
  plot_vmaf_score_file(&json_file, &plot_path).unwrap();
  Ok(())
}

pub fn run_vmaf_on_chunk(
  encoded: impl AsRef<Path>,
  pipe_cmd: &[String],
  stat_file: impl AsRef<Path>,
  model: Option<impl AsRef<Path>>,
  res: &str,
  sample_rate: usize,
  vmaf_filter: &str,
  threads: usize,
) -> Result<(), Error> {
  // Select filter for sampling from the source
  let select = if sample_rate > 1 {
    format!(
      "select=not(mod(n\\,{})),setpts={:.4}*PTS,",
      sample_rate,
      1.0 / sample_rate as f64
    )
  } else {
    format!("")
  };

  let distorted = format!("[0:v]scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];", &res);
  let reference = format!("[1:v]{}{}scale={}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];", select, vmaf_filter, &res);

  let vmaf = if let Some(model) = model {
    format!(
      "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}:model_path={}:n_threads={}",
      ffmpeg::escape_path_in_filter(stat_file),
      &model.as_ref().to_str().unwrap(),
      threads
    )
  } else {
    format!(
      "[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={}:n_threads={}",
      ffmpeg::escape_path_in_filter(stat_file),
      threads
    )
  };

  let vmaf_cmd = [
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
    encoded.as_ref().to_str().unwrap(),
    "-r",
    "60",
    "-i",
    "-",
    "-filter_complex",
  ];

  let cmd_out = ["-f", "null", "-"];

  let mut source_pipe = Command::new(pipe_cmd.get(0).unwrap());
  source_pipe.args(&pipe_cmd[1..]);
  source_pipe.stdout(Stdio::piped());
  source_pipe.stderr(Stdio::piped());

  let handle = source_pipe
    .stderr(Stdio::piped())
    .spawn()
    .unwrap_or_else(|e| {
      panic!(
        "Failed to execute source pipe: {} \ncommand: {:#?}",
        e, source_pipe
      )
    });

  // Making final ffmpeg command
  let mut cmd = Command::new("ffmpeg");
  cmd.args(vmaf_cmd);
  cmd.arg(format!("{}{}{}", distorted, reference, vmaf));
  cmd.args(cmd_out);
  cmd.stderr(Stdio::piped());
  cmd.stdout(Stdio::piped());
  let output = cmd
    .stdin(handle.stdout.unwrap())
    .output()
    .unwrap_or_else(|e| panic!("Failed to execute vmaf pipe: {}\ncommand: {:#?}", e, cmd));

  assert!(
    output.status.success(),
    "VMAF calculation failed:\nCommand: {:?}\nOutput: {:?}",
    cmd,
    output
  );

  Ok(())
}
