use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, available_parallelism};

use crate::broker::EncoderCrash;
use crate::context::Av1anContext;
use crate::progress_bar::{dec_bar, update_progress_bar_estimates};
use crate::util::printable_base10_digits;
use crate::{get_done, Chunk, DoneChunk, Instant};

/// Worker spawns a thread that process a chunk,
/// holds processing status,
/// signals back it's status.
pub struct Worker {
  sender: mpsc::Sender<String>,
  chunk: Chunk,
  worker_id: usize,
  handle: Option<thread::JoinHandle<()>>,
  ignore_frame_mismatch: bool,
  context: Av1anContext,
}

impl Worker {
  fn new(
    chunk: Chunk,
    context: Av1anContext,
    ignore_frame_mismatch: bool,
    worker_id: usize,
  ) -> Worker {
    let (sender, _) = mpsc::channel();

    Worker {
      sender,
      chunk,
      handle: None,
      ignore_frame_mismatch,
      context,
      worker_id,
    }
  }

  fn encode_chunk(&self) -> Result<(), Box<EncoderCrash>> {
    let st_time = Instant::now();

    if let Some(ref tq) = self.context.args.target_quality {
      tq.per_shot_target_quality_routine(&mut self.chunk).unwrap();
    }

    //TODO: Move all the messaging to broker level
    // space padding at the beginning to align with "finished chunk"
    debug!(
      " started chunk {:05}: {} frames",
      self.chunk.index,
      self.chunk.frames()
    );

    // we display the index, so we need to subtract 1 to get the max index
    let padding = printable_base10_digits(self.chunk_queue.len() - 1) as usize;

    let passes = self.chunk.passes;
    for current_pass in 1..=passes {
      for r#try in 1..=self.context.args.max_tries {
        let res = self.context.create_pipes(
          &self.chunk,
          current_pass,
          self.worker_id,
          padding,
          self.ignore_frame_mismatch,
        );
        if let Err((e, frames)) = res {
          dec_bar(frames);

          if r#try == self.context.args.max_tries {
            error!(
              "[chunk {}] encoder failed {} times, shutting down worker",
              self.chunk.index, self.context.args.max_tries
            );
            return Err(e);
          }
          // avoids double-print of the error message as both a WARN and ERROR,
          // since `Broker::encoding_loop` will print the error message as well
          warn!("Encoder failed (on chunk {}):\n{}", self.chunk.index, e);
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
      println!("Processing");

      // encode chunk
      self.encode_chunk();

      // Send a response back to the main thread
      sender.send(format!("Finished")).unwrap();
    });

    self.handle = Some(handle);
  }

  fn join(self) {
    // Wait for the worker thread to finish
    if let Some(handle) = self.handle {
      handle.join().unwrap();
    }
  }
}
