#! /bin/env python

import subprocess
from subprocess import PIPE, STDOUT
from pathlib import Path

from .chunk import Chunk


def get_yuv_file_path(chunk: Chunk) -> Path:
    """
    Gets the yuv path to be used for a given chunk

    :param chunk: the Chunk
    :return: a yuv file path for the chunk
    """
    return (chunk.temp / 'split') / f'{chunk.name}.yuv'


def to_yuv(chunk: Chunk) -> Path:
    """
    Generates a yuv file for a given chunk

    :param chunk: the Chunk
    :return: a yuv file path for the chunk
    """
    output = get_yuv_file_path(chunk)

    ffmpeg_gen_pipe = subprocess.Popen(chunk.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)

    # TODO: apply ffmpeg filter to the yuv file
    cmd = f'ffmpeg -y -loglevel error -i - -f rawvideo -vf format=yuv420p10le {output.as_posix()}'
    pipe = subprocess.Popen(cmd.split(), stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT, universal_newlines=True)
    pipe.wait()

    return output
