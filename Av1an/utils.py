#!/bin/env python

import json
import re
import statistics
import subprocess
import sys
from pathlib import Path
from subprocess import PIPE


import cv2
import numpy as np

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


def man_q(command: list, q: int):
    """Return command with new cq value"""

    adjusted_command = []
    for cmd in command:
        if 'aomenc' in command or 'vpxenc' in command:
            mt = '--cq-level='
            for x in command:
                if mt in x:
                    adjusted_command.append(command[:command.index(x) - 1] + (f'{mt}{q}',) + command[command.index(mt) + 1:])

        elif 'x265' in command or 'x264' in command:
            mt = '--crf'
            adjusted_command.append(command[:command.index(mt)] + (str(q),) + command[command.index(mt) + 2:])

        elif 'rav1e' in command:
            mt = '--quantizer'
            adjusted_command.append(command[:command.index(mt)] + (str(q),) + command[command.index(mt) + 2:])

        elif 'SvtAv1EncApp' in command:
            mt = '--qp'
            adjusted_command.append(command[:command.index(mt)] + (str(q),) + command[command.index(mt) + 2:])
        else:
            adjusted_command.append(cmd)
    return tuple(adjusted_command)


def frame_probe_cv2(source: Path):
    video = cv2.VideoCapture(source.as_posix())
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    return total

