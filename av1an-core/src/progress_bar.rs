use std::fmt::Write;
use std::time::Duration;

use indicatif::{
  HumanBytes, HumanDuration, MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState,
  ProgressStyle,
};
use once_cell::sync::OnceCell;

use crate::util::printable_base10_digits;
use crate::{get_done, Verbosity};

const PROGRESS_CHARS: &str = "█▉▊▋▌▍▎▏  ";

const INDICATIF_PROGRESS_TEMPLATE: &str = if cfg!(windows) {
  // Do not use a spinner on Windows since the default console cannot display
  // the characters used for the spinner
  "{elapsed_precise:.bold} ▕{wide_bar:.blue/white.dim}▏ {percent:.bold}  {pos} ({fps:.bold}, eta {fixed_eta}{msg})"
} else {
  "{spinner:.green.bold} {elapsed_precise:.bold} ▕{wide_bar:.blue/white.dim}▏ {percent:.bold}  {pos} ({fps:.bold}, eta {fixed_eta}{msg})"
};

const INDICATIF_SPINNER_TEMPLATE: &str = if cfg!(windows) {
  // Do not use a spinner on Windows since the default console cannot display
  // the characters used for the spinner
  "{elapsed_precise:.bold} [{wide_bar:.blue/white.dim}]  {pos} frames ({fps:.bold})"
} else {
  "{spinner:.green.bold} {elapsed_precise:.bold} [{wide_bar:.blue/white.dim}]  {pos} frames ({fps:.bold})"
};

static PROGRESS_BAR: OnceCell<ProgressBar> = OnceCell::new();

pub fn get_progress_bar() -> Option<&'static ProgressBar> {
  PROGRESS_BAR.get()
}

fn pretty_progress_style() -> ProgressStyle {
  ProgressStyle::default_bar()
    .template(INDICATIF_PROGRESS_TEMPLATE)
    .unwrap()
    .with_key("fps", |state: &ProgressState, w: &mut dyn Write| {
      if state.pos() == 0 || state.elapsed().as_secs_f32() < f32::EPSILON {
        write!(w, "0 fps").unwrap();
      } else {
        let fps = state.pos() as f32 / state.elapsed().as_secs_f32();
        if fps < 1.0 {
          write!(w, "{:.2} s/fr", 1.0 / fps).unwrap();
        } else {
          write!(w, "{:.2} fps", fps).unwrap();
        }
      }
    })
    .with_key("fixed_eta", |state: &ProgressState, w: &mut dyn Write| {
      if state.pos() == 0 || state.elapsed().as_secs_f32() < f32::EPSILON {
        write!(w, "unknown").unwrap();
      } else {
        let spf = state.elapsed().as_secs_f32() / state.pos() as f32;
        let remaining = state.len().unwrap_or(0) - state.pos();
        write!(
          w,
          "{:#}",
          HumanDuration(Duration::from_secs_f32(spf * remaining as f32))
        )
        .unwrap();
      }
    })
    .with_key("pos", |state: &ProgressState, w: &mut dyn Write| {
      write!(w, "{}/{}", state.pos(), state.len().unwrap_or(0)).unwrap();
    })
    .with_key("percent", |state: &ProgressState, w: &mut dyn Write| {
      write!(w, "{:>3.0}%", state.fraction() * 100_f32).unwrap();
    })
    .progress_chars(PROGRESS_CHARS)
}

fn spinner_style() -> ProgressStyle {
  ProgressStyle::default_spinner()
    .template(INDICATIF_SPINNER_TEMPLATE)
    .unwrap()
    .with_key("fps", |state: &ProgressState, w: &mut dyn Write| {
      if state.pos() == 0 || state.elapsed().as_secs_f32() < f32::EPSILON {
        write!(w, "0 fps").unwrap();
      } else {
        let fps = state.pos() as f32 / state.elapsed().as_secs_f32();
        if fps < 1.0 {
          write!(w, "{:.2} s/fr", 1.0 / fps).unwrap();
        } else {
          write!(w, "{:.2} fps", fps).unwrap();
        }
      }
    })
    .with_key("pos", |state: &ProgressState, w: &mut dyn Write| {
      write!(w, "{}", state.pos()).unwrap();
    })
    .progress_chars(PROGRESS_CHARS)
}

/// Initialize progress bar
/// Enables steady 100 ms tick
pub fn init_progress_bar(len: u64) {
  let pb = if len > 0 {
    PROGRESS_BAR.get_or_init(|| ProgressBar::new(len).with_style(pretty_progress_style()))
  } else {
    // Avoid showing `xxx/0` if we don't know the length yet.
    // Affects scenechange progress.
    PROGRESS_BAR.get_or_init(|| ProgressBar::new(len).with_style(spinner_style()))
  };
  pb.set_draw_target(ProgressDrawTarget::stderr());
  pb.enable_steady_tick(Duration::from_millis(100));
  pb.reset();
  pb.reset_eta();
  pb.reset_elapsed();
  pb.set_position(0);
}

pub fn convert_to_progress() {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.set_style(pretty_progress_style());
  }
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

pub fn update_bar_info(kbps: f64, est_size: HumanBytes) {
  if let Some(pb) = PROGRESS_BAR.get() {
    pb.set_message(format!(", {:.1} Kbps, est. {}", kbps, est_size));
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

pub fn set_len(len: u64) {
  let pb = PROGRESS_BAR.get().unwrap();
  pb.set_length(len);
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

pub fn init_multi_progress_bar(len: u64, workers: usize, total_chunks: usize) {
  MULTI_PROGRESS_BAR.get_or_init(|| {
    let mpb = MultiProgress::new();

    let mut pbs = Vec::new();

    let digits = printable_base10_digits(total_chunks) as usize;

    for _ in 1..=workers {
      let pb = ProgressBar::hidden()
        // no spinner on windows, so we remove the prefix to line up with the progress bar
        .with_style(
          ProgressStyle::default_spinner()
            .template(if cfg!(windows) {
              "{prefix:.dim} {msg}"
            } else {
              "  {prefix:.dim} {msg}"
            })
            .unwrap(),
        );
      pb.set_prefix(format!("[Idle  {width:width$}]", width = digits));
      pbs.push(mpb.add(pb));
    }

    let pb = ProgressBar::hidden();
    pb.set_style(pretty_progress_style());
    pb.enable_steady_tick(Duration::from_millis(100));
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

pub fn update_mp_chunk(worker_idx: usize, chunk: usize, padding: usize) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs[worker_idx].set_prefix(format!("[Chunk {:>digits$}]", chunk, digits = padding));
  }
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

pub fn update_mp_bar_info(kbps: f64, est_size: HumanBytes) {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    pbs
      .last()
      .unwrap()
      .set_message(format!(", {:.1} Kbps, est. {}", kbps, est_size));
  }
}

pub fn finish_multi_progress_bar() {
  if let Some((_, pbs)) = MULTI_PROGRESS_BAR.get() {
    for pb in pbs.iter() {
      pb.finish();
    }
  }
}

pub fn update_progress_bar_estimates(
  frame_rate: f64,
  total_frames: usize,
  verbosity: Verbosity,
  audio_size: u64,
) {
  let completed_frames: usize = get_done()
    .done
    .iter()
    .map(|ref_multi| ref_multi.value().frames)
    .sum();
  let total_size: u64 = get_done()
    .done
    .iter()
    .map(|ref_multi| ref_multi.value().size_bytes)
    .sum::<u64>();
  let seconds_completed = completed_frames as f64 / frame_rate;
  let kbps = total_size as f64 * 8. / 1000. / seconds_completed;
  let progress = completed_frames as f64 / total_frames as f64;
  let est_size = total_size as f64 / progress + audio_size as f64;
  if verbosity == Verbosity::Normal {
    update_bar_info(kbps, HumanBytes(est_size as u64));
  } else if verbosity == Verbosity::Verbose {
    update_mp_bar_info(kbps, HumanBytes(est_size as u64));
  }
}
