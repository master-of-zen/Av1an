#!/usr/bin/env python3
import time
import json
from pathlib import Path
from typing import List
import sys
import concurrent
import concurrent.futures
import shutil

from libAv1an.Encoders import ENCODERS
from libAv1an.LibAv1an.args import Args
from libAv1an.Chunks.chunk import Chunk
from libAv1an.Chunks.chunk_queue import load_or_gen_chunk_queue
from libAv1an.LibAv1an.concat import concat_routine
from libAv1an.Chunks.resume import write_progress_file
from libAv1an.VMAF.target_vmaf import target_vmaf_routine
from libAv1an.LibAv1an.utils import frame_probe_fast, frame_probe, process_inputs
from libAv1an.LibAv1an.setup import determine_resources, outputs_filenames, setup
from libAv1an.LibAv1an.ffmpeg import extract_audio
from libAv1an.LibAv1an.fp_reuse import segment_first_pass
from libAv1an.Chunks.split import split_routine, extra_splits
from libAv1an.VMAF.vmaf import plot_vmaf
from libAv1an.LibAv1an.vapoursynth import is_vapoursynth
from libAv1an.LibAv1an.callbacks import Callbacks
from libAv1an.LibAv1an.run_cmd import process_pipe, process_encoding_pipe


# todo, saving and loading more info to the scenes data
def main_queue(args, cb: Callbacks):
    # Todo: Redo Queue
    try:
        tm = time.time()

        args.queue = process_inputs(args.input, cb)

        for file in args.queue:
            tm = time.time()
            args.input = file
            is_vs = is_vapoursynth(args.input)
            args.is_vs = is_vs
            args.chunk_method = 'vs_ffms2' if is_vs else args.chunk_method

            if len(args.queue) > 1:
                print(f'Encoding: {file}')
                args.output_file = None

            encode_file(args, cb)

            print(f'Finished: {round(time.time() - tm, 1)}s\n')
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


def encode_file(args: Args, cb: Callbacks):
    """
    Encodes a single video file on the local machine.

    :param args: The args for this encode
    :param cb: Callbacks
    :return: None
    """

    outputs_filenames(args)

    done_path = args.temp / 'done.json'
    resuming = args.resume and done_path.exists()

    # set up temp dir and logging
    setup(args.temp, args.resume)
    cb.run_callback("logready", args.logging, args.temp)

    # find split locations
    split_locations = split_routine(args, resuming, cb)

    # Applying extra splits
    if args.extra_split:
        split_locations = extra_splits(args, split_locations, cb)

    # create a chunk queue
    chunk_queue = load_or_gen_chunk_queue(args, resuming, split_locations, cb)

    # things that need to be done only the first time
    if not resuming:

        extract_audio(args.input, args.temp, args.audio_params, cb)

        if args.reuse_first_pass:
            segment_first_pass(args.temp, split_locations)

    # do encoding loop
    args.workers = determine_resources(args.encoder, args.workers)
    startup(args, chunk_queue, cb)
    encoding_loop(args, cb, chunk_queue)

    # concat
    concat_routine(args, cb)

    if args.vmaf or args.vmaf_plots:
        plot_vmaf(args.input, args.output_file, args, args.vmaf_path, args.vmaf_res, cb)

    # Delete temp folders
    if not args.keep:
        shutil.rmtree(args.temp)


def startup(args: Args, chunk_queue: List[Chunk], cb: Callbacks):
    """
    If resuming, open done file and get file properties from there
    else get file properties and

    """
    # TODO: move this out and pass in total frames and initial frames
    done_path = args.temp / 'done.json'
    if args.resume and done_path.exists():
        cb.run_callback("log", 'Resuming...\n')
        with open(done_path) as done_file:
            data = json.load(done_file)
        total = data['total']
        done = len(data['done'])
        initial = sum(data['done'].values())
        cb.run_callback("log", f'Resumed with {done} encoded clips done\n\n')
    else:
        initial = 0
        total = frame_probe_fast(args.input, args.is_vs)
        d = {'total': total, 'done': {}}
        with open(done_path, 'w') as done_file:
            json.dump(d, done_file)
    clips = len(chunk_queue)
    args.workers = min(args.workers, clips)
    print(f'\rQueue: {clips} Workers: {args.workers} Passes: {args.passes}\n'
          f'Params: {" ".join(args.video_params)}')
    cb.run_callback("startencode", total, initial)


def encoding_loop(args: Args, cb: Callbacks, chunk_queue: List[Chunk]):
    """Creating process pool for encoders, creating progress bar."""
    if args.workers != 0:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
            future_cmd = {executor.submit(encode, cmd, args, cb): cmd for cmd in chunk_queue}
            for future in concurrent.futures.as_completed(future_cmd):
                try:
                    future.result()
                except Exception as exc:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                    cb.run_callback("terminate", 1)
    cb.run_callback("endencode")


def encode(chunk: Chunk, args: Args, cb: Callbacks):
    """
    Encodes a chunk.

    :param chunk: The chunk to encode
    :param args: The cli args
    :param cb: The callback object
    :return: None
    """
    try:
        st_time = time.time()

        chunk_frames = chunk.frames

        cb.run_callback("log", f'Enc: {chunk.name}, {chunk_frames} fr\n\n')

        # Target Vmaf Mode
        if args.vmaf_target:
            target_vmaf_routine(args, chunk, cb)

        ENCODERS[args.encoder].on_before_chunk(args, chunk)

        # skip first pass if reusing
        start = 2 if args.reuse_first_pass and args.passes >= 2 else 1

        # Run all passes for this chunk
        for current_pass in range(start, args.passes + 1):
            try:
                enc = ENCODERS[args.encoder]
                pipe = enc.make_pipes(args, chunk, args.passes, current_pass, chunk.output)

                if args.encoder in ('aom', 'vpx', 'rav1e', 'x265', 'x264', 'vvc', 'svt_av1'):
                    process_encoding_pipe(pipe, args.encoder, cb)

                if args.encoder in ('svt_vp9'):
                    # SVT-AV1 developer: SVT-AV1 is special in the way it outputs to console
                    # SVT-AV1 got a new output mode, but SVT-VP9 is still special
                    process_pipe(pipe)
                    cb.run_callback("svtvp9update", chunk_frames, args.passes)

            except Exception as e:
                _, _, exc_tb = sys.exc_info()
                print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')

        ENCODERS[args.encoder].on_after_chunk(args, chunk)

        # get the number of encoded frames, if no check assume it worked and encoded same number of frames
        encoded_frames = chunk_frames if args.no_check else frame_check_output(chunk, chunk_frames)

        # write this chunk as done if it encoded correctly
        if encoded_frames == chunk_frames:
            write_progress_file(Path(args.temp / 'done.json'), chunk, encoded_frames)

        enc_time = round(time.time() - st_time, 2)
        cb.run_callback("log", f'Done: {chunk.name} Fr: {encoded_frames}\n'
            f'Fps: {round(encoded_frames / enc_time, 4)} Time: {enc_time} sec.\n\n')

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')


def frame_check_output(chunk: Chunk, expected_frames: int) -> int:
    actual_frames = frame_probe(chunk.output_path)
    if actual_frames != expected_frames:
        print(f'Frame Count Differ for Source {chunk.name}: {actual_frames}/{expected_frames}')
    return actual_frames
