use crate::{
  finish_multi_progress_bar, finish_progress_bar, frame_probe, get_done, project::Project, Chunk,
  Instant, TargetQuality, VecDeque, Verbosity,
};
use itertools::Itertools;
use std::{fs::File, io::Write, path::Path, sync::mpsc::Sender};

pub struct Broker<'a> {
  pub chunk_queue: Vec<Chunk>,
  pub project: &'a Project,
  pub target_quality: Option<TargetQuality<'a>>,
}

impl<'a> Broker<'a> {
  pub fn new(
    chunk_queue: Vec<Chunk>,
    project: &'a Project,
    target_quality: Option<TargetQuality<'a>>,
  ) -> Self {
    Broker {
      chunk_queue,
      project,
      target_quality,
    }
  }

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
                if queue.encode_chunk(&mut chunk, consumer_idx).is_err() {
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

  fn encode_chunk(&self, chunk: &mut Chunk, worker_id: usize) -> Result<(), VecDeque<String>> {
    let st_time = Instant::now();

    info!("Enc: {}, {} fr", chunk.index, chunk.frames);

    // TODO change logic if other target quality methods are added in the future
    if let Some(ref tq) = self.target_quality {
      tq.per_shot_target_quality_routine(chunk);
    }

    // Run all passes for this chunk
    const MAX_TRIES: usize = 3;
    for current_pass in 1..=self.project.passes {
      for r#try in 1..=MAX_TRIES {
        let res = self.project.create_pipes(chunk, current_pass, worker_id);
        if let Err((exit_status, output)) = res {
          warn!(
            "Encoder failed (on chunk {}) with {}:\n{}",
            chunk.index,
            exit_status,
            textwrap::indent(&output.iter().join("\n"), /* 8 spaces */ "        ")
          );
          if r#try == MAX_TRIES {
            error!(
              "Encoder crashed (on chunk {}) {} times, terminating thread",
              chunk.index, MAX_TRIES
            );
            return Err(output);
          }
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
    let actual_frames = frame_probe(&chunk.output_path());

    if actual_frames != expected_frames {
      warn!(
        "FRAME MISMATCH: Chunk #{}: {}/{} fr",
        chunk.index, actual_frames, expected_frames
      );
    }

    actual_frames
  }
}
