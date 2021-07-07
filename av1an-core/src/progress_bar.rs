use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use once_cell::sync::OnceCell;
use std::error;

const INDICATIF_PROGRESS_TEMPLATE: &str =
  "{spinner} [{elapsed_precise}] [{wide_bar}] {percent:>3}% {pos}/{len} ({fps}, eta {eta})";

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

pub fn init_progress_bar(len: u64) -> Result<(), Box<dyn error::Error>> {
  PROGRESS_BAR.get_or_init(|| {
    let bar = ProgressBar::new(len);
    bar.set_style(
      ProgressStyle::default_bar()
        .template(INDICATIF_PROGRESS_TEMPLATE)
        .progress_chars("#>-"),
    );
    bar.enable_steady_tick(100);
    bar
  });

  Ok(())
}

pub fn update_bar(inc: u64) -> Result<(), Box<dyn error::Error>> {
  Ok(
    PROGRESS_BAR
      .get()
      .expect("The progress bar was not initialized!")
      .inc(inc),
  )
}

pub fn finish_progress_bar() -> Result<(), Box<dyn error::Error>> {
  Ok(
    PROGRESS_BAR
      .get()
      .expect("The progress bar was not initialized!")
      .finish(),
  )
}
