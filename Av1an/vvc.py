#! /bin/env python

import subprocess
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
    # TODO: could cause problems with windows not really supporting pipes
    cmd = f'{chunk.ffmpeg_gen_cmd} | ffmpeg -y -loglevel error -i - -f rawvideo -vf format=yuv420p10le {output.as_posix()}'
    subprocess.run(cmd, shell=True)
    return output
