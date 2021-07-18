#![warn(clippy::needless_pass_by_value)]

#[macro_use]
extern crate log;

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use sysinfo::SystemExt;
pub mod concat;
pub mod ffmpeg;
pub mod file_validation;
pub mod logger;
pub mod progress_bar;
pub mod split;
pub mod target_quality;
pub mod vapoursynth;
pub mod vmaf;

use itertools::chain;
use regex::Regex;
use std::borrow::Cow;
use std::cmp;
use std::path::PathBuf;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]

pub enum Encoder {
  aom,
  rav1e,
  libvpx,
  #[strum(serialize = "svt-av1")]
  svt_av1,
  x264,
  x265,
}

macro_rules! into_vec {
  ($($x:expr),* $(,)?) => {
    vec![
      $(
        $x.into(),
      )*
    ]
  };
}

impl Encoder {
  pub fn compose_1_1_pass(&self, params: Vec<String>, output: String) -> Vec<String> {
    match &self {
      // Aomenc
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=1"],
        params,
        into_vec!["-o", output, "-"],
      )
      .collect(),

      // Rav1e
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y"],
        params,
        into_vec!["--output", output]
      )
      .collect(),

      // VPX
      Self::libvpx => chain!(
        into_vec!["vpxenc", "--passes=1"],
        params,
        into_vec!["-o", output, "-"]
      )
      .collect(),

      // SVT-AV1
      Self::svt_av1 => chain!(
        into_vec!["SvtAv1EncApp", "-i", "stdin", "--progress", "2",],
        params,
        into_vec!["-b", output,],
      )
      .collect(),

      // x264
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
        into_vec!["-", "-o", output,]
      )
      .collect(),

      // x265
      Self::x265 => chain!(
        into_vec!["x265", "--y4m",],
        params,
        into_vec!["-", "-o", output,]
      )
      .collect(),
    }
  }

  pub fn compose_1_2_pass(&self, params: Vec<String>, fpf: String) -> Vec<String> {
    match &self {
      // Aomenc
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=2", "--pass=1"],
        params,
        into_vec![
          format!("--fpf={}.log", fpf),
          "-o",
          if cfg!(windows) { "nul" } else { "/dev/null" },
          "-"
        ],
      )
      .collect(),

      // Rav1e
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y", "-q"],
        params,
        into_vec![
          "--first-pass",
          format!("{}.stat", fpf),
          "--output",
          if cfg!(windows) { "nul" } else { "/dev/null" },
        ]
      )
      .collect(),

      // VPX
      Self::libvpx => chain!(
        into_vec!["vpxenc", "--passes=2", "--pass=1"],
        params,
        into_vec![
          format!("--fpf={}.log", fpf),
          "-o",
          if cfg!(windows) { "nul" } else { "/dev/null" },
          "-"
        ],
      )
      .collect(),

      // SVT-AV1
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
          if cfg!(windows) { "nul" } else { "/dev/null" },
        ],
      )
      .collect(),

      // x264
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
        into_vec![
          "--stats",
          format!("{}.log", fpf),
          "-",
          "-o",
          if cfg!(windows) { "nul" } else { "/dev/null" },
        ]
      )
      .collect(),

      // x265
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
        into_vec![
          "--stats",
          format!("{}.log", fpf),
          "-",
          "-o",
          if cfg!(windows) { "nul" } else { "/dev/null" },
        ]
      )
      .collect(),
    }
  }

  pub fn compose_2_2_pass(&self, params: Vec<String>, fpf: String, output: String) -> Vec<String> {
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
        into_vec!["--second-pass", format!("{}.stat", fpf), "--output", output,]
      )
      .collect(),
      Self::libvpx => chain!(
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
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", output,]
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
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", output,]
      )
      .collect(),
    }
  }

  pub fn get_default_arguments(&self) -> Vec<&str> {
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
      Encoder::libvpx => into_vec![
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

  pub fn get_default_pass(&self) -> u8 {
    match &self {
      Self::aom => 2,
      Self::rav1e => 1,
      Self::libvpx => 2,
      Self::svt_av1 => 1,
      Self::x264 => 1,
      Self::x265 => 1,
    }
  }

  /// Default quantizer range target quality mode
  pub fn get_default_cq_range(&self) -> (usize, usize) {
    match &self {
      Self::aom => (15, 55),
      Self::rav1e => (50, 140),
      Self::libvpx => (15, 55),
      Self::svt_av1 => (15, 50),
      Self::x264 => (15, 35),
      Self::x265 => (15, 35),
    }
  }

  pub fn help_command(&self) -> [&str; 2] {
    match &self {
      Self::aom => ["aomenc", "--help"],
      Self::rav1e => ["rav1e", "--fullhelp"],
      Self::libvpx => ["vpxenc", "--help"],
      Self::svt_av1 => ["SvtAv1EncApp", "--help"],
      Self::x264 => ["x264", "--fullhelp"],
      Self::x265 => ["x265", "--fullhelp"],
    }
  }

  /// Default quantizer range target quality mode
  pub fn encoder_bin(&self) -> &str {
    match &self {
      Self::aom => "aomenc",
      Self::rav1e => "rav1e",
      Self::libvpx => "vpxenc",
      Self::svt_av1 => "SvtAv1EncApp",
      Self::x264 => "x264",
      Self::x265 => "x265",
    }
  }

  pub fn output_extension(&self) -> &str {
    match &self {
      Self::aom => "ivf",
      Self::rav1e => "ivf",
      Self::libvpx => "ivf",
      Self::svt_av1 => "ivf",
      Self::x264 => "mkv",
      Self::x265 => "mkv",
    }
  }

  fn q_regex_str(&self) -> &str {
    match &self {
      Self::aom => r"--cq-level=.+",
      Self::rav1e => r"--quantizer",
      Self::libvpx => r"--cq-level=.+",
      Self::svt_av1 => r"(--qp|-q|--crf)",
      Self::x264 => r"--crf",
      Self::x265 => r"--crf",
    }
  }

  fn replace_q(&self, index: usize, q: usize) -> (usize, String) {
    match &self {
      Self::aom => (index, format!("--cq-level={}", q)),
      Self::rav1e => (index + 1, q.to_string()),
      Self::libvpx => (index, format!("--cq-level={}", q)),
      Self::svt_av1 => (index + 1, q.to_string()),
      Self::x264 => (index + 1, q.to_string()),
      Self::x265 => (index + 1, q.to_string()),
    }
  }

  pub fn man_command(&self, params: Vec<String>, q: usize) -> Vec<String> {
    let index = list_index_of_regex(params.clone(), self.q_regex_str()).unwrap();

    let mut new_params = params;
    let (replace_index, replace_q) = self.replace_q(index, q);
    new_params[replace_index] = replace_q;

    new_params
  }

  fn pipe_match(&self) -> &str {
    match &self {
      Self::aom => r".*Pass (?:1/1|2/2) .*frame.*?/([^ ]+?) ",
      Self::rav1e => r"encoded.*? ([^ ]+?) ",
      Self::libvpx => r".*Pass (?:1/1|2/2) .*frame.*?/([^ ]+?) ",
      Self::svt_av1 => r"Encoding frame\s+(\d+)",
      Self::x264 => r"^[^\d]*(\d+)",
      Self::x265 => r"(\d+) frames",
    }
  }

  pub fn match_line(&self, line: &str) -> Option<usize> {
    let encoder_regex = Regex::new(self.pipe_match()).unwrap();
    if !encoder_regex.is_match(line) {
      return Some(0);
    }
    let captures = encoder_regex.captures(line)?.get(1)?.as_str();
    captures.parse::<usize>().ok()
  }

  pub fn construct_target_quality_command(&self, threads: String, q: String) -> Vec<Cow<str>> {
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
        threads,
        "--tiles",
        "16",
        "--quantizer",
        q,
        "--low-latency",
        "--rdo-lookahead-frames",
        "5",
        "--no-scene-detection",
      ],
      Self::libvpx => into_vec![
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
        threads,
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
      Self::x264 => into_vec![
        "x264",
        "--log-level",
        "error",
        "--demuxer",
        "y4m",
        "-",
        "--no-progress",
        "--threads",
        threads,
        "--preset",
        "medium",
        "--crf",
        q,
      ],
      Self::x265 => into_vec![
        "x265",
        "--log-level",
        "0",
        "--no-progress",
        "--y4m",
        "--frame-threads",
        cmp::min(threads.parse().unwrap(), 16).to_string(),
        "--preset",
        "fast",
        "--crf",
        q,
      ],
    }
  }

  pub fn construct_target_quality_command_probe_slow(&self, q: String) -> Vec<Cow<str>> {
    match &self {
      Self::aom => into_vec!["aomenc", "--passes=1", format!("--cq-level={}", q),],
      Self::rav1e => into_vec!["rav1e", "-y", "--quantizer", q,],
      Self::libvpx => into_vec![
        "vpxenc",
        "--passes=1",
        "--pass=1",
        "--codec=vp9",
        "--end-usage=q",
        format!("--cq-level={}", q),
      ],
      Self::svt_av1 => into_vec!["SvtAv1EncApp", "-i", "stdin", "--crf", q,],
      Self::x264 => into_vec![
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
      Self::x265 => into_vec![
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

  // Function remove_patterns that takes in args and patterns and removes all instances of the patterns from the args.
  pub fn remove_patterns(&self, args: Vec<String>, patterns: Vec<String>) -> Vec<String> {
    let mut out = args;
    for pattern in patterns {
      if let Some(index) = out.iter().position(|value| value.contains(&pattern)) {
        out.remove(index);
        // If pattern does not contain =, we need to remove the index that follows.
        if !pattern.contains('=') {
          out.remove(index);
        }
      }
    }
    out
  }

  // Function unwrap cow strings that take in a vec of strings and returns a vec of strings.
  pub fn decow_strings(&self, args: Vec<Cow<str>>) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect::<Vec<String>>()
  }

  pub fn probe_cmd(
    &self,
    temp: String,
    name: String,
    q: String,
    ffmpeg_pipe: Vec<String>,
    probing_rate: String,
    n_threads: String,
    video_params: Vec<String>,
    probe_slow: bool,
  ) -> (Vec<String>, Vec<String>) {
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
    let probe_path = probe.into_os_string().into_string().unwrap();

    let mut params;
    if probe_slow {
      let mut args = video_params;
      let patterns = into_vec![
        "--cq-level=",
        "--passes=",
        "--pass=",
        "--crf",
        "--quantizer"
      ];
      args = self.remove_patterns(args, patterns);
      let ps = self.construct_target_quality_command_probe_slow(q);
      params = self.decow_strings(ps);
      params.append(&mut args)
    } else {
      let ps = self.construct_target_quality_command(n_threads, q);
      params = self.decow_strings(ps);
    }

    let output: Vec<String> = match &self {
      Self::aom => chain!(params, into_vec!["-o", probe_path, "-"]).collect(),
      Self::rav1e => chain!(params, into_vec!["-o", probe_path, "-"]).collect(),
      Self::svt_av1 => chain!(params, into_vec!["-b", probe_path]).collect(),
      Self::libvpx => chain!(params, into_vec!["-o", probe_path, "-"]).collect(),
      Self::x264 => chain!(params, into_vec!["-o", probe_path, "-"]).collect(),
      Self::x265 => chain!(params, into_vec!["-o", probe_path, "-"]).collect(),
    };

    (pipe, output)
  }
}

pub fn compose_ffmpeg_pipe(params: Vec<String>) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params);

  p
}

pub fn list_index_of_regex(params: Vec<String>, regex_str: &str) -> Option<usize> {
  let re = Regex::new(regex_str).unwrap();

  assert!(
    !params.is_empty(),
    "List index of regex got empty list of params"
  );

  for (i, cmd) in params.iter().enumerate() {
    if re.is_match(cmd) {
      return Some(i);
    }
  }
  panic!("No match found for params: {:#?}", params)
}

#[derive(Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum ConcatMethod {
  #[strum(serialize = "mkvmerge")]
  MKVMerge,
  #[strum(serialize = "ffmpeg")]
  FFmpeg,
  #[strum(serialize = "ivf")]
  Ivf,
}

#[derive(Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum SplitMethod {
  #[strum(serialize = "av-scenechange")]
  AvScenechange,
  #[strum(serialize = "none")]
  None,
}

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, strum::EnumString, strum::IntoStaticStr)]
pub enum ChunkMethod {
  #[strum(serialize = "select")]
  Select,
  #[strum(serialize = "hybrid")]
  Hybrid,
  #[strum(serialize = "ffms2")]
  FFMS2,
  #[strum(serialize = "lsmash")]
  LSMASH,
}

/// Check for FFmpeg
pub fn get_ffmpeg_info() -> String {
  let mut cmd = Command::new("ffmpeg");
  cmd.stderr(Stdio::piped());
  String::from_utf8(cmd.output().unwrap().stderr).unwrap()
}

pub fn adapt_probing_rate(rate: usize) -> usize {
  match rate {
    1..=4 => rate,
    _ => 4,
  }
}

/// Determine the optimal number of workers for an encoder
#[must_use]
pub fn determine_workers(encoder: Encoder) -> u64 {
  // TODO look for lighter weight solution? sys-info maybe?
  let mut system = sysinfo::System::new();
  system.refresh_memory();

  let cpu = num_cpus::get() as u64;
  // available_memory returns kb, convert to gb
  let ram_gb = system.available_memory() / 10u64.pow(6);

  std::cmp::max(
    match encoder {
      Encoder::aom | Encoder::rav1e | Encoder::libvpx => std::cmp::min(
        (cpu as f64 / 3.0).round() as u64,
        (ram_gb as f64 / 1.5).round() as u64,
      ),
      Encoder::svt_av1 | Encoder::x264 | Encoder::x265 => std::cmp::min(cpu, ram_gb) / 8,
    },
    1,
  )
}

pub fn get_percentile(scores: &mut [f64], percentile: f64) -> f64 {
  // Calculates percentile from vector of valuees
  scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));

  let k = (scores.len() - 1) as f64 * percentile;
  let f = k.floor();
  let c = k.ceil();

  if f == c {
    return scores[k as usize];
  }

  let d0 = scores[f as usize] as f64 * (c - k);
  let d1 = scores[f as usize] as f64 * (k - f);

  d0 + d1
}

#[derive(Deserialize, Debug)]
struct Foo {
  vmaf: f64,
}

#[derive(Deserialize, Debug)]
struct Bar {
  metrics: Foo,
}

#[derive(Deserialize, Debug)]
struct Baz {
  frames: Vec<Bar>,
}

pub fn read_file_to_string(file: &Path) -> Result<String, Error> {
  Ok(fs::read_to_string(&file).unwrap_or_else(|_| panic!("Can't open file {:?}", file)))
}

pub fn read_vmaf_file(file: &Path) -> Result<Vec<f64>, serde_json::Error> {
  let json_str = read_file_to_string(file).unwrap();
  let bazs = serde_json::from_str::<Baz>(&json_str)?;
  let v = bazs
    .frames
    .into_iter()
    .map(|x| x.metrics.vmaf)
    .collect::<Vec<_>>();

  Ok(v)
}

pub fn read_weighted_vmaf(file: &Path, percentile: f64) -> Result<f64, serde_json::Error> {
  let mut scores = read_vmaf_file(file).unwrap();

  Ok(get_percentile(&mut scores, percentile))
}
