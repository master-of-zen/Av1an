use crate::into_vec;
use crate::list_index_of_regex;
use crate::regex;

use serde::{Deserialize, Serialize};

use itertools::chain;

use std::borrow::Cow;
use std::cmp;
use std::fmt::Display;
use std::path::PathBuf;

use regex::Regex;

const NULL: &'static str = if cfg!(target_os = "windows") {
  "nul"
} else {
  "/dev/null"
};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum Encoder {
  aom,
  rav1e,
  vpx,
  #[strum(serialize = "svt-av1")]
  svt_av1,
  x264,
  x265,
}

impl Display for Encoder {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(<&'static str>::from(self))
  }
}

impl Encoder {
  /// Composes 1st pass command for 1 pass encoding
  pub fn compose_1_1_pass(self, params: Vec<String>, output: String) -> Vec<String> {
    match &self {
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=1"],
        params,
        into_vec!["-o", output, "-"],
      )
      .collect(),
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y"],
        params,
        into_vec!["--output", output]
      )
      .collect(),
      Self::vpx => chain!(
        into_vec!["vpxenc", "--passes=1"],
        params,
        into_vec!["-o", output, "-"]
      )
      .collect(),
      Self::svt_av1 => chain!(
        into_vec!["SvtAv1EncApp", "-i", "stdin", "--progress", "2"],
        params,
        into_vec!["-b", output],
      )
      .collect(),
      Self::x264 => chain!(
        into_vec![
          "x264",
          "--stitchable",
          "--log-level",
          "error",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["-", "-o", output]
      )
      .collect(),
      Self::x265 => chain!(
        into_vec!["x265", "--y4m"],
        params,
        into_vec!["-", "-o", output]
      )
      .collect(),
    }
  }

  /// Composes 1st pass command for 2 pass encoding
  pub fn compose_1_2_pass(self, params: Vec<String>, fpf: &str) -> Vec<String> {
    match &self {
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=2", "--pass=1"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", NULL, "-"],
      )
      .collect(),
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y", "-q"],
        params,
        into_vec!["--first-pass", format!("{}.stat", fpf), "--output", NULL]
      )
      .collect(),
      Self::vpx => chain!(
        into_vec!["vpxenc", "--passes=2", "--pass=1"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", NULL, "-"],
      )
      .collect(),
      Self::svt_av1 => chain!(
        into_vec![
          "SvtAv1EncApp",
          "-i",
          "stdin",
          "--progress",
          "2",
          "--irefresh-type",
          "2",
        ],
        params,
        into_vec![
          "--pass",
          "1",
          "--stats",
          format!("{}.stat", fpf),
          "-b",
          NULL,
        ],
      )
      .collect(),
      Self::x264 => chain!(
        into_vec![
          "x264",
          "--stitchable",
          "--log-level",
          "error",
          "--pass",
          "1",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", NULL]
      )
      .collect(),
      Self::x265 => chain!(
        into_vec![
          "x265",
          "--stitchable",
          "--log-level",
          "error",
          "--pass",
          "1",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", NULL]
      )
      .collect(),
    }
  }

  /// Composes 2st pass command for 2 pass encoding
  pub fn compose_2_2_pass(self, params: Vec<String>, fpf: &str, output: String) -> Vec<String> {
    match &self {
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=2", "--pass=2"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", output, "-"],
      )
      .collect(),
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y", "-q"],
        params,
        into_vec!["--second-pass", format!("{}.stat", fpf), "--output", output]
      )
      .collect(),
      Self::vpx => chain!(
        into_vec!["vpxenc", "--passes=2", "--pass=2"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", output, "-"],
      )
      .collect(),
      Self::svt_av1 => chain!(
        into_vec![
          "SvtAv1EncApp",
          "-i",
          "stdin",
          "--progress",
          "2",
          "--irefresh-type",
          "2",
        ],
        params,
        into_vec![
          "--pass",
          "2",
          "--stats",
          format!("{}.stat", fpf),
          "-b",
          output,
        ],
      )
      .collect(),
      Self::x264 => chain!(
        into_vec![
          "x264",
          "--stitchable",
          "--log-level",
          "error",
          "--pass",
          "2",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", output]
      )
      .collect(),
      Self::x265 => chain!(
        into_vec![
          "x265",
          "--stitchable",
          "--log-level",
          "error",
          "--pass",
          "2",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", output]
      )
      .collect(),
    }
  }

  /// Returns default settings for the encoder
  pub fn get_default_arguments(self) -> Vec<String> {
    match &self {
      Encoder::aom => into_vec![
        "--threads=8",
        "-b",
        "10",
        "--cpu-used=6",
        "--end-usage=q",
        "--cq-level=30",
        "--tile-columns=2",
        "--tile-rows=1",
      ],
      Encoder::rav1e => into_vec![
        "--tiles",
        "8",
        "--speed",
        "6",
        "--quantizer",
        "100",
        "--no-scene-detection",
      ],
      Encoder::vpx => into_vec![
        "--codec=vp9",
        "-b",
        "10",
        "--profile=2",
        "--threads=4",
        "--cpu-used=0",
        "--end-usage=q",
        "--cq-level=30",
        "--row-mt=1",
      ],
      Encoder::svt_av1 => into_vec!["--preset", "4", "--keyint", "240", "--rc", "0", "--crf", "25"],
      Encoder::x264 => into_vec!["--preset", "slow", "--crf", "25"],
      Encoder::x265 => into_vec!["-p", "slow", "--crf", "25", "-D", "10"],
    }
  }

  /// Return number of default passes for encoder
  pub const fn get_default_pass(self) -> u8 {
    match &self {
      Self::aom | Self::vpx => 2,
      _ => 1,
    }
  }

  /// Default quantizer range target quality mode
  pub const fn get_default_cq_range(self) -> (usize, usize) {
    match &self {
      Self::aom | Self::vpx => (15, 55),
      Self::rav1e => (50, 140),
      Self::svt_av1 => (15, 50),
      Self::x264 | Self::x265 => (15, 35),
    }
  }

  /// Returns help command for encoder
  pub const fn help_command(self) -> [&'static str; 2] {
    match &self {
      Self::aom => ["aomenc", "--help"],
      Self::rav1e => ["rav1e", "--fullhelp"],
      Self::vpx => ["vpxenc", "--help"],
      Self::svt_av1 => ["SvtAv1EncApp", "--help"],
      Self::x264 => ["x264", "--fullhelp"],
      Self::x265 => ["x265", "--fullhelp"],
    }
  }

  /// Get the name of the executable/binary for the encoder
  pub const fn bin(self) -> &'static str {
    match self {
      Self::aom => "aomenc",
      Self::rav1e => "rav1e",
      Self::vpx => "vpxenc",
      Self::svt_av1 => "SvtAv1EncApp",
      Self::x264 => "x264",
      Self::x265 => "x265",
    }
  }

  /// Get the default output extension for the encoder
  pub const fn output_extension(&self) -> &str {
    match &self {
      Self::aom | Self::rav1e | Self::vpx | Self::svt_av1 => "ivf",
      Self::x264 | Self::x265 => "mkv",
    }
  }

  /// Returns regex used for matching q/crf arguments in command line
  fn q_regex(&self) -> &'static Regex {
    match &self {
      Self::aom | Self::vpx => regex!(r"--cq-level=.+"),
      Self::rav1e => regex!(r"--quantizer"),
      Self::svt_av1 => regex!(r"(--qp|-q|--crf)"),
      Self::x264 | Self::x265 => regex!(r"--crf"),
    }
  }

  fn replace_q(self, index: usize, q: usize) -> (usize, String) {
    match &self {
      Self::aom | Self::vpx => (index, format!("--cq-level={}", q)),
      Self::rav1e | Self::svt_av1 | Self::x265 | Self::x264 => (index + 1, q.to_string()),
    }
  }

  /// Returns changed q/crf in command line arguments
  pub fn man_command(self, params: Vec<String>, q: usize) -> Vec<String> {
    let index = list_index_of_regex(&params, self.q_regex()).unwrap();

    let mut new_params = params;
    let (replace_index, replace_q) = self.replace_q(index, q);
    new_params[replace_index] = replace_q;

    new_params
  }

  /// Retuns regex for matching encoder progress in cli
  fn pipe_match(&self) -> &'static Regex {
    match &self {
      Self::aom | Self::vpx => regex!(r".*Pass (?:1/1|2/2) .*frame.*?/([^ ]+?) "),
      Self::rav1e => regex!(r"encoded.*? ([^ ]+?) "),
      Self::svt_av1 => regex!(r"Encoding frame\s+(\d+)"),
      Self::x264 => regex!(r"^[^\d]*(\d+)"),
      Self::x265 => regex!(r"(\d+) frames"),
    }
  }

  /// Returs option of q/crf value from cli encoder output
  pub fn match_line(self, line: &str) -> Option<usize> {
    let encoder_regex = self.pipe_match();
    if !encoder_regex.is_match(line) {
      return Some(0);
    }
    let captures = encoder_regex.captures(line)?.get(1)?.as_str();
    captures.parse::<usize>().ok()
  }

  /// Returns command used for target quality probing
  pub fn construct_target_quality_command(
    self,
    threads: usize,
    q: usize,
  ) -> Vec<Cow<'static, str>> {
    match &self {
      Self::aom => into_vec![
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
      Self::rav1e => into_vec![
        "rav1e",
        "-y",
        "-s",
        "10",
        "--threads",
        threads.to_string(),
        "--tiles",
        "16",
        "--quantizer",
        q.to_string(),
        "--low-latency",
        "--rdo-lookahead-frames",
        "5",
        "--no-scene-detection",
      ],
      Self::vpx => into_vec![
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
      Self::svt_av1 => into_vec![
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
        q.to_string(),
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
      Self::x264 => into_vec![
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
        q.to_string(),
      ],
      Self::x265 => into_vec![
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
        q.to_string(),
      ],
    }
  }

  /// Returns command used for target quality probing (slow, correctness focused version)
  pub fn construct_target_quality_command_probe_slow(self, q: usize) -> Vec<Cow<'static, str>> {
    match &self {
      Self::aom => into_vec!["aomenc", "--passes=1", format!("--cq-level={}", q)],
      Self::rav1e => into_vec!["rav1e", "-y", "--quantizer", q.to_string()],
      Self::vpx => into_vec![
        "vpxenc",
        "--passes=1",
        "--pass=1",
        "--codec=vp9",
        "--end-usage=q",
        format!("--cq-level={}", q),
      ],
      Self::svt_av1 => into_vec!["SvtAv1EncApp", "-i", "stdin", "--crf", q.to_string()],
      Self::x264 => into_vec![
        "x264",
        "--log-level",
        "error",
        "--demuxer",
        "y4m",
        "-",
        "--no-progress",
        "--crf",
        q.to_string(),
      ],
      Self::x265 => into_vec![
        "x265",
        "--log-level",
        "0",
        "--no-progress",
        "--y4m",
        "--crf",
        q.to_string(),
      ],
    }
  }

  /// Function remove_patterns that takes in args and patterns and removes all instances of the patterns from the args.
  pub fn remove_patterns(args: &mut Vec<String>, patterns: &[&str]) {
    for pattern in patterns {
      if let Some(index) = args.iter().position(|value| value.contains(pattern)) {
        args.remove(index);
        // If pattern does not contain =, we need to remove the index that follows.
        if !pattern.contains('=') {
          args.remove(index);
        }
      }
    }
  }

  /// Constructs tuple of commands for target quality probing
  pub fn probe_cmd(
    self,
    temp: String,
    name: String,
    q: usize,
    ffmpeg_pipe: Vec<String>,
    probing_rate: usize,
    n_threads: usize,
    mut video_params: Vec<String>,
    probe_slow: bool,
  ) -> (Vec<String>, Vec<Cow<'static, str>>) {
    let pipe: Vec<String> = chain!(
      into_vec![
        "ffmpeg",
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        "-",
        "-vf",
        format!("select=not(mod(n\\,{}))", probing_rate).as_str(),
        "-vsync",
        "0",
      ],
      ffmpeg_pipe
    )
    .collect();

    let probe_name = format!("v_{}{}.ivf", q, name);
    let mut probe = PathBuf::from(temp);
    probe.push("split");
    probe.push(&probe_name);
    let probe_path = probe.to_str().unwrap().to_owned();

    let params: Vec<Cow<str>> = if probe_slow {
      let patterns = [
        "--cq-level=",
        "--passes=",
        "--pass=",
        "--crf",
        "--quantizer",
      ];
      Self::remove_patterns(&mut video_params, &patterns);
      let mut ps = self.construct_target_quality_command_probe_slow(q);

      ps.reserve(video_params.len());
      for arg in video_params {
        ps.push(Cow::Owned(arg));
      }

      ps
    } else {
      self.construct_target_quality_command(n_threads, q)
    };

    let output: Vec<Cow<str>> = match self {
      Self::svt_av1 => chain!(params, into_vec!["-b", probe_path]).collect(),
      Self::aom | Self::rav1e | Self::vpx | Self::x264 | Self::x265 => {
        chain!(params, into_vec!["-o", probe_path, "-"]).collect()
      }
    };

    (pipe, output)
  }
}
