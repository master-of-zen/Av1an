import concurrent
import concurrent.futures
import json
import shutil
import sys
import time
from collections import deque
from multiprocessing.managers import BaseManager
from pathlib import Path
from subprocess import STDOUT
from typing import List

from av1an.chunk import Chunk
from av1an.chunk.chunk_queue import load_or_gen_chunk_queue
from av1an.commandtypes import CommandPair
from av1an.encoder import ENCODERS
from av1an.ffmpeg import extract_audio
from av1an.fp_reuse import segment_first_pass
from av1an.logger import log, set_log
from av1an.project import Project
from av1an.project.Project import Project
from av1an.resume import write_progress_file
from av1an.split import split_routine
from av1an.startup.file_validation import process_inputs
from av1an.target_quality import (per_frame_target_quality_routine,
                                  per_shot_target_quality_routine)
from av1an.utils import frame_probe, frame_probe_fast, terminate
from av1an.vmaf import VMAF

from .Counter import BaseManager, Counter, Manager
from .Pipes import tqdm_bar


class Main:

    def __init__(self, args):
        self.file_queue: list[Path] = []
        self.args = args
        self.file_queue = process_inputs(args.input)
        self.projects = self.create_project_list()

    def create_project_list(self):
        """
        Returns list of initialized Project objects with single input
        """
        queue = []
        for file in self.file_queue:
            project = Project(vars(self.args))
            project.input = file
            project.outputs_filenames()
            queue.append(project)
        return queue


    def run(self):
        """
        Run encoding in queue or single file
        """
        for i, proj in enumerate(self.projects):
            if proj.output_file.exists() and len(self.projects) > 1:
                print(f":: Skipping file {proj.input.name}\n:: Outputfile {proj.output_file.name} exists")

                # Don't print new line on last project to console
                if i+1 < len(self.projects):
                    print()

                continue
            try:
                tm = time.time()

                if len(self.projects) > 1:
                    print(f":: Encoding file {proj.input.name}")
                EncodingManager().encode_file(proj)

                print(f'Finished: {round(time.time() - tm, 1)}s\n')
            except KeyboardInterrupt:
                print('Encoding stopped')
                sys.exit()


class EncodingManager:

    def __init__(self):
        self.workers = None
        self.vmaf = None

    def encode_file(self, project: Project):
        """
        Encodes a single video file on the local machine.

        :param project: The project for this encode
        :return: None
        """

        project.setup()
        set_log(project.logging, project.temp)

        # find split locations
        split_locations = split_routine(project, project.resume)

        # create a chunk queue
        chunk_queue = load_or_gen_chunk_queue(project, project.resume, split_locations)

        # things that need to be done only the first time
        if not project.resume:

            extract_audio(project.input, project.temp, project.audio_params)

            if project.reuse_first_pass:
                segment_first_pass(project.temp, split_locations)

        # do encoding loop
        project.determine_workers()
        self.startup(project, chunk_queue)
        self.encoding_loop(project, chunk_queue)

        # concat
        project.concat_routine()

        if project.vmaf or project.vmaf_plots:
            self.vmaf = VMAF(n_threads=project.n_threads, model=project.vmaf_path, res=project.vmaf_res, vmaf_filter=project.vmaf_filter)
            self.vmaf.plot_vmaf(project.input, project.output_file, project)

        # Delete temp folders
        if not project.keep:
            shutil.rmtree(project.temp)

    def startup(self, project: Project, chunk_queue: List[Chunk]):
        done_path = project.temp / 'done.json'
        if project.resume and done_path.exists():
            log('Resuming...\n')
            with open(done_path) as done_file:
                data = json.load(done_file)

            project.set_frames(data['frames'])
            done = len(data['done'])
            initial = sum(data['done'].values())
            log(f'Resumed with {done} encoded clips done\n\n')
        else:
            initial = 0
            total = project.get_frames()
            d = {'frames': total, 'done': {}}
            with open(done_path, 'w') as done_file:
                json.dump(d, done_file)
        clips = len(chunk_queue)
        project.workers = min(project.workers, clips)
        print(f'\rQueue: {clips} Workers: {project.workers} Passes: {project.passes}\n'
                f'Params: {" ".join(project.video_params)}')
        BaseManager.register('Counter', Counter)
        counter = Manager().Counter(project.get_frames(), initial)
        project.counter = counter

    def encoding_loop(self, project: Project, chunk_queue: List[Chunk]):
        """
        Creating process pool for encoders, creating progress bar
        """
        with concurrent.futures.ThreadPoolExecutor(max_workers=project.workers) as executor:
            future_cmd = {executor.submit(self.encode, cmd, project): cmd for cmd in chunk_queue}
            for future in concurrent.futures.as_completed(future_cmd):
                try:
                    future.result()
                except Exception as exc:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                    terminate()
        project.counter.close()

    def encode(self, chunk: Chunk, project: Project):
        """
        Encodes a chunk.

        :param chunk: The chunk to encode
        :param project: The cli project
        :return: None
        """
        st_time = time.time()

        chunk_frames = chunk.frames

        log(f'Enc: {chunk.name}, {chunk_frames} fr\n\n')

        # Target Quality Mode
        if project.target_quality:
            if project.target_quality_method == 'per_shot':
                per_shot_target_quality_routine(project, chunk)
            if project.target_quality_method == 'per_frame':
                per_frame_target_quality_routine(project, chunk)

        ENCODERS[project.encoder].on_before_chunk(project, chunk)

        # skip first pass if reusing
        start = 2 if project.reuse_first_pass and project.passes >= 2 else 1

        # Run all passes for this chunk
        for current_pass in range(start, project.passes + 1):
            tqdm_bar(project, chunk, project.encoder, project.counter, chunk_frames, project.passes, current_pass)

        ENCODERS[project.encoder].on_after_chunk(project, chunk)

        # get the number of encoded frames, if no check assume it worked and encoded same number of frames
        encoded_frames = chunk_frames if project.no_check else self.frame_check_output(chunk, chunk_frames)

        # write this chunk as done if it encoded correctly
        if encoded_frames == chunk_frames:
            write_progress_file(Path(project.temp / 'done.json'), chunk, encoded_frames)

        enc_time = round(time.time() - st_time, 2)
        log(f'Done: {chunk.name} Fr: {encoded_frames}/{chunk_frames}\n'
            f'Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.\n\n')

    def frame_check_output(self, chunk: Chunk, expected_frames: int) -> int:
        actual_frames = frame_probe(chunk.output_path)
        if actual_frames != expected_frames:
            print(f':: Chunk #{chunk.name}: {actual_frames}/{expected_frames} fr')
        return actual_frames
