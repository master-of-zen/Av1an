#!/bin/env python

import json
import re
import statistics
import subprocess
import sys
from typing import Tuple, List
from pathlib import Path
from subprocess import PIPE
import cv2
import numpy as np

from Av1an.commandtypes import Command


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


def man_q(command: Command, q: int):
    """Return command with new cq value"""

    adjusted_command = command.copy()

    if ('aomenc' in command) or ('vpxenc' in command):
        i = list_index_of_regex(adjusted_command, r"--cq-level=.+")
        adjusted_command[i] = f'--cq-level={q}'

    elif ('x265' in command) or ('x264' in command):
        i = list_index_of_regex(adjusted_command, r"--crf")
        adjusted_command[i + 1] = f'{q}'

    elif 'rav1e' in command:
        i = list_index_of_regex(adjusted_command, r"--quantizer")
        adjusted_command[i + 1] = f'{q}'

    elif 'SvtAv1EncApp' in command:
        i = list_index_of_regex(adjusted_command, r"--qp")
        adjusted_command[i + 1] = f'{q}'

    return adjusted_command


def frame_probe_cv2(source: Path):
    video = cv2.VideoCapture(source.as_posix())
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    return total
