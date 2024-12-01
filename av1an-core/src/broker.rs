use std::fmt::{Debug, Display};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::mpsc::Sender;
use std::thread::available_parallelism;

use cfg_if::cfg_if;
use smallvec::SmallVec;
use thiserror::Error;
use tracing::{debug, error, warn};

use crate::context::Av1anContext;
use crate::progress_bar::{dec_bar, update_progress_bar_estimates};
use crate::util::printable_base10_digits;
use crate::{finish_progress_bar, get_done, Chunk, DoneChunk, Instant};

#[derive(Debug)]
pub struct Broker<'a> {
  pub chunk_queue: Vec<Chunk>,
  pub project: &'a Av1anContext,
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
          f.write_str(&textwrap::indent(s, /* 8 spaces */ "        "))?;
        } else {
          f.write_str(s)?;
        }
      }
      Self::Bytes(b) => write!(f, "raw bytes: {b:?}")?,
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

impl StringOrBytes {
  pub fn as_bytes(&self) -> &[u8] {
    match self {
      Self::String(s) => s.as_bytes(),
      Self::Bytes(b) => b,
    }
  }
}

#[derive(Error, Debug)]
pub struct EncoderCrash {
  pub exit_status: ExitStatus,
  pub stdout: StringOrBytes,
  pub stderr: StringOrBytes,
  pub source_pipe_stderr: StringOrBytes,
  pub ffmpeg_pipe_stderr: Option<StringOrBytes>,
}

impl Display for EncoderCrash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "encoder crashed: {}\nstdout:\n{:#?}\nstderr:\n{:#?}\nsource pipe stderr:\n{:#?}",
      self.exit_status, self.stdout, self.stderr, self.source_pipe_stderr,
    )?;

    if let Some(ffmpeg_pipe_stderr) = &self.ffmpeg_pipe_stderr {
      write!(f, "\nffmpeg pipe stderr:\n{ffmpeg_pipe_stderr:#?}")?;
    }

    Ok(())
  }
}

impl Broker<'_> {
  /// Main encoding loop. set_thread_affinity may be ignored if the value is invalid.
  #[tracing::instrument(skip(self))]
  pub fn encoding_loop(self, tx: Sender<()>, set_thread_affinity: Option<usize>) {
    if !self.chunk_queue.is_empty() {
      let (sender, receiver) = crossbeam_channel::bounded(self.chunk_queue.len());

      for chunk in &self.chunk_queue {
        sender.send(chunk.clone()).unwrap();
      }
      drop(sender);

      crossbeam_utils::thread::scope(|s| {
        let consumers: Vec<_> = (0..self.project.args.workers)
          .map(|idx| (receiver.clone(), &self, idx))
          .map(|(rx, queue, worker_id)| {
            let tx = tx.clone();
            s.spawn(move |_| {
              cfg_if! {
                if #[cfg(any(target_os = "linux", target_os = "windows"))] {
                  if let Some(threads) = set_thread_affinity {
                    if threads == 0 {
                      warn!("Ignoring set_thread_affinity: Requested 0 threads");
                    } else {
                      match available_parallelism() {
                        Ok(parallelism) => {
                          let available_threads = parallelism.get();
                          let mut cpu_set = SmallVec::<[usize; 16]>::new();
                          let start_thread = (threads * worker_id) % available_threads;
                          cpu_set.extend((start_thread..start_thread + threads).map(|t| t % available_threads));
                          if let Err(e) = affinity::set_thread_affinity(&cpu_set) {
                            warn!(
                              "Failed to set thread affinity for worker {}: {}",
                              worker_id, e
                            );
                          }
                        },
                        Err(e) => {
                          warn!("Failed to get thread count: {}. Thread affinity will not be set", e);
                        }
                      }
                    }
                  }
                }
              }

              while let Ok(mut chunk) = rx.recv() {
                if let Err(e) = queue.encode_chunk(&mut chunk, worker_id) {
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
          consumer.join().unwrap().ok();
        }
      })
      .unwrap();

      finish_progress_bar();
    }
  }

  #[tracing::instrument(skip(self))]
  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), Box<EncoderCrash>> {
    let st_time = Instant::now();

    if let Some(ref tq) = self.project.args.target_quality {
      tq.per_shot_target_quality_routine(chunk).unwrap();
    }

    // space padding at the beginning to align with "finished chunk"
    debug!(
      " started chunk {:05}: {} frames",
      chunk.index,
      chunk.frames()
    );

    // we display the index, so we need to subtract 1 to get the max index
    let padding = printable_base10_digits(self.chunk_queue.len() - 1) as usize;

    let passes = chunk.passes;
    for current_pass in 1..=passes {
      for r#try in 1..=self.project.args.max_tries {
        let res = self
          .project
          .create_pipes(chunk, current_pass, worker_id, padding);
        if let Err((e, frames)) = res {
          dec_bar(frames);

          if r#try == self.project.args.max_tries {
            error!(
              "[chunk {}] encoder failed {} times, shutting down worker",
              chunk.index, self.project.args.max_tries
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

    let enc_time = st_time.elapsed();
    let fps = chunk.frames() as f64 / enc_time.as_secs_f64();

    let progress_file = Path::new(&self.project.args.temp).join("done.json");
    get_done().done.insert(
      chunk.name(),
      DoneChunk {
        frames: chunk.frames(),
        size_bytes: Path::new(&chunk.output())
          .metadata()
          .expect("Unable to get size of finished chunk")
          .len(),
      },
    );

    let mut progress_file = File::create(progress_file).unwrap();
    progress_file
      .write_all(serde_json::to_string(get_done()).unwrap().as_bytes())
      .unwrap();

    update_progress_bar_estimates(
      chunk.frame_rate,
      self.project.frames,
      self.project.args.verbosity,
    );

    debug!(
      "finished chunk {:05}: {} frames, {:.2} fps, took {:.2?}",
      chunk.index,
      chunk.frames(),
      fps,
      enc_time
    );

    Ok(())
  }
}
