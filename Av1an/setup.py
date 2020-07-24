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


def set_vmaf(args):

    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate()

    if args.vmaf_steps < 4:
        print('Target vmaf require more than 3 probes/steps')
        terminate()


    defaul_ranges = {'svt_av1': (20, 40), 'rav1e': (70, 150), 'aom': (25, 50), 'vpx': (25, 50),'x265': (20, 40), 'vvc': (20, 50)}

    if args.min_q is None or args.max_q is None:
        args.min_q, args.max_q = defaul_ranges.get(args.encoder)


def check_exes(args):
    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()
    encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc','x265': 'x265', 'vvc': 'vvc_encoder'}


    # Check if encoder executable is reachable
    if args.encoder in encoders:
        enc = encoders.get(args.encoder)

        if not find_executable(enc):
            print(f'Encoder {enc} not found')
            terminate()
    else:
        print(f'Not valid encoder {args.encoder}\nValid encoders: "aom rav1e", "svt_av1", "vpx", "x265" ')
        terminate()

    if args.encoder == 'vvc':
        if not find_executable('vvc_concat'):
            print('vvc concatenation executabe "vvc_concat" not found')
            terminate()


def startup_check(args):

    encoders_default_passes = {'svt_av1': 1, 'rav1e': 1, 'aom': 2, 'vpx': 2,'x265': 1, 'vvc':1 }


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

    if args.video_params is None:
        args.video_params = get_default_params_for_encoder(args.encoder)



    args.pix_format = f'-strict -1 -pix_fmt {args.pix_format}'
    args.ffmpeg_pipe = f' {args.ffmpeg} {args.pix_format} -f yuv4mpegpipe - |'


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
