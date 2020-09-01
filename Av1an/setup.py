#!/bin/env python

import atexit
import os
import shlex
import shutil
import sys
from distutils.spawn import find_executable
from pathlib import Path

from psutil import virtual_memory

from Av1an.encoders import ENCODERS
from Av1an.arg_parse import Args
from Av1an.utils import terminate


def set_vmaf(args):
    """
    Av1an setup for VMAF

    :param args: the Args
    """
    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate()

    if args.vmaf_steps < 4:
        print('Target vmaf require more than 3 probes/steps')
        terminate()

    encoder = ENCODERS[args.encoder]

    if args.min_q is None:
        args.min_q, _ = encoder.default_q_range
    if args.max_q is None:
        _, args.max_q = encoder.default_q_range


def check_exes(args: Args):
    """
    Checking required executables

    :param args: the Args
    """

    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()

    # this shouldn't happen as encoder choices are validated by argparse
    if args.encoder not in ENCODERS:
        valid_encoder_str = ", ".join([repr(k) for k in ENCODERS])
        print(f'Not valid encoder {args.encoder}')
        print(f'Valid encoders: {valid_encoder_str}')
        terminate()

    if args.chunk_method == 'vs_ffms2' and (not find_executable('vspipe')):
        print('vspipe executable not found')
        terminate()


def setup_encoder(args: Args):
    """
    Settup encoder params and passes

    :param args: the Args
    """
    encoder = ENCODERS[args.encoder]

    # validate encoder settings
    settings_valid, error_msg = encoder.is_valid(args)
    if not settings_valid:
        print(error_msg)
        terminate()

    if args.passes is None:
        args.passes = encoder.default_passes

    args.video_params = encoder.default_args if args.video_params is None \
    else shlex.split(args.video_params)


def startup_check(args: Args):
    """
    Performing essential checks at startup_check
    Set constant values
    """
    if sys.version_info < (3, 6):
        print('Python 3.6+ required')
        sys.exit()
    if sys.platform == 'linux':
        def restore_term():
            os.system("stty sane")

        atexit.register(restore_term)

    check_exes(args)

    set_vmaf(args)

    if args.reuse_first_pass and args.encoder != 'aom' and args.split_method != 'aom_keyframes':
        print('Reusing the first pass is only supported with \
              the aom encoder and aom_keyframes split method.')
        terminate()

    setup_encoder(args)

    # No check because vvc
    if args.encoder == 'vvc':
        args.no_check = True

    if args.encoder == 'svt_vp9' and args.passes == 2:
        print("Implicitly changing 2 pass svt-vp9 to 1 pass\n2 pass svt-vp9 isn't supported")
        args.passes = 1

    args.audio_params = shlex.split(args.audio_params)
    args.ffmpeg = shlex.split(args.ffmpeg)

    args.pix_format = ['-strict', '-1', '-pix_fmt', args.pix_format]
    args.ffmpeg_pipe = [*args.ffmpeg, *args.pix_format,
                        '-bufsize', '50000K', '-f', 'yuv4mpegpipe', '-']


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        workers = round(min(cpu / 2, ram / 1.5))

    elif encoder in ('svt_av1', 'svt_vp9', 'x265', 'x264'):
        workers = round(min(cpu, ram)) // 8

    elif encoder in 'vvc':
        workers = round(min(cpu, ram))

    # fix if workers round up to 0
    if workers == 0:
        workers = 1

    return workers


def setup(temp: Path, resume):
    """Creating temporally folders when needed."""
    # Make temporal directories, and remove them if already presented
    if not resume:
        if temp.is_dir():
            shutil.rmtree(temp)

    (temp / 'split').mkdir(parents=True, exist_ok=True)
    (temp / 'encode').mkdir(exist_ok=True)


def outputs_filenames(args: Args):
    """
    Set output filename

    :param args: the Args
    """
    suffix = '.mkv'
    args.output_file = Path(f'{args.output_file}{suffix}') if args.output_file \
                  else Path(f'{args.input.stem}_av1{suffix}')
