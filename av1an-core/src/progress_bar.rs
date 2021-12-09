use crate::get_done;
use crate::Verbosity;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use once_cell::sync::OnceCell;

use crate::util::printable_base10_digits;

const INDICATIF_PROGRESS_TEMPLATE: &str = if cfg!(windows) {
  // Do not use a spinner on Windows since the default console cannot display
  // the characters used for the spinner
  "{elapsed_precise:.bold} [{wide_bar:.blue/white.dim}] {percent:.bold}  {pos} ({fps:.bold}, eta {eta}{msg})"
} else {
  "{spinner:.green.bold} {elapsed_precise:.bold} [{wide_bar:.blue/white.dim}] {percent:.bold}  {pos} ({fps:.bold}, eta {eta}{msg})"
};

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

pub fn get_progress_bar() -> Option<&'static ProgressBar> {
  PROGRESS_BAR.get()
}

fn pretty_progress_style() -> ProgressStyle {
  ProgressStyle::default_bar()
    .template(INDICATIF_PROGRESS_TEMPLATE)
    .with_key("fps", |state| match state.per_sec() {
      fps if fps.abs() < f64::EPSILON => "0 fps".into(),
      fps if fps < 1.0 => format!("{:.2} s/fr", 1.0 / fps),
      fps => format!("{:.2} fps", fps),
    })
    .with_key("pos", |state| format!("{}/{}", state.pos, state.len))
    .with_key("percent", |state| {
      format!("{:>3.0}%", state.fraction() * 100_f32)
    })
    .progress_chars("#>-")
}

/// Initialize progress bar
/// Enables steady 100 ms tick
pub fn init_progress_bar(len: u64) {
  let pb = PROGRESS_BAR.get_or_init(|| ProgressBar::new(len).with_style(pretty_progress_style()));
  pb.set_draw_target(ProgressDrawTarget::stderr());
  pb.enable_steady_tick(100);
  pb.reset();
  pb.reset_eta();
  pb.reset_elapsed();
  pb.set_position(0);
}

pub fn inc_bar(inc: u64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.inc(inc);
  }
}

pub fn dec_bar(dec: u64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.set_position(pb.position().saturating_sub(dec));
  }
}

pub fn update_bar_info(kbps: f64, est_mb: f64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.set_message(format!(", {:.1} Kbps, est. {:.1} MB", kbps, est_mb));
  }
}

pub fn set_pos(pos: u64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.set_position(pos);
  }
}

pub fn finish_progress_bar() {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.finish();
  }
}

static MULTI_PROGRESS_BAR: OnceCell<(MultiProgress, Vec<ProgressBar>)> = OnceCell::new();

pub fn get_first_multi_progress_bar() -> Option<&'static ProgressBar> {
  if let Some((_, pbars)) = MULTI_PROGRESS_BAR.get() {
    pbars.get(0)
  } else {
    None
  }
}

pub fn reset_bar_at(pos: u64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.reset();
    pb.set_position(pos);
    pb.reset_eta();
    pb.reset_elapsed();
  }
}

pub fn reset_mp_bar_at(pos: u64) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    if let Some(pb) = pbs.last() {
      pb.reset();
      pb.set_position(pos);
      pb.reset_eta();
      pb.reset_elapsed();
    }
  }
}

pub fn init_multi_progress_bar(len: u64, workers: usize) {
  MULTI_PROGRESS_BAR.get_or_init(|| {
    let mpb = MultiProgress::new();

    let mut pbs = Vec::new();

    let digits = printable_base10_digits(workers) as usize;

    for i in 1..=workers {
      let pb = ProgressBar::hidden()
        // no spinner on windows, so we remove the prefix to line up with the progress bar
        .with_style(ProgressStyle::default_spinner().template(if cfg!(windows) {
          "{prefix:.dim} {msg}"
        } else {
          "  {prefix:.dim} {msg}"
        }));
      pb.set_prefix(format!("[Worker {:>digits$}]", i, digits = digits));
      pbs.push(mpb.add(pb));
    }

    let pb = ProgressBar::hidden();
    pb.set_style(pretty_progress_style());
    pb.enable_steady_tick(100);
    pb.reset_elapsed();
    pb.reset_eta();
    pb.set_position(0);
    pb.set_length(len);
    pb.reset();
    pbs.push(mpb.add(pb));

    mpb.set_draw_target(ProgressDrawTarget::stderr());

    (mpb, pbs)
  });
}

pub fn update_mp_msg(worker_idx: usize, msg: String) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs[worker_idx].set_message(msg);
  }
}

pub fn inc_mp_bar(inc: u64) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs.last().unwrap().inc(inc);
  }
}

pub fn dec_mp_bar(dec: u64) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    let pb = pbs.last().unwrap();
    pb.set_position(pb.position().saturating_sub(dec));
  }
}

pub fn update_mp_bar_info(kbps: f64, est_mb: f64) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs
      .last()
      .unwrap()
      .set_message(format!(", {:.1} Kbps, est. {:.1} MB", kbps, est_mb));
  }
}

pub fn finish_multi_progress_bar() {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    for pb in pbs.iter() {
      pb.finish();
    }
  }
}

pub fn update_progress_bar_estimates(frame_rate: f64, total_frames: usize, verbosity: Verbosity) {
  let completed_frames: usize = get_done()
    .done
    .iter()
    .map(|ref_multi| ref_multi.value().frames)
    .sum();
  let total_kb: u32 = get_done()
    .done
    .iter()
    .map(|ref_multi| ref_multi.value().size_kb)
    .sum();
  let seconds_completed = completed_frames as f64 / frame_rate;
  let kbps = f64::from(total_kb) * 8. / seconds_completed;
  let progress = completed_frames as f64 / total_frames as f64;
  let est_mb = f64::from(total_kb) / progress / 1000.;
  if verbosity == Verbosity::Normal {
    update_bar_info(kbps, est_mb);
  } else if verbosity == Verbosity::Verbose {
    update_mp_bar_info(kbps, est_mb);
  }
}
