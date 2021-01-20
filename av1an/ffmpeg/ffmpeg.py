#!/bin/env python

import re
import subprocess
from pathlib import Path
from subprocess import PIPE, STDOUT
from typing import List
from av1an.logger import log

# TODO: redo to module, add ffmpeg scenedetection for fallback

def frame_probe_ffmpeg(source: Path):
    """
    Get frame count.
    Direct counting of frame count using ffmpeg. Slow, Precise.
    :param: source: Path to input file
    """
    cmd = ["ffmpeg", "-hide_banner", "-i", source.as_posix(), "-map", "0:v:0", "-f", "null", "-"]
    r = subprocess.run(cmd, stdout=PIPE, stderr=PIPE)
    matches = re.findall(r"frame=\s*([0-9]+)\s", r.stderr.decode("utf-8") + r.stdout.decode("utf-8"))
    return int(matches[-1])


def get_frametypes(file: Path) -> List:
    """
    Read file and return list with all frame types
    :param file: Path for file
    :return: list with sequence of frame types
    """

    frames = []

    ff = ["ffmpeg", "-hide_banner", "-i", file.as_posix(),
          "-vf", "showinfo", "-f", "null", "-loglevel", "debug", "-"]

    pipe = subprocess.Popen(ff, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    while True:
        line = pipe.stdout.readline().strip().decode("utf-8")

        if len(line) == 0 and pipe.poll() is not None:
            break

        frames.append(line)

    return frames


def get_keyframes(file: Path) -> List[int]:
    """
    Read file info and return list of all keyframes

    :param file: Path for input file
    :return: list with frame numbers of keyframes
    """

    keyframes = []

    ff = ["ffmpeg", "-hide_banner", "-i", file.as_posix(),
          "-vf", r"select=eq(pict_type\,PICT_TYPE_I)",
          "-f", "null", "-loglevel", "debug", "-"]

    pipe = subprocess.Popen(ff, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    while True:
        line = pipe.stdout.readline().strip().decode("utf-8")

        if len(line) == 0 and pipe.poll() is not None:
            break

        match = re.search(r"n:([0-9]+)\.[0-9]+ pts:.+key:1", line)
        if match:
            keyframe = int(match.group(1))
            keyframes.append(keyframe)

    return keyframes


def extract_audio(input_vid: Path, temp, audio_params):
    """Extracting audio from source, transcoding if needed."""
    log(f'Audio processing\nParams: {" ".join(audio_params)}\n')
    audio_file = temp / 'audio.mkv'

    # Checking is source have audio track
    check = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-ss', '0', '-i', input_vid.as_posix(), '-t', '0',
             '-vn', '-c:a', 'copy', '-f', 'null', '-']
    is_audio_here = len(subprocess.run(check, stdout=PIPE, stderr=STDOUT).stdout) == 0

    # If source have audio track - process it
    if is_audio_here:
        cmd = ('ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', input_vid.as_posix(), '-map_metadata', '-1',
            '-dn', '-vn', *audio_params, audio_file.as_posix())
        subprocess.run(cmd)
