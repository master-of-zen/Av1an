import time
import sys
import concurrent
import concurrent.futures
from collections import deque
from av1an.project.Project import Project
from typing import List
from av1an.target_quality import (per_frame_target_quality_routine,
                                  per_shot_target_quality_routine)
from av1an.encoder import ENCODERS
from av1an.utils import frame_probe, terminate
from av1an.resume import write_progress_file
from av1an.chunk import Chunk
from av1an.logger import log, set_log
from pathlib import Path
from .Pipes import tqdm_bar


class Queue:
    """
    Queue manager with ability to add/remove/restart jobs
    """
    def __init__(self, project, chunk_queue):
        self.chunk_queue = chunk_queue
        self.queue = []
        self.project = project
        self.thread_executor = concurrent.futures.ThreadPoolExecutor()
        self.status = 'Ok'

    def encoding_loop(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=self.project.workers) as executor:
            future_cmd = {executor.submit(self.encode_chunk, cmd): cmd for cmd in self.chunk_queue}
            for future in concurrent.futures.as_completed(future_cmd):
                try:
                    future.result()
                except Exception as exc:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                    terminate()
        self.project.counter.close()


    def encode_chunk(self, chunk: Chunk):
        """
        Encodes a chunk. If chunk fails, restarts it limited amount of times.
        Return if executed just fine, sets status fatal for queue if failed

        :param chunk: The chunk to encode
        :return: None
        """
        restart_count = 0

        while restart_count < 3:
            try:
                st_time = time.time()

                chunk_frames = chunk.frames

                log(f'Enc: {chunk.index}, {chunk_frames} fr\n\n')

                # Target Quality Mode
                if self.project.target_quality:
                    if self.project.target_quality_method == 'per_shot':
                        per_shot_target_quality_routine(self.project, chunk)
                    if self.project.target_quality_method == 'per_frame':
                        per_frame_target_quality_routine(self.project, chunk)

                ENCODERS[self.project.encoder].on_before_chunk(self.project, chunk)

                # skip first pass if reusing
                start = 2 if self.project.reuse_first_pass and self.project.passes >= 2 else 1

                # Run all passes for this chunk
                for current_pass in range(start, self.project.passes + 1):
                    tqdm_bar(self.project, chunk, self.project.encoder, self.project.counter, chunk_frames, self.project.passes, current_pass)

                ENCODERS[self.project.encoder].on_after_chunk(self.project, chunk)

                # get the number of encoded frames, if no check assume it worked and encoded same number of frames
                encoded_frames = chunk_frames if self.project.no_check else self.frame_check_output(chunk, chunk_frames)

                # write this chunk as done if it encoded correctly
                if encoded_frames == chunk_frames:
                    write_progress_file(Path(self.project.temp / 'done.json'), chunk, encoded_frames)

                enc_time = round(time.time() - st_time, 2)
                log(f'Done: {chunk.index} Fr: {encoded_frames}/{chunk_frames}\n'
                    f'Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.\n\n')
                return

            except Exception as e:
                msg = f':: Chunk #{chunk.index} crashed with:\n:: Exception: {type(e)}\n {e}\n:: Restarting chunk\n'
                log(msg + '\n')
                print(msg)
                restart_count += 1

        msg = f'::FATAL::\n::Chunk #{chunk.index} failed more than 3 times, shutting down thread\n\n'
        log(msg)
        print(msg)
        self.status = 'FATAL'

    def frame_check_output(self, chunk: Chunk, expected_frames: int, last_chunk=False) -> int:
        actual_frames = frame_probe(chunk.output_path)
        if actual_frames != expected_frames:
            msg = f':: Chunk #{chunk.index}: {actual_frames}/{expected_frames} fr'
            log(msg)
            print(msg)
        return actual_frames