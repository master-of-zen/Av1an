use core::f64;
use std::fs::read;
use std::mem::transmute;
use std::path::{Path, PathBuf};
use std::u64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AomFirstPassStats {
  frame: f64,                    // Frame number
  weight: f64,                   // Weight assigned to this frame
  intra_error: f64,              // Intra prediction error.
  frame_avg_wavelet_energy: f64, // Average wavelet energy computed using Discrete Wavelet Transform (DWT).
  coded_error: f64,
  sr_coded_error: f64,
  tr_coded_error: f64,
  pcnt_inter: f64,
  pcnt_motion: f64,
  pcnt_second_ref: f64,
  pcnt_third_ref: f64,
  pcnt_neutral: f64,
  intra_skip_pct: f64,
  inactive_zone_rows: f64,
  inactive_zone_cols: f64,
  mvr: f64,
  mvr_abs: f64,
  mvc: f64,
  mvc_abs: f64,
  mvrv: f64,
  mvcv: f64,
  mv_in_out_count: f64,
  new_mv_count: f64,
  duration: f64,
  count: f64,
  raw_error_stdev: f64,
  is_flash: f64,
  noise_var: f64,
  cor_coeff: f64,
}

pub fn read_aomenc_stats_struct(file: PathBuf) -> Vec<AomFirstPassStats> {
  let raw_data: Vec<u8> = read(file).unwrap();
  let frame_list: Vec<AomFirstPassStats> = unsafe { transmute(raw_data) };
  frame_list
}

fn get_second_ref_usage_thresh(frame_count_so_far: u64) -> f64 {
  let adapt_upto = 32.0;
  let min_second_ref_usage_thresh = 0.085;
  let second_ref_usage_thresh_max_delta = 0.035;

  if frame_count_so_far as f64 >= adapt_upto {
    min_second_ref_usage_thresh + second_ref_usage_thresh_max_delta
  } else {
    min_second_ref_usage_thresh
      + (frame_count_so_far as f64 / (adapt_upto - 1.0)) * second_ref_usage_thresh_max_delta
  }
}

fn double_divide_check(x: f64) -> f64 {
  if x < 0.0 {
    x - 0.000001
  } else {
    x + 0.000001
  }
}

fn test_candidate_kf(
  dict_list: Vec<AomFirstPassStats>,
  current_frame_index: u64,
  frame_count_so_far: u64,
) -> bool {
  let p: AomFirstPassStats = dict_list[(current_frame_index - 1) as usize];
  let c: AomFirstPassStats = dict_list[(current_frame_index) as usize];
  let f: AomFirstPassStats = dict_list[(current_frame_index + 1) as usize];

  let boost_factor = 12.5;
  let min_intra_level = 0.25;
  let intra_vs_inter_thresh = 2.0;
  let very_low_inter_thresh = 0.05;
  let kf_ii_err_threshold = 2.5;
  let err_change_threshold = 0.4;
  let ii_improvement_threshold = 3.5;
  let kf_ii_max = 128.0;

  let qmode = true;
  let mut is_keyframe = false;

  let pcnt_intra = 1.0 - c.pcnt_inter;
  let modified_pcnt_inter = c.pcnt_inter - c.pcnt_neutral;

  let mut second_ref_usage_thresh = get_second_ref_usage_thresh(frame_count_so_far);

  if frame_count_so_far > 2
    && (c.pcnt_second_ref < second_ref_usage_thresh)
    && (f.pcnt_second_ref < second_ref_usage_thresh)
    && ((c.pcnt_inter < very_low_inter_thresh)
      || ((pcnt_intra > min_intra_level)
        && (pcnt_intra > (intra_vs_inter_thresh * modified_pcnt_inter))
        && ((c.intra_error / double_divide_check(c.coded_error)) < kf_ii_err_threshold)
        && (((p.coded_error - c.coded_error).abs() / double_divide_check(c.coded_error)
          > err_change_threshold)
          || ((p.intra_error - c.intra_error).abs() / double_divide_check(c.intra_error)
            > err_change_threshold)
          || ((f.intra_error / double_divide_check(f.coded_error)) > ii_improvement_threshold))))
  {
    let mut boost_score = 0.0;
    let mut old_boost_score = 0.0;
    let mut decay_accumulator = 1.0;
    for i in 0..16 {
      let lnf: AomFirstPassStats = dict_list[(current_frame_index + 1 + i) as usize];
      let mut next_iiratio = boost_factor * lnf.intra_error / double_divide_check(lnf.coded_error);

      if next_iiratio > kf_ii_max {
        next_iiratio = kf_ii_max
      }
      // Cumulative effect of decay in prediction quality.

      if lnf.pcnt_inter > 0.85 {
        decay_accumulator = decay_accumulator * lnf.pcnt_inter
      } else {
        let decay_accumulator = decay_accumulator * ((0.85 + lnf.pcnt_inter) / 2.0);
      }

      // Keep a running total.
      boost_score = boost_score + decay_accumulator * next_iiratio;

      // Test various breakout clauses.
      if ((lnf.pcnt_inter < 0.05)
        || (next_iiratio < 1.5)
        || (((lnf.pcnt_inter - lnf.pcnt_neutral) < 0.20) && (next_iiratio < 3.0))
        || ((boost_score - old_boost_score) < 3.0)
        || (lnf.intra_error < 200.0))
      {
        break;
      }
      old_boost_score = boost_score;

      // If there is tolerable prediction for at least the next 3 frames then break out else discard this potential key frame && move on
      if boost_score > 30.0 && (i > 3) {
        is_keyframe = true;
      }
    }
  }
  return is_keyframe;
}

pub fn find_aom_keyframes(stat_file: PathBuf, min_keyframe: usize) -> Vec<usize> {
  let stats = read_aomenc_stats_struct(stat_file);
  println!("Stat 10 {:#?}", &stats[10]);

  let mut keyframes: Vec<usize> = vec![];
  let mut frame_count_so_far = 1usize;

  println!("Frame count in stat {}", stats.len() / 232);
  for current_frame_index in 1..(stats.len() / 232) {
    let mut kf = false;

    if frame_count_so_far > min_keyframe {
      kf = test_candidate_kf(
        stats.clone(),
        current_frame_index as u64,
        frame_count_so_far as u64,
      );
    }
    if kf {
      println!("{}", kf);
      keyframes.push(current_frame_index);
    }
    frame_count_so_far = 1;
  }
  keyframes
}
