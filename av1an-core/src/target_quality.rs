use crate::logger::log;

use splines::{Interpolation, Key, Spline};
use std::cmp::Ordering;
use std::fmt::Error;

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

pub fn vmaf_auto_threads(workers: usize) -> usize {
  const OVER_PROVISION_FACTOR: f64 = 1.25;

  // Logical CPUs
  let threads = num_cpus::get();

  std::cmp::max(
    ((threads / workers) as f64 * OVER_PROVISION_FACTOR) as usize,
    1,
  )
}

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

pub fn log_probes(
  vmaf_cq_scores: Vec<(f64, u32)>,
  frames: u32,
  probing_rate: u32,
  name: &str,
  target_q: u32,
  target_vmaf: f64,
  skip: &str,
) {
  let skip_string = match skip {
    "high" => "Early Skip High Q".to_string(),
    "low" => "Early Skip Low Q".to_string(),
    _ => "".to_string(),
  };

  let mut scores_sorted = vmaf_cq_scores;
  scores_sorted.sort_by_key(|x| x.1);

  log(format!("Chunk: {}, Rate: {}, Fr {}", name, probing_rate, frames).as_str());
  log(format!("Probes {:?} {}", scores_sorted, skip_string).as_str());
  log(format!("Target Q: {:.0} VMAF: {:.2}", target_q, target_vmaf).as_str());
}
