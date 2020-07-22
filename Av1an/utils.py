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


def man_q(command: str, q: int):
    """Return command with new cq value"""

    if 'aomenc' in command or 'vpxenc' in command:
        mt = '--cq-level='
        cmd = command[:command.find(mt) + 11] + str(q) + ' ' + command[command.find(mt) + 13:]

    elif 'x265' in command:
        mt = '--crf'
        cmd = command[:command.find(mt) + 6] + str(q) + ' ' +  command[command.find(mt) + 9:]

    elif 'rav1e' in command:
        mt = '--quantizer'
        cmd = command[:command.find(mt) + 11] + ' ' + str(q) + ' ' +  command[command.find(mt) + 15:]

    elif 'SvtAv1EncApp' in command:
        mt = '--qp'
        cmd = command[:command.find(mt) + 4] + ' ' + str(q) + ' ' +  command[command.find(mt) + 7:]
    return cmd


def frame_probe_cv2(source: Path):
    video = cv2.VideoCapture(source.as_posix())
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    return total


def get_brightness(video):
    """Getting average brightness value for single video."""
    brightness = []
    cap = cv2.VideoCapture(video)
    try:
        while True:
            # Capture frame-by-frame
            _, frame = cap.read()

            # Our operations on the frame come here
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)

            # Display the resulting frame
            mean = cv2.mean(gray)
            brightness.append(mean[0])
            if cv2.waitKey(1) & 0xFF == ord('q'):
                break
    except cv2.error:
        pass

    # When everything done, release the capture
    cap.release()
    brig_geom = round(statistics.geometric_mean([x + 1 for x in brightness]), 1)

    return brig_geom
