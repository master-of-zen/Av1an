#!/bin/env python

import re
import sys
from typing import List
from pathlib import Path
import cv2
import numpy as np

from Av1an.commandtypes import Command
from Av1an.ffmpeg import frame_probe_ffmpeg
from Av1an.vapoursynth import frame_probe_vspipe, is_vapoursynth

def terminate():
    sys.exit(1)


def process_inputs(inputs):
    # Check input file for being valid
    if not inputs:
        print('No input file')
        terminate()

    if inputs[0].is_dir():
        inputs = [x for x in inputs[0].iterdir() if x.suffix in (".mkv", ".mp4", ".mov", ".avi", ".flv", ".m2ts")]

    valid = np.array([i.exists() for i in inputs])

    if not all(valid):
        print(f'File(s) do not exist: {", ".join([str(inputs[i]) for i in np.where(not valid)[0]])}')
        terminate()

    return inputs


def get_cq(command):
    """
    Return cq values from command
    :param command: string with commands for encoder
    :return: list with frame numbers of keyframes

    """
    matches = re.findall(r"--cq-level= *([^ ]+?) ", command)
    return int(matches[-1])


def list_index_of_regex(lst: List[str], regex_str: str) -> int:
    """
    Gets the first index of the list where regex_str matches

    :param lst: the list
    :param regex_str: the regex as a string
    :return: the index where regex_str appears in the list
    :raises ValueError: if regex_str is not found
    """
    reg = re.compile(regex_str)
    for i, elem in enumerate(lst):
        if reg.match(elem):
            return i
    raise ValueError(f'{reg} is not in list')


def frame_probe_fast(source: Path, is_vs: bool = False):
    """
    Consolidated function to retrieve the number of frames from the input quickly,
    falls back on a slower (but accurate) frame count if a quick count cannot be found.

    Handles vapoursynth input as well.
    """
    total = 0
    if not is_vs:
        total = frame_probe_cv2(source)

    if is_vs or total < 1:
        total = frame_probe(source)

    return total


def frame_probe_cv2(source: Path):
    video = cv2.VideoCapture(source.as_posix())
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    video.release()
    return total


def frame_probe(source: Path):
    """
    Determines the total number of frames in a given input.

    Differentiates between a Vapoursynth script and standard video
    and delegates to vspipe or ffmpeg respectively.
    """
    if is_vapoursynth(source):
        return frame_probe_vspipe(source)
    return frame_probe_ffmpeg(source)
