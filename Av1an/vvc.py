#! /bin/env python

import subprocess
from pathlib import Path
from .logger import log

def to_yuv(file: Path):
    output = file.with_suffix('.yuv')
    cmd = f'ffmpeg -y -loglevel error -i {file.as_posix()} -f rawvideo -vf format=yuv420p10le {output.as_posix()}'
    subprocess.run(cmd, shell=True)
    return output


def vvc_concat(temp: Path, output: Path):
    encode_files = sorted((temp / 'encode').iterdir())
    bitstreams = [x.as_posix() for x in encode_files]
    bitstreams = ' '.join(bitstreams)
    cmd = f'vvc_concat  {bitstreams} {output.as_posix()}'

    output = subprocess.run(cmd, shell=True)

