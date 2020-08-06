#!/usr/bin/env python3
import time
import json
import os
from pathlib import Path
from typing import List
import sys
import concurrent
import concurrent.futures
import shutil

from .arg_parse import Args
from .chunk import Chunk
from .chunk_queue import load_or_gen_chunk_queue
from .concat import concat_routine
from .resume import write_progress_file
from .target_vmaf import target_vmaf_routine
from .utils import frame_probe_cv2, terminate, process_inputs
from .bar import Manager, tqdm_bar
from .setup import determine_resources, outputs_filenames, setup
from .logger import log, set_log
from .config import conf
from .ffmpeg import extract_audio, frame_probe
from .fp_reuse import segment_first_pass
from .split import split_routine
from .vmaf import plot_vmaf
from .vvc import to_yuv


def main_queue(args):
    # Todo: Redo Queue
    try:
        conf(args)
        tm = time.time()

        args.queue = process_inputs(args.input)

        for file in args.queue:
            tm = time.time()
            args.input = file

            if len(args.queue) > 1:
                print(f'Encoding: {file}')
                args.output_file = None

            encode_file(args)
            print(f'Finished: {round(time.time() - tm, 1)}s\n')
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


def encode_file(args: Args):
    """
    Encodes a single video file on the local machine.

    :param args: The args for this encode
    :return: None
    """

    args.output_file = outputs_filenames(args.input, args.output_file, args.encoder)

    done_path = args.temp / 'done.json'
    resuming = args.resume and done_path.exists()

    # set up temp dir and logging
    setup(args.temp, args.resume)
    set_log(args.logging, args.temp)

    # find split locations
    split_locations = split_routine(args, resuming)

    # create a chunk queue
    chunk_queue = load_or_gen_chunk_queue(args, resuming, split_locations)

    # things that need to be done only the first time
    if not resuming:

        extract_audio(args.input, args.temp, args.audio_params)

        if args.reuse_first_pass:
            segment_first_pass(args.temp, split_locations)

    # do encoding loop
    args.workers = determine_resources(args.encoder, args.workers)
    encoding_loop(args, chunk_queue)

    # concat
    concat_routine(args)

    if args.vmaf or args.vmaf_plots:
        plot_vmaf(args.input, args.output_file, args, args.vmaf_path, args.vmaf_res)

    # Delete temp folders
    if not args.keep:
        shutil.rmtree(args.temp)


def encoding_loop(args: Args, chunk_queue: List[Chunk]):
    """Creating process pool for encoders, creating progress bar."""
    try:
        done_path = args.temp / 'done.json'

        # TODO: move this out and pass in total frames and initial frames
        if args.resume and done_path.exists():
            log('Resuming...\n')

            with open(done_path) as done_file:
                data = json.load(done_file)

            total = data['total']
            done = len(data['done'])
            initial = sum(data['done'].values())

            log(f'Resumed with {done} encoded clips done\n\n')
        else:
            initial = 0
            total = frame_probe_cv2(args.input)

            if total < 1:
                total = frame_probe(args.input)

            d = {'total': total, 'done': {}}
            with open(done_path, 'w') as done_file:
                json.dump(d, done_file)

        clips = len(chunk_queue)
        args.workers = min(args.workers, clips)

        print(f'\rQueue: {clips} Workers: {args.workers} Passes: {args.passes}\n'
              f'Params: {args.video_params.strip()}')

        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
            counter = Manager().Counter(total, initial)
            future_cmd = {executor.submit(encode, cmd, counter, args): cmd for cmd in chunk_queue}
            for future in concurrent.futures.as_completed(future_cmd):
                future_cmd[future]
                try:
                    future.result()
                except Exception as exc:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                    terminate()
    except KeyboardInterrupt:
        terminate()


def encode(chunk: Chunk, counter, args: Args):
    """
    Encodes a chunk.

    :param chunk: The chunk to encode
    :param counter: the counter to update the bar
    :param args: The cli args
    :return: None
    """
    try:
        st_time = time.time()

        chunk_frames = chunk.frames

        log(f'Enc: {chunk.name}, {chunk_frames} fr\n\n')

        # Target Vmaf Mode
        if args.vmaf_target:
            target_vmaf_routine(args, chunk)

        # remove first pass command if reusing
        if args.reuse_first_pass:
            chunk.remove_first_pass_from_commands()

        # if vvc, we need to create a yuv file
        if args.encoder == 'vvc':
            log(f'Creating yuv for chunk {chunk.name}\n')
            vvc_yuv_file = to_yuv(chunk)
            log(f'Created yuv for chunk {chunk.name}\n')

        # Run all passes for this chunk
        for pass_cmd in chunk.pass_cmds:
            tqdm_bar(chunk.ffmpeg_gen_cmd, pass_cmd, args.encoder, counter, chunk_frames, args.passes)

        # if vvc, we need to delete the yuv file
        if args.encoder == 'vvc':
            os.remove(vvc_yuv_file)

        # get the number of encoded frames, if no check or vvc, assume it worked and encoded same number of frames
        perform_encoded_frame_check = not (args.no_check or args.encoder == 'vvc')
        encoded_frames = frame_check_output(chunk, chunk_frames) if perform_encoded_frame_check else chunk_frames

        # write this chunk as done if it encoded correctly
        if encoded_frames == chunk_frames:
            write_progress_file(Path(args.temp / 'done.json'), chunk, encoded_frames)

        enc_time = round(time.time() - st_time, 2)
        log(f'Done: {chunk.name} Fr: {encoded_frames}\n'
            f'Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.\n\n')

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')


def frame_check_output(chunk: Chunk, expected_frames: int) -> int:
    actual_frames = frame_probe(chunk.output_path.with_suffix(".ivf"))
    if actual_frames != expected_frames:
        print(f'Frame Count Differ for Source {chunk.name}: {actual_frames}/{expected_frames}')
    return actual_frames
