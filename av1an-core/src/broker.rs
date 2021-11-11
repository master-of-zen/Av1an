use crate::{
  ffmpeg, finish_multi_progress_bar, finish_progress_bar, get_done, settings::EncodeArgs, Chunk,
  Instant, TargetQuality, Verbosity,
};
use std::{
  fmt::{Debug, Display},
  fs::File,
  io::Write,
  path::Path,
  process::ExitStatus,
  sync::mpsc::Sender,
};

use thiserror::Error;

pub struct Broker<'a> {
  pub chunk_queue: Vec<Chunk>,
  pub project: &'a EncodeArgs,
  pub target_quality: Option<TargetQuality<'a>>,
}

#[derive(Clone)]
pub enum StringOrBytes {
  String(String),
  Bytes(Vec<u8>),
}

impl Debug for StringOrBytes {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::String(s) => {
        if f.alternate() {
          f.write_str(&textwrap::indent(&s, /* 8 spaces */ "        "))?;
        } else {
          f.write_str(s)?;
        }
      }
      Self::Bytes(b) => write!(f, "raw bytes: {:?}", b)?,
    }

    Ok(())
  }
}

impl From<Vec<u8>> for StringOrBytes {
  fn from(bytes: Vec<u8>) -> Self {
    if simdutf8::basic::from_utf8(&bytes).is_ok() {
      // SAFETY: this branch guarantees that the input is valid UTF8
      Self::String(unsafe { String::from_utf8_unchecked(bytes) })
    } else {
      Self::Bytes(bytes)
    }
  }
}

impl From<String> for StringOrBytes {
  fn from(s: String) -> Self {
    Self::String(s)
  }
}

#[derive(Error, Debug)]
pub struct EncoderCrash {
  pub exit_status: ExitStatus,
  pub stdout: StringOrBytes,
  pub stderr: StringOrBytes,
  pub pipe_stderr: StringOrBytes,
}

impl Display for EncoderCrash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "encoder crashed: {}\nstdout:\n{:#?}\nstderr:\n{:#?}\nsource pipe stderr:\n{:#?}",
      self.exit_status, self.stdout, self.stderr, self.pipe_stderr
    )?;
    Ok(())
  }
}

impl<'a> Broker<'a> {
  pub fn new(
    chunk_queue: Vec<Chunk>,
    project: &'a EncodeArgs,
    target_quality: Option<TargetQuality<'a>>,
  ) -> Self {
    Broker {
      chunk_queue,
      project,
      target_quality,
    }
  }

  #[allow(clippy::needless_pass_by_value)]
  pub fn encoding_loop(self, tx: Sender<()>) {
    if !self.chunk_queue.is_empty() {
      let (sender, receiver) = crossbeam_channel::bounded(self.chunk_queue.len());

      for chunk in &self.chunk_queue {
        sender.send(chunk.clone()).unwrap();
      }
      drop(sender);

      crossbeam_utils::thread::scope(|s| {
        let consumers: Vec<_> = (0..self.project.workers)
          .map(|i| (receiver.clone(), &self, i))
          .map(|(rx, queue, consumer_idx)| {
            let tx = tx.clone();
            s.spawn(move |_| {
              while let Ok(mut chunk) = rx.recv() {
                if let Err(e) = queue.encode_chunk(&mut chunk, consumer_idx) {
                  error!("[chunk {}] {}", chunk.index, e);

                  tx.send(()).unwrap();
                  return Err(());
                }
              }
              Ok(())
            })
          })
          .collect();
        for consumer in consumers {
          let _ = consumer.join().unwrap();
        }
      })
      .unwrap();

      if self.project.verbosity == Verbosity::Normal {
        finish_progress_bar();
      } else if self.project.verbosity == Verbosity::Verbose {
        finish_multi_progress_bar();
      }
    }
  }

  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), EncoderCrash> {
    let st_time = Instant::now();

    info!(
      "encoding started for chunk {} ({} frames)",
      chunk.index, chunk.frames
    );

    if let Some(ref tq) = self.target_quality {
      tq.per_shot_target_quality_routine(chunk)?;
    }

    // Run all passes for this chunk
    const MAX_TRIES: usize = 3;
    for current_pass in 1..=self.project.passes {
      for r#try in 1..=MAX_TRIES {
        let res = self.project.create_pipes(chunk, current_pass, worker_id);
        if let Err(e) = res {
          if r#try == MAX_TRIES {
            error!(
              "[chunk {}] encoder crashed {} times, shutting down worker",
              chunk.index, MAX_TRIES
            );
            return Err(e);
          }
          // avoids double-print of the error message as both a WARN and ERROR,
          // since `Broker::encoding_loop` will print the error message as well
          warn!("Encoder failed (on chunk {}):\n{}", chunk.index, e);
        } else {
          break;
        }
      }
    }

    let encoded_frames = Self::frame_check_output(chunk, chunk.frames);

    if encoded_frames == chunk.frames {
      let progress_file = Path::new(&self.project.temp).join("done.json");
      get_done().done.insert(chunk.name(), encoded_frames);

      let mut progress_file = File::create(&progress_file).unwrap();
      progress_file
        .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
        .unwrap();

      let enc_time = st_time.elapsed();

      info!(
        "Done: {} Fr: {}/{}",
        chunk.index, encoded_frames, chunk.frames
      );
      info!(
        "Fps: {:.2} Time: {:?}",
        encoded_frames as f64 / enc_time.as_secs_f64(),
        enc_time
      );
    }

    Ok(())
  }

  fn frame_check_output(chunk: &Chunk, expected_frames: usize) -> usize {
    let actual_frames = ffmpeg::num_frames(chunk.output().as_ref()).unwrap();

    if actual_frames != expected_frames {
      warn!(
        "FRAME MISMATCH: chunk {}: {}/{} (actual/expected frames)",
        chunk.index, actual_frames, expected_frames
      );
    }

    actual_frames
  }
}
