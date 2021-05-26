use core::f64;

struct AomFirstPassStats {
  frame: u64,                    // Frame number
  weight: u64,                   // Weight assigned to this frame
  intra_error: u64,              // Intra prediction error.
  frame_avg_wavelet_energy: u64, // Average wavelet energy computed using Discrete Wavelet Transform (DWT).
  coded_error: u64,
  sr_coded_error: u64,
  tr_coded_error: u64,
  pcnt_inter: u64,
  pcnt_motion: u64,
  pcnt_second_ref: u64,
  pcnt_third_ref: u64,
  pcnt_neutral: u64,
  intra_skip_pct: u64,
  inactive_zone_rows: u64,
  inactive_zone_cols: u64,
  mvr: u64,
  mvr_abs: u64,
  mvrv: u64,
  mvcv: u64,
  mv_in_out_count: u64,
  new_mv_count: u64,
  duration: u64,
  count: u64,
  raw_error_stdev: u64,
  is_flash: u64,
  noise_var: u64,
  cor_coeff: u64,
}

fn get_second_ref_usage_thresh(frame_count_so_far: u64) -> f64 {
  let adapt_upto = 32.0;
  let min_second_ref_usage_thresh = 0.085;
  let second_ref_usage_thresh_max_delta = 0.035;

  if frame_count_so_far as f64 >= adapt_upto {
    return min_second_ref_usage_thresh + second_ref_usage_thresh_max_delta;
  } else {
    min_second_ref_usage_thresh
      + (frame_count_so_far as f64 / (adapt_upto - 1.0)) * second_ref_usage_thresh_max_delta
  }
}
