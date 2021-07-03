import concurrent
import concurrent.futures
import json
import sys
import time
import traceback

from av1an.chunk import Chunk
from av1an.target_quality import TargetQuality
from av1an.utils import frame_probe
from av1an_pyo3 import log

from .Pipes import create_pipes


class Queue:
    def __init__(self, project, chunk_queue):
        self.chunk_queue = chunk_queue
        self.queue = []
        self.project = project
        self.thread_executor = concurrent.futures.ThreadPoolExecutor()
        self.status = "Ok"
        self.tq = TargetQuality(project) if project.target_quality else None

    def encoding_loop(self):
        if len(self.chunk_queue) != 0:
            with concurrent.futures.ThreadPoolExecutor(
                max_workers=self.project.workers
            ) as executor:
                future_cmd = {
                    executor.submit(self.encode_chunk, cmd): cmd
                    for cmd in self.chunk_queue
                }
                for future in concurrent.futures.as_completed(future_cmd):
                    try:
                        future.result()
                    except Exception as exc:
                        _, _, exc_tb = sys.exc_info()
                        print(f"Encoding error {exc}\nAt line {exc_tb.tb_lineno}")
                        sys.exit(1)
        self.project.counter.close()

    def encode_chunk(self, chunk: Chunk):
        restart_count = 0

        while restart_count < 3:
            try:
                st_time = time.time()

                chunk_frames = chunk.frames

                log(f"Enc: {chunk.index}, {chunk_frames} fr")

                # Target Quality Mode
                if self.project.target_quality:
                    if self.project.target_quality_method == "per_shot":
                        self.tq.per_shot_target_quality_routine(chunk)

                # Run all passes for this chunk
                for current_pass in range(1, self.project.passes + 1):
                    create_pipes(
                        self.project,
                        chunk,
                        self.project.encoder,
                        self.project.counter,
                        chunk_frames,
                        self.project.passes,
                        current_pass,
                    )

                # get the number of encoded frames, if no check assume it worked and encoded same number of frames
                encoded_frames = self.frame_check_output(chunk, chunk_frames)

                # write this chunk as done if it encoded correctly
                if encoded_frames == chunk_frames:
                    progress_file = self.project.temp / "done.json"
                    with progress_file.open() as f:
                        d = json.load(f)
                        d["done"][chunk.name] = encoded_frames
                        with progress_file.open("w") as f:
                            json.dump(d, f)

                enc_time = round(time.time() - st_time, 2)
                log(f"Done: {chunk.index} Fr: {encoded_frames}/{chunk_frames}")
                log(f"Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.")
                return

            except Exception as e:
                msg1, msg2, msg3 = (
                    f"Chunk #{chunk.index} crashed",
                    f"Exception: {type(e)} {e}",
                    "Restarting chunk",
                )
                log(msg1)
                log(msg2)
                log(msg3)
                print(f"{msg1}\n::{msg2}\n::{msg3}")
                traceback.print_exc()
                restart_count += 1

        msg1, msg2 = (
            "FATAL",
            f"Chunk #{chunk.index} failed more than 3 times, shutting down thread",
        )
        log(msg1)
        log(msg2)

        print(f"::{msg1}\n::{msg2}")
        self.status = "FATAL"

    def frame_check_output(
        self, chunk: Chunk, expected_frames: int, last_chunk=False
    ) -> int:
        actual_frames = frame_probe(chunk.output_path)
        if actual_frames != expected_frames:
            msg = f"Chunk #{chunk.index}: {actual_frames}/{expected_frames} fr"
            log(msg)
            print("::" + msg)
        return actual_frames
