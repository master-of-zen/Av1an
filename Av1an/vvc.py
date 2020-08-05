#! /bin/env python

import subprocess
from pathlib import Path
from .logger import log

def to_yuv(file: Path):
    output = file.with_suffix('.yuv')
    cmd = f'ffmpeg -y -loglevel error -i {file.as_posix()} -f rawvideo -vf format=yuv420p10le {output.as_posix()}'
    subprocess.run(cmd, shell=True)
    return output
