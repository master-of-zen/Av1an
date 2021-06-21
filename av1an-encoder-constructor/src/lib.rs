use std::str::FromStr;

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
      Encoder::svt_av1 => {
        into_vec!["--preset", "4", "--keyint", "240", "--rc", "0", "--crf", "25",]
      }
      Encoder::x264 => into_vec!["--preset", "slow", "--crf", "25"],
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
