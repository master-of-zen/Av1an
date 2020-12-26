#!/usr/bin/env python3
import time
import json
from pathlib import Path
from typing import List
import sys
import concurrent
import concurrent.futures
import shutil

from .encoder import ENCODERS
from .vmaf import VMAF
from .project import  Project
from .chunk import Chunk
from .chunk.chunk_queue import load_or_gen_chunk_queue
from av1an.resume import write_progress_file
from .target_quality import per_shot_target_quality_routine, per_frame_target_quality_routine
from av1an.utils import frame_probe_fast, frame_probe, terminate
from av1an.bar import Manager, tqdm_bar
from av1an.logger import log, set_log
from av1an.ffmpeg import extract_audio
from av1an.fp_reuse import segment_first_pass
from av1an.split import split_routine


def encode_file(project: Project):
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
    startup(project, chunk_queue)
    encoding_loop(project, chunk_queue)

    # concat
    project.concat_routine()

    if project.vmaf or project.vmaf_plots:
        vmaf = VMAF()
        vmaf.plot_vmaf(project.input, project.output_file, project)

    # Delete temp folders
    if not project.keep:
        shutil.rmtree(project.temp)


def startup(project: Project, chunk_queue: List[Chunk]):
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

    counter = Manager().Counter(project.get_frames(), initial)
    project.counter = counter


def encoding_loop(project: Project, chunk_queue: List[Chunk]):
    """Creating process pool for encoders, creating progress bar."""
    with concurrent.futures.ThreadPoolExecutor(max_workers=project.workers) as executor:
        future_cmd = {executor.submit(encode, cmd, project): cmd for cmd in chunk_queue}
        for future in concurrent.futures.as_completed(future_cmd):
            try:
                future.result()
            except Exception as exc:
                _, _, exc_tb = sys.exc_info()
                print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                terminate()
    project.counter.close()


def encode(chunk: Chunk, project: Project):
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
    encoded_frames = chunk_frames if project.no_check else frame_check_output(chunk, chunk_frames)

    # write this chunk as done if it encoded correctly
    if encoded_frames == chunk_frames:
        write_progress_file(Path(project.temp / 'done.json'), chunk, encoded_frames)

    enc_time = round(time.time() - st_time, 2)
    log(f'Done: {chunk.name} Fr: {encoded_frames}\n'
        f'Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.\n\n')


def frame_check_output(chunk: Chunk, expected_frames: int) -> int:
    actual_frames = frame_probe(chunk.output_path)
    if actual_frames != expected_frames:
        print(f':: Chunk #{chunk.name}: {actual_frames}/{expected_frames} fr')
    return actual_frames
