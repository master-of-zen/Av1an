#!/bin/env python

import re
import subprocess
from pathlib import Path
from subprocess import PIPE, STDOUT
from typing import List
from av1an.logger import log


def get_frametypes(file: Path) -> List:
    """
    Read file and return list with all frame types
    :param file: Path for file
    :return: list with sequence of frame types
    """

    frames = []

    ff = [
        "ffmpeg",
        "-hide_banner",
        "-i",
        file.as_posix(),
        "-vf",
        "showinfo",
        "-f",
        "null",
        "-loglevel",
        "debug",
        "-",
    ]

    pipe = subprocess.Popen(ff, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    while True:
        line = pipe.stdout.readline().strip().decode("utf-8")

        if len(line) == 0 and pipe.poll() is not None:
            break

        frames.append(line)

    return frames
