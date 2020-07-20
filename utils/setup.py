#!/bin/env python

import os
import sys
import shutil
import atexit
from distutils.spawn import find_executable
from pathlib import Path

from psutil import virtual_memory

from .utils import terminate
from .compose import get_default_params_for_encoder


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        workers =  round(min(cpu / 2, ram / 1.5))

    elif encoder in ('svt_av1', 'x265'):
        workers =  round(min(cpu, ram)) // 5

    # fix if workers round up to 0
    if workers == 0:
        workers = 1

    return workers


def startup_check(args):

    if sys.version_info < (3, 6):
        print('Python 3.6+ required')
        sys.exit()
    if sys.platform == 'linux':
        def restore_term():
            os.system("stty sane")
        atexit.register(restore_term)

    encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc','x265': 'x265'}
    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()

    # Check if encoder executable is reachable
    if args.encoder in encoders:
        enc = encoders.get(args.encoder)

        if not find_executable(enc):
            print(f'Encoder {enc} not found')
            terminate()
    else:
        print(f'Not valid encoder {args.encoder}\nValid encoders: "aom rav1e", "svt_av1", "vpx", "x265" ')
        terminate()

    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate()

    if args.reuse_first_pass and args.encoder != 'aom' and args.split_method != 'aom_keyframes':
        print('Reusing the first pass is only supported with the aom encoder and aom_keyframes split method.')
        terminate()

    if args.vmaf_steps < 4:
        print('Target vmaf require more than 3 probes/steps')
        terminate()

    if args.video_params is None:
        args.video_params = get_default_params_for_encoder(args.encoder)


def setup(temp: Path, resume):
    """Creating temporally folders when needed."""
    # Make temporal directories, and remove them if already presented
    if not resume:
        if temp.is_dir():
            shutil.rmtree(temp)

    (temp / 'split').mkdir(parents=True, exist_ok=True)
    (temp / 'encode').mkdir(exist_ok=True)


def outputs_filenames(inp: Path, out:Path, encoder):
    suffix = '.mkv'
    if out:
        return out.with_suffix(suffix)
    else:
        return Path(f'{inp.stem}_av1{suffix}')
