#!/bin/env python

import os
import shutil
from pathlib import Path

from psutil import virtual_memory
from libAv1an.LibAv1an.args import Args


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
    args.output_file = Path(args.output_file).with_suffix(suffix) if args.output_file \
        else Path(f'{args.input.stem}_{args.encoder}{suffix}')
