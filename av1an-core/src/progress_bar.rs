// TODO move this functionality to av1an-cli (and without global variables) when
// Python code is removed. This is only here (and implemented this way) for
// interoperability with Python.

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::error;

use once_cell::sync::Lazy;

const INDICATIF_PROGRESS_TEMPLATE: &str =
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps}, eta {eta})";

static PROGRESS_BAR: Lazy<ProgressBar> = Lazy::new(|| {
  let pb = ProgressBar::hidden();
  pb.set_style(
    ProgressStyle::default_bar()
      .template(INDICATIF_PROGRESS_TEMPLATE)
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
