use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

const INDICATIF_PROGRESS_TEMPLATE: &str = if cfg!(target_os = "windows") {
  // Do not use a spinner on Windows since the default console cannot display
  // the characters used for the spinner
  "[{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps} fps, eta {eta})"
} else {
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps} fps, eta {eta})"
};

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

pub fn init_progress_bar(len: u64) {
  let pb = PROGRESS_BAR.get_or_init(|| {
    ProgressBar::new(len).with_style(
      ProgressStyle::default_bar()
        .template(INDICATIF_PROGRESS_TEMPLATE)
        .with_key("fps", |state| format!("{:.2}", state.per_sec()))
        .progress_chars("#>-"),
    )
  });
  pb.enable_steady_tick(100);
  pb.reset();
  pb.reset_eta();
  pb.reset_elapsed();
  pb.set_position(0);
}

pub fn update_bar(inc: u64) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.inc(inc);
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

static MULTI_PROGRESS_BAR: OnceCell<(MultiProgress, Mutex<Vec<ProgressBar>>)> = OnceCell::new();

pub fn init_multi_progress_bar(len: u64, workers: usize) {
  let multi_progress_bar = MULTI_PROGRESS_BAR.get_or_init(|| {
    let pb = MultiProgress::new();

    (pb, Mutex::new(Vec::new()))
  });

  let mut pbs = multi_progress_bar.1.lock();

  for i in 0..workers {
    let pb = ProgressBar::hidden()
      .with_style(ProgressStyle::default_spinner().template("[{prefix}] {msg}"));
    pb.set_prefix(format!("Worker {:02}", i + 1));
    pbs.push(multi_progress_bar.0.add(pb));
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
  pbs.push(multi_progress_bar.0.add(pb));

  multi_progress_bar
    .0
    .set_draw_target(ProgressDrawTarget::stderr());
}

pub fn update_mp_msg(worker_idx: usize, msg: String) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs.lock()[worker_idx].set_message(msg);
  }
}

pub fn update_mp_bar(inc: u64) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs.lock().last().unwrap().inc(inc);
  }
}

pub fn finish_multi_progress_bar() {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    for pb in pbs.lock().iter() {
      pb.finish();
    }
  }
}
