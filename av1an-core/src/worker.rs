use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, available_parallelism};

use cfg_if::cfg_if;
use smallvec::SmallVec;
use thiserror::Error;

use crate::broker::EncoderCrash;
use crate::context::Av1anContext;
use crate::progress_bar::{dec_bar, update_mp_chunk, update_progress_bar_estimates};
use crate::{finish_progress_bar, get_done, Chunk, DoneChunk, Instant};

/// Worker spawns a thread that process a chunk,
/// holds processing status,
/// signals back it's status.
pub struct Worker {
  sender: mpsc::Sender<WorkerMessage>,
  chunk: Chunk,
  worker_id: usize,
  handle: Option<thread::JoinHandle<()>>,
  context: Av1anContext,
}

pub enum WorkerMessage {
  /// Started chunk x with y frames
  StartedChunk(usize, usize),

  StartedTargetQuality,

  FinishedTargetQuality,

  StartedEncode,

  WorkerFailedAndRestarting(usize, usize),

  WorkerFailedAndShuttingDown(usize),

  FinishedEncode,
}

impl Worker {
  fn new(chunk: Chunk, context: Av1anContext, worker_id: usize) -> Worker {
    let (sender, _) = mpsc::channel();

    Worker {
      sender,
      chunk,
      handle: None,
      context,
      worker_id,
    }
  }

  fn encode_chunk(&self) -> Result<(), Box<EncoderCrash>> {
    let st_time = Instant::now();

    //TODO: Move all the messaging to broker level
    self
      .sender
      .send(WorkerMessage::StartedChunk(
        self.chunk.index,
        self.chunk.frames(),
      ))
      .unwrap();

    let passes = self.chunk.passes;
    for current_pass in 1..=passes {
      for r#try in 1..=self.context.args.max_tries {
        let res = self
          .context
          .create_pipes(&self.chunk, current_pass, self.worker_id);

        if let Err((e, frames)) = res {
          dec_bar(frames);

          if r#try == self.context.args.max_tries {
            self
              .sender
              .send(WorkerMessage::WorkerFailedAndShuttingDown(
                self.context.args.max_tries,
              ))
              .unwrap();

            return Err(e);
          }
        } else {
          break;
        }
      }
    }

    let enc_time = st_time.elapsed();
    let fps = self.chunk.frames() as f64 / enc_time.as_secs_f64();

    // Move being done to broker
    let progress_file = Path::new(&self.context.args.temp).join("done.json");
    get_done().done.insert(
      self.chunk.name(),
      DoneChunk {
        frames: self.chunk.frames(),
        size_bytes: Path::new(&self.chunk.output())
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
      self.chunk.frame_rate,
      self.context.frames,
      self.context.args.verbosity,
    );

    debug!(
      "finished chunk {:05}: {} frames, {:.2} fps, took {:.2?}",
      self.chunk.index,
      self.chunk.frames(),
      fps,
      enc_time
    );

    Ok(())
  }

  fn start(mut self) {
    let sender = self.sender.clone();
    let handle = thread::spawn(move || {
      if let Some(ref tq) = &self.context.args.target_quality {
        sender.send(WorkerMessage::StartedTargetQuality).unwrap();
        tq.per_shot_target_quality_routine(&mut self.chunk).unwrap();
        sender.send(WorkerMessage::FinishedTargetQuality).unwrap();

        self.encode_chunk().unwrap();
      }

      // Send a response back to the main thread
      sender.send(WorkerMessage::FinishedEncode).unwrap();
    });
    let _ = handle.join();
  }
}
