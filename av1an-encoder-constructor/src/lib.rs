use itertools::chain;
use regex::Regex;
use std::borrow::Cow;
use std::cmp;
use std::path::PathBuf;
use std::{str::FromStr, usize};
macro_rules! into_vec {
  ($($x:expr),* $(,)?) => {
    vec![
      $(
        $x.into(),
      )*
    ]
  };
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum Encoder {
  aom,
  rav1e,
  libvpx,
  svt_av1,
  x264,
  x265,
}

impl FromStr for Encoder {
  type Err = ();

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    // set to match usage in python code
    match s {
      "aom" => Ok(Self::aom),
      "rav1e" => Ok(Self::rav1e),
      "vpx" => Ok(Self::libvpx),
      "svt_av1" => Ok(Self::svt_av1),
      "x264" => Ok(Self::x264),
      "x265" => Ok(Self::x265),
      _ => Err(()),
    }
  }
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
      // Aomenc
      Self::aom => chain!(
        into_vec!["aomenc", "--passes=2", "--pass=2"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", output, "-"],
      )
      .collect(),

      // Rav1e
      Self::rav1e => chain!(
        into_vec!["rav1e", "-", "-y", "-q"],
        params,
        into_vec!["--second-pass", format!("{}.stat", fpf), "--output", output,]
      )
      .collect(),

      // VPX
      Self::libvpx => chain!(
        into_vec!["vpxenc", "--passes=2", "--pass=2"],
        params,
        into_vec![format!("--fpf={}.log", fpf), "-o", output, "-"],
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
          "2",
          "--stats",
          format!("{}.stat", fpf),
          "-b",
          output,
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
          "2",
          "--demuxer",
          "y4m",
        ],
        params,
        into_vec!["--stats", format!("{}.log", fpf), "-", "-o", output,]
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
      // Aomenc
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

      // Rav1e
      Encoder::rav1e => into_vec![
        "--tiles",
        "8",
        "--speed",
        "6",
        "--quantizer",
        "100",
        "--no-scene-detection",
      ],

      //VPX
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

      // SVT-AV1
      Encoder::svt_av1 => {
        into_vec!["--preset", "4", "--keyint", "240", "--rc", "0", "--crf", "25",]
      }

      // x264
      Encoder::x264 => into_vec!["--preset", "slow", "--crf", "25"],

      // x265
      Encoder::x265 => into_vec!["-p", "slow", "--crf", "25", "-D", "10"],
    }
  }

  pub fn get_default_pass(&self) -> usize {
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

  pub fn match_line(&self, line: String) -> Option<usize> {
    let encoder_regex = Regex::new(self.pipe_match()).unwrap();
    if !encoder_regex.is_match(&line) {
      return Some(0);
    }
    let captures = encoder_regex
      .captures(&line)
      .unwrap()
      .get(1)
      .unwrap()
      .as_str();
    let result = captures.parse::<usize>().unwrap();
    Some(result)
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
  pub fn probe_cmd(
    &self,
    temp: String,
    name: String,
    q: String,
    ffmpeg_pipe: Vec<String>,
    probing_rate: String,
    n_threads: String,
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

    let ps = self.construct_target_quality_command(n_threads, q);
    let params = ps.iter().map(|s| s.to_string()).collect::<Vec<String>>();

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
  pub fn construct_target_quality_slow_command(&self, q: String) -> Vec<Cow<str>> {
    match &self {
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
