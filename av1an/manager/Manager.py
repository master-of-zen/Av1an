import json
import shutil
import sys
import time
from multiprocessing.managers import BaseManager
from pathlib import Path
from typing import List

from av1an.chunk import Chunk
from av1an.chunk.chunk_queue import load_or_gen_chunk_queue
from av1an.ffmpeg import extract_audio
from av1an.fp_reuse import segment_first_pass
from av1an.logger import log, set_log
from av1an.project.Project import Project
from av1an.resume import write_progress_file
from av1an.split import split_routine
from av1an.startup.file_validation import process_inputs

from av1an.utils import frame_probe, terminate
from av1an.vmaf import VMAF

from .Counter import BaseManager, Counter, Manager
from .Queue import Queue


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
        self.initial_frames = 0

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

        self.done_file(project, chunk_queue)
        if not project.resume:
            extract_audio(project.input, project.temp, project.audio_params)

            if project.reuse_first_pass:
                segment_first_pass(project.temp, split_locations)

        # do encoding loop
        project.determine_workers()
        self.startup(project, chunk_queue)
        queue = Queue(project, chunk_queue)
        queue.encoding_loop()

        if queue.status.lower() == 'fatal':
            msg = '::FATAL:: Encoding process encountered fatal error, shutting down\n'
            print('\n', msg)
            log(msg)
            sys.exit(1)

        # concat
        project.concat_routine()

        if project.vmaf or project.vmaf_plots:
            self.vmaf = VMAF(n_threads=project.n_threads, model=project.vmaf_path, res=project.vmaf_res, vmaf_filter=project.vmaf_filter)
            self.vmaf.plot_vmaf(project.input, project.output_file, project)

        # Delete temp folders
        if not project.keep:
            shutil.rmtree(project.temp)

    def done_file(self, project: Project, chunk_queue: List[Chunk]):
        done_path = project.temp / 'done.json'
        if project.resume and done_path.exists():
            log('Resuming...\n')
            with open(done_path) as done_file:
                data = json.load(done_file)

            project.set_frames(data['frames'])
            done = len(data['done'])
            self.initial_frames = sum(data['done'].values())
            log(f'Resumed with {done} encoded clips done\n\n')
        else:
            self.initial_frames = 0
            total = project.get_frames()
            d = {'frames': total, 'done': {}}
            with open(done_path, 'w') as done_file:
                json.dump(d, done_file)

    def startup(self, project: Project, chunk_queue: List[Chunk]):
        clips = len(chunk_queue)
        project.workers = min(project.workers, clips)
        print(f'\rQueue: {clips} Workers: {project.workers} Passes: {project.passes}\n'
                f'Params: {" ".join(project.video_params)}')
        BaseManager.register('Counter', Counter)
        counter = Manager().Counter(project.get_frames(), self.initial_frames, not project.quiet)
        project.counter = counter

