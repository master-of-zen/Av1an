// TODO move this functionality to av1an-cli (and without global variables) when
// Python code is removed. This is only here (and implemented this way) for
// interoperability with Python.

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::error;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

const INDICATIF_PROGRESS_TEMPLATE: &str = if cfg!(target_os = "windows") {
  // Do not use a spinner on Windows since the default console cannot display
  // the characters used for the spinner
  "[{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps} fps, eta {eta})"
} else {
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps} fps, eta {eta})"
};

static PROGRESS_BAR: Lazy<ProgressBar> = Lazy::new(|| {
  let pb = ProgressBar::hidden();
  pb.set_style(
    ProgressStyle::default_bar()
      .template(INDICATIF_PROGRESS_TEMPLATE)
      .with_key("fps", |state| format!("{:.2}", state.per_sec()))
      .progress_chars("#>-"),
  );
  pb.set_draw_target(ProgressDrawTarget::stderr());

  pb
});

pub fn init_progress_bar(len: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.enable_steady_tick(100);
  PROGRESS_BAR.reset_elapsed();
  PROGRESS_BAR.reset_eta();
  PROGRESS_BAR.set_position(0);
  PROGRESS_BAR.set_length(len);
  PROGRESS_BAR.reset();

  Ok(())
}

pub fn update_bar(inc: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.inc(inc);
  Ok(())
}

pub fn set_pos(pos: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.set_position(pos);
  Ok(())
}

pub fn finish_progress_bar() -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.finish();
  Ok(())
}

static MULTI_PROGRESS_BAR: Lazy<(MultiProgress, Mutex<Vec<ProgressBar>>)> = Lazy::new(|| {
  let pb = MultiProgress::new();
  pb.set_draw_target(ProgressDrawTarget::stderr());

  (pb, Mutex::new(Vec::new()))
});

pub fn init_multi_progress_bar(len: u64, workers: usize) -> Result<(), Box<dyn error::Error>> {
  let mut pbs = MULTI_PROGRESS_BAR.1.lock();

  for i in 0..workers {
    let pb = ProgressBar::hidden()
      .with_style(ProgressStyle::default_spinner().template("[{prefix}] {msg}"));
    pb.set_prefix(format!("Worker {:02}", i + 1));
    pbs.push(MULTI_PROGRESS_BAR.0.add(pb));
  }

  let pb = ProgressBar::hidden();
  pb.set_style(
    ProgressStyle::default_bar()
      .template(INDICATIF_PROGRESS_TEMPLATE)
      .with_key("fps", |state| format!("{:.2}", state.per_sec()))
      .progress_chars("#>-"),
  );
  pb.enable_steady_tick(100);
  pb.reset_elapsed();
  pb.reset_eta();
  pb.set_position(0);
  pb.set_length(len);
  pb.reset();
  pbs.push(MULTI_PROGRESS_BAR.0.add(pb));

  MULTI_PROGRESS_BAR
    .0
    .set_draw_target(ProgressDrawTarget::stderr());

  Ok(())
}

pub fn update_mp_msg(worker_idx: usize, msg: String) -> Result<(), Box<dyn error::Error>> {
  let pbs = MULTI_PROGRESS_BAR.1.lock();
  pbs[worker_idx].set_message(msg);
  Ok(())
}

pub fn update_mp_bar(inc: u64) -> Result<(), Box<dyn error::Error>> {
  let pbs = MULTI_PROGRESS_BAR.1.lock();
  pbs.last().unwrap().inc(inc);
  Ok(())
}

pub fn finish_multi_progress_bar() -> Result<(), Box<dyn error::Error>> {
  let pbs = MULTI_PROGRESS_BAR.1.lock();
  for pb in pbs.iter() {
    pb.finish();
  }
  Ok(())
}
