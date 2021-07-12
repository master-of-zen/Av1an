// TODO move this functionality to av1an-cli (and without global variables) when
// Python code is removed. This is only here (and implemented this way) for
// interoperability with Python.

use indicatif::{ProgressBar, ProgressStyle};
use std::error;
use std::sync::Mutex;

use once_cell::sync::Lazy;

const INDICATIF_PROGRESS_TEMPLATE: &str =
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps}, eta {eta})";

static PROGRESS_BAR: Lazy<Mutex<ProgressBar>> = Lazy::new(|| Mutex::new(ProgressBar::new(0)));

pub fn init_progress_bar(len: u64) -> Result<(), Box<dyn error::Error>> {
  let pb = PROGRESS_BAR.lock().unwrap();

  pb.reset_elapsed();
  pb.reset_eta();
  pb.set_position(0);
  pb.set_length(len);
  pb.reset();
  pb.set_style(
    ProgressStyle::default_bar()
      .template(INDICATIF_PROGRESS_TEMPLATE)
      .progress_chars("#>-"),
  );
  pb.enable_steady_tick(100);

  Ok(())
}

pub fn update_bar(inc: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.lock().unwrap().inc(inc);
  Ok(())
}

pub fn set_pos(pos: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.lock().unwrap().set_position(pos);
  Ok(())
}

pub fn finish_progress_bar() -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.lock().unwrap().finish();
  Ok(())
}
