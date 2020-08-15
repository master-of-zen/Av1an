#!/bin/env python

import os
import sys
import shutil
import atexit
import shlex
from distutils.spawn import find_executable
from pathlib import Path

from psutil import virtual_memory

from .arg_parse import Args
from .encoders import ENCODERS
from .utils import terminate


def set_vmaf(args):

    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate()

    if args.vmaf_steps < 4:
        print('Target vmaf require more than 3 probes/steps')
        terminate()

    default_ranges = {'svt_av1': (20, 40), 'rav1e': (70, 150), 'aom': (25, 50), 'vpx': (25, 50),'x265': (20, 40), 'x264': (20, 35), 'vvc': (20, 50)}

    if args.min_q is None:
        args.min_q, _ = default_ranges[args.encoder]
    if args.max_q is None:
        _, args.max_q = default_ranges[args.encoder]


def check_exes(args: Args):

    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()

    # this shouldn't happen as encoder choices are validated by argparse
    if args.encoder not in ENCODERS:
        valid_encoder_str = ", ".join([repr(k) for k in ENCODERS.keys()])
        print(f'Not valid encoder {args.encoder}')
        print(f'Valid encoders: {valid_encoder_str}')
        terminate()

    # make sure encoder is valid on system path
    encoder = ENCODERS[args.encoder]
    if not encoder.check_exists():
        print(f'Encoder {encoder.encoder_bin} not found. Is it installed in the system path?')
        terminate()

    if args.chunk_method == 'vs_ffms2' and (not find_executable('vspipe')):
        print('vspipe executable not found')
        terminate()


def startup_check(args):

    encoders_default_passes = {'svt_av1': 1, 'rav1e': 1, 'aom': 2, 'vpx': 2,'x265': 1, 'x264': 1, 'vvc':1 }


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
        print('Reusing the first pass is only supported with the aom encoder and aom_keyframes split method.')
        terminate()


    if args.encoder == 'vvc' and not args.vvc_conf:
        print('Conf file for vvc required')
        terminate()

    # No check because vvc
    if args.encoder == 'vvc':
        args.no_check = True

    if args.passes is None:
        args.passes = encoders_default_passes.get(args.encoder)


    if args.video_params is None and args.encoder == 'vvc':
        print('VVC require:\n',
        ' -wdt X - video width\n',
        ' -hgt X - video height\n',
        ' -fr X  - framerate\n',
        ' -q X   - quantizer\n',
        'Example: -wdt 640 -hgt 360 -fr 23.98 -q 30 '
        )
        terminate()

    # TODO: rav1e 2 pass is broken
    if args.encoder == 'rav1e' and args.passes == 2:
        print("Implicitly changing 2 pass rav1e to 1 pass\n2 pass Rav1e doesn't work")
        args.passes = 1

    if args.video_params is None:
        args.video_params = ENCODERS[args.encoder].default_args
    else:
        args.video_params = shlex.split(args.video_params)
    args.audio_params = shlex.split(args.audio_params)
    args.ffmpeg = shlex.split(args.ffmpeg)

    args.pix_format = ['-strict', '-1', '-pix_fmt', args.pix_format]
    args.ffmpeg_pipe = [*args.ffmpeg, *args.pix_format, '-bufsize', '50000K', '-f', 'yuv4mpegpipe', '-']


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        workers =  round(min(cpu / 2, ram / 1.5))

    elif encoder in ('svt_av1', 'x265', 'x264'):
        workers =  round(min(cpu, ram)) // 8

    elif encoder in ('vvc'):
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


def outputs_filenames(inp: Path, out:Path, encoder):
    suffix = '.mkv'
    if out:
        return out.with_suffix(suffix)
    else:
        return Path(f'{inp.stem}_av1{suffix}')
