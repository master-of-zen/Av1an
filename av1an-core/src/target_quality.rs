extern crate num_cpus;

use std::borrow::Cow;
use std::cmp;
use std::usize;

use crate::Encoder;

macro_rules! into_vec {
  ($($x:expr),* $(,)?) => {
    vec![
      $(
        $x.into(),
      )*
    ]
  };
}

pub fn construct_target_quality_command(
  encoder: Encoder,
  threads: usize,
  q: &str,
) -> Vec<Cow<str>> {
  match encoder {
    Encoder::aom => into_vec![
      "aomenc",
      "--passes=1",
      format!("--threads={}", threads),
      "--tile-columns=2",
      "--tile-rows=1",
      "--end-usage=q",
      "-b",
      "8",
      "--cpu-used=6",
      format!("--cq-level={}", q),
      "--enable-filter-intra=0",
      "--enable-smooth-intra=0",
      "--enable-paeth-intra=0",
      "--enable-cfl-intra=0",
      "--enable-obmc=0",
      "--enable-palette=0",
      "--enable-overlay=0",
      "--enable-intrabc=0",
      "--enable-angle-delta=0",
      "--reduced-tx-type-set=1",
      "--enable-dual-filter=0",
      "--enable-intra-edge-filter=0",
      "--enable-order-hint=0",
      "--enable-flip-idtx=0",
      "--enable-dist-wtd-comp=0",
      "--enable-interintra-wedge=0",
      "--enable-onesided-comp=0",
      "--enable-interintra-comp=0",
      "--enable-global-motion=0",
      "--enable-cdef=0",
      "--max-reference-frames=3",
      "--cdf-update-mode=2",
      "--deltaq-mode=0",
      "--sb-size=64",
      "--min-partition-size=32",
      "--max-partition-size=32",
    ],
    Encoder::rav1e => into_vec![
      "rav1e",
      "-y",
      "-s",
      "10",
      "--threads",
      threads.to_string(),
      "--tiles",
      "16",
      "--quantizer",
      q,
      "--low-latency",
      "--rdo-lookahead-frames",
      "5",
      "--no-scene-detection",
    ],
    Encoder::libvpx => into_vec![
      "vpxenc",
      "-b",
      "10",
      "--profile=2",
      "--passes=1",
      "--pass=1",
      "--codec=vp9",
      format!("--threads={}", threads),
      "--cpu-used=9",
      "--end-usage=q",
      format!("--cq-level={}", q),
      "--row-mt=1",
    ],
    Encoder::svt_av1 => into_vec![
      "SvtAv1EncApp",
      "-i",
      "stdin",
      "--lp",
      threads.to_string(),
      "--preset",
      "8",
      "--keyint",
      "240",
      "--crf",
      q,
      "--tile-rows",
      "1",
      "--tile-columns",
      "2",
      "--pred-struct",
      "0",
      "--sg-filter-mode",
      "0",
      "--enable-restoration-filtering",
      "0",
      "--cdef-level",
      "0",
      "--disable-dlf",
      "0",
      "--mrp-level",
      "0",
      "--enable-mfmv",
      "0",
      "--enable-local-warp",
      "0",
      "--enable-global-motion",
      "0",
      "--enable-interintra-comp",
      "0",
      "--obmc-level",
      "0",
      "--rdoq-level",
      "0",
      "--filter-intra-level",
      "0",
      "--enable-intra-edge-filter",
      "0",
      "--enable-pic-based-rate-est",
      "0",
      "--pred-me",
      "0",
      "--bipred-3x3",
      "0",
      "--compound",
      "0",
      "--ext-block",
      "0",
      "--hbd-md",
      "0",
      "--palette-level",
      "0",
      "--umv",
      "0",
      "--tf-level",
      "3",
    ],
    Encoder::x264 => into_vec![
      "x264",
      "--log-level",
      "error",
      "--demuxer",
      "y4m",
      "-",
      "--no-progress",
      "--threads",
      threads.to_string(),
      "--preset",
      "medium",
      "--crf",
      q,
    ],
    Encoder::x265 => into_vec![
      "x265",
      "--log-level",
      "0",
      "--no-progress",
      "--y4m",
      "--frame-threads",
      cmp::min(threads, 16).to_string(),
      "--preset",
      "fast",
      "--crf",
      q,
    ],
  }
}

pub fn construct_target_quality_slow_command(encoder: Encoder, q: &str) -> Vec<Cow<str>> {
  match encoder {
    Encoder::aom => into_vec!["aomenc", "--passes=1", format!("--cq-level={}", q),],
    Encoder::rav1e => into_vec!["rav1e", "-y", "--quantizer", q],
    Encoder::libvpx => into_vec![
      "vpxenc",
      "--passes=1",
      "--pass=1",
      format!("--cq-level={}", q),
    ],
    Encoder::svt_av1 => into_vec!["SvtAv1EncApp", "-i", "stdin", "--crf", q,],
    Encoder::x264 => into_vec![
      "x264",
      "--log-level",
      "error",
      "--demuxer",
      "y4m",
      "-",
      "--no-progress",
      "--crf",
      q,
    ],
    Encoder::x265 => into_vec![
      "x265",
      "--log-level",
      "0",
      "--no-progress",
      "--y4m",
      "--crf",
      q,
    ],
  }
}

pub fn weighted_search(num1: f64, vmaf1: f64, num2: f64, vmaf2: f64, target: f64) -> usize {
  let dif1 = (transform_vmaf(target as f64) - transform_vmaf(vmaf2)).abs();
  let dif2 = (transform_vmaf(target as f64) - transform_vmaf(vmaf1)).abs();

  let tot = dif1 + dif2;

  let new_point = (num1 * (dif1 / tot) + (num2 * (dif2 / tot))).round() as usize;
  new_point
}

pub fn transform_vmaf(vmaf: f64) -> f64 {
  let x: f64 = 1.0 - vmaf / 100.0;
  if vmaf < 99.99 {
    -x.ln()
  } else {
    return 9.2;
  }
}

pub fn vmaf_auto_threads(workers: usize) -> usize {
  const OVER_PROVISION_FACTOR: f64 = 1.25;

  // Logical CPUs
  let threads = num_cpus::get();

  std::cmp::max(
    ((threads / workers) as f64 * OVER_PROVISION_FACTOR) as usize,
    1,
  )
}
