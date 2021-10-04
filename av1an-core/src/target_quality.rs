use crate::{
  chunk::Chunk,
  process_pipe,
  project::Project,
  vmaf::{self, read_weighted_vmaf},
  Encoder,
};
use splines::{Interpolation, Key, Spline};
use std::{cmp::Ordering, convert::TryInto, fmt::Error, path::Path, process::Stdio};

// TODO: just make it take a reference to a `Project`
pub struct TargetQuality<'a> {
  vmaf_res: String,
  vmaf_filter: Option<&'a str>,
  n_threads: usize,
  model: Option<&'a Path>,
  probing_rate: usize,
  probes: u32,
  target: f32,
  min_q: u32,
  max_q: u32,
  encoder: Encoder,
  ffmpeg_pipe: Vec<String>,
  temp: String,
  workers: usize,
  video_params: Vec<String>,
  probe_slow: bool,
}

impl<'a> TargetQuality<'a> {
  pub fn new(project: &'a Project) -> Self {
    Self {
      vmaf_res: project
        .vmaf_res
        .clone()
        .unwrap_or_else(|| String::with_capacity(0)),
      vmaf_filter: project.vmaf_filter.as_deref(),
      n_threads: project.vmaf_threads.unwrap_or(0) as usize,
      model: project.vmaf_path.as_deref(),
      probes: project.probes,
      target: project.target_quality.unwrap(),
      min_q: project.min_q.unwrap(),
      max_q: project.max_q.unwrap(),
      encoder: project.encoder,
      ffmpeg_pipe: project.ffmpeg_pipe.clone(),
      temp: project.temp.clone(),
      workers: project.workers,
      video_params: project.video_params.clone(),
      probe_slow: project.probe_slow,
      probing_rate: adapt_probing_rate(project.probing_rate as usize),
    }
  }

  fn per_shot_target_quality(&self, chunk: &Chunk) -> u32 {
    let mut vmaf_cq = vec![];
    let frames = chunk.frames;

    let mut q_list = vec![];

    // Make middle probe
    let middle_point = (self.min_q + self.max_q) / 2;
    q_list.push(middle_point);
    let last_q = middle_point;

    let mut score = read_weighted_vmaf(self.vmaf_probe(chunk, last_q as usize), 0.25).unwrap();
    vmaf_cq.push((score, last_q));

    // Initialize search boundary
    let mut vmaf_lower = score;
    let mut vmaf_upper = score;
    let mut vmaf_cq_lower = last_q;
    let mut vmaf_cq_upper = last_q;

    // Branch
    let next_q = if score < f64::from(self.target) {
      self.min_q
    } else {
      self.max_q
    };

    q_list.push(next_q);

    // Edge case check
    score = read_weighted_vmaf(self.vmaf_probe(chunk, next_q as usize), 0.25).unwrap();
    vmaf_cq.push((score, next_q));

    if (next_q == self.min_q && score < f64::from(self.target))
      || (next_q == self.max_q && score > f64::from(self.target))
    {
      log_probes(
        vmaf_cq,
        frames as u32,
        self.probing_rate as u32,
        &chunk.name(),
        next_q,
        score,
        if score < f64::from(self.target) {
          Skip::Low
        } else {
          Skip::High
        },
      );
      return next_q;
    }

    // Set boundary
    if score < f64::from(self.target) {
      vmaf_lower = score;
      vmaf_cq_lower = next_q;
    } else {
      vmaf_upper = score;
      vmaf_cq_upper = next_q;
    }

    // VMAF search
    for _ in 0..self.probes - 2 {
      let new_point = weighted_search(
        f64::from(vmaf_cq_lower),
        vmaf_lower,
        f64::from(vmaf_cq_upper),
        vmaf_upper,
        f64::from(self.target),
      );

      if vmaf_cq
        .iter()
        .map(|(_, x)| *x)
        .any(|x| x == new_point as u32)
      {
        break;
      }

      q_list.push(new_point as u32);
      score = read_weighted_vmaf(self.vmaf_probe(chunk, new_point), 0.25).unwrap();
      vmaf_cq.push((score, new_point as u32));

      // Update boundary
      if score < f64::from(self.target) {
        vmaf_lower = score;
        vmaf_cq_lower = new_point as u32;
      } else {
        vmaf_upper = score;
        vmaf_cq_upper = new_point as u32;
      }
    }

    let (q, q_vmaf) = interpolated_target_q(vmaf_cq.clone(), f64::from(self.target));
    log_probes(
      vmaf_cq,
      frames as u32,
      self.probing_rate as u32,
      &chunk.name(),
      q as u32,
      q_vmaf,
      Skip::None,
    );

    q as u32
  }

  fn vmaf_probe(&self, chunk: &Chunk, q: usize) -> String {
    let n_threads = if self.n_threads == 0 {
      vmaf_auto_threads(self.workers)
    } else {
      self.n_threads
    };

    let cmd = self.encoder.probe_cmd(
      self.temp.clone(),
      chunk.name(),
      q,
      self.ffmpeg_pipe.clone(),
      self.probing_rate,
      n_threads,
      self.video_params.clone(),
      self.probe_slow,
    );

    let future = async {
      let mut source = if let [pipe_cmd, args @ ..] = &*chunk.source {
        tokio::process::Command::new(pipe_cmd)
          .args(args)
          .stderr(Stdio::piped())
          .stdout(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let source_pipe_stdout: Stdio = source.stdout.take().unwrap().try_into().unwrap();

      let mut ffmpeg_pipe = if let [ffmpeg, args @ ..] = &*cmd.0 {
        tokio::process::Command::new(ffmpeg)
          .args(args)
          .stdin(source_pipe_stdout)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      let ffmpeg_pipe_stdout: Stdio = ffmpeg_pipe.stdout.take().unwrap().try_into().unwrap();

      let pipe = if let [cmd, args @ ..] = &*cmd.1 {
        tokio::process::Command::new(cmd.as_ref())
          .args(args.iter().map(AsRef::as_ref))
          .stdin(ffmpeg_pipe_stdout)
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .spawn()
          .unwrap()
      } else {
        unreachable!()
      };

      process_pipe(pipe, chunk.index).await.unwrap();
    };

    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_io()
      .build()
      .unwrap();

    rt.block_on(future);

    let probe_name =
      Path::new(&chunk.temp)
        .join("split")
        .join(format!("v_{}{}.ivf", q, chunk.name()));
    let fl_path = Path::new(&chunk.temp)
      .join("split")
      .join(format!("{}.json", chunk.name()));

    let fl_path = fl_path.to_str().unwrap().to_owned();

    vmaf::run_vmaf(
      &probe_name,
      chunk.source.as_slice(),
      &fl_path,
      self.model.as_ref(),
      &self.vmaf_res,
      self.probing_rate,
      self.vmaf_filter,
      self.n_threads,
    )
    .unwrap();

    fl_path
  }

  pub fn per_shot_target_quality_routine(&self, chunk: &mut Chunk) {
    chunk.per_shot_target_quality_cq = Some(self.per_shot_target_quality(chunk));
  }
}

pub fn weighted_search(num1: f64, vmaf1: f64, num2: f64, vmaf2: f64, target: f64) -> usize {
  let dif1 = (transform_vmaf(target as f64) - transform_vmaf(vmaf2)).abs();
  let dif2 = (transform_vmaf(target as f64) - transform_vmaf(vmaf1)).abs();

  let tot = dif1 + dif2;

  num1.mul_add(dif1 / tot, num2 * (dif2 / tot)).round() as usize
}

pub fn transform_vmaf(vmaf: f64) -> f64 {
  let x: f64 = 1.0 - vmaf / 100.0;
  if vmaf < 99.99 {
    -x.ln()
  } else {
    9.2
  }
}

/// Returns auto detected amount of threads used for vmaf calculation
pub fn vmaf_auto_threads(workers: usize) -> usize {
  const OVER_PROVISION_FACTOR: f64 = 1.25;

  // Logical CPUs
  let threads = num_cpus::get();

  std::cmp::max(
    ((threads / workers) as f64 * OVER_PROVISION_FACTOR) as usize,
    1,
  )
}

/// Use linear interpolation to get q/crf values closest to the target value
pub fn interpolate_target_q(scores: Vec<(f64, u32)>, target: f64) -> Result<f64, Error> {
  let mut sorted = scores;
  sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

  let keys = sorted
    .iter()
    .map(|(x, y)| Key::new(*x, f64::from(*y), Interpolation::Linear))
    .collect();

  let spline = Spline::from_vec(keys);

  Ok(spline.sample(target).unwrap())
}

/// Use linear interpolation to get vmaf value that expected from q
pub fn interpolate_target_vmaf(scores: Vec<(f64, u32)>, q: f64) -> Result<f64, Error> {
  let mut sorted = scores;
  sorted.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(Ordering::Less));

  let keys = sorted
    .iter()
    .map(|f| Key::new(f64::from(f.1), f.0 as f64, Interpolation::Linear))
    .collect();

  let spline = Spline::from_vec(keys);

  Ok(spline.sample(q).unwrap())
}

#[derive(Copy, Clone)]
pub enum Skip {
  High,
  Low,
  None,
}

pub fn log_probes(
  vmaf_cq_scores: Vec<(f64, u32)>,
  frames: u32,
  probing_rate: u32,
  name: &str,
  target_q: u32,
  target_vmaf: f64,
  skip: Skip,
) {
  let mut scores_sorted = vmaf_cq_scores;
  scores_sorted.sort_by_key(|x| x.1);

  info!("Chunk: {}, Rate: {}, Fr {}", name, probing_rate, frames);
  info!(
    "Probes {:?}{}",
    scores_sorted,
    match skip {
      Skip::High => " Early Skip High Q",
      Skip::Low => " Early Skip Low Q",
      Skip::None => "",
    }
  );
  info!("Target Q: {:.0} VMAF: {:.2}", target_q, target_vmaf);
}

pub const fn adapt_probing_rate(rate: usize) -> usize {
  match rate {
    1..=4 => rate,
    _ => 4,
  }
}

pub fn interpolated_target_q(scores: Vec<(f64, u32)>, target: f64) -> (f64, f64) {
  let q = interpolate_target_q(scores.clone(), target).unwrap();

  let vmaf = interpolate_target_vmaf(scores, q).unwrap();

  (q, vmaf)
}
