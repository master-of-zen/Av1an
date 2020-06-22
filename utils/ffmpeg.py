#!/bin/env python

import os
import subprocess
from subprocess import PIPE, STDOUT
import shutil
from pathlib import Path
from .logger import log, set_log_file


def concatenate_video(temp, output, keep=False):
    """With FFMPEG concatenate encoded segments into final file."""

    log('Concatenating\n')

    with open(f'{temp / "concat" }', 'w') as f:

        encode_files = sorted((temp / 'encode').iterdir())
        # Replace all the ' with '/'' so ffmpeg can read the path correctly
        f.writelines("file '" + str(file.absolute()).replace('\'','\'\\\'\'') + "'\n" for file in encode_files)

    # Add the audio file if one was extracted from the input
    audio_file = temp / "audio.mkv"
    if audio_file.exists():
        audio = f'-i {audio_file} -c:a copy'
    else:
        audio = ''

    cmd = f' ffmpeg -y -hide_banner -loglevel error -f concat -safe 0 -i {temp / "concat"} ' \
          f'{audio} -c copy -y "{output}"'
    concat = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT).stdout
    if len(concat) > 0:
        raise Exception

    # Delete temp folders
    if not keep:
        shutil.rmtree(temp)


def extract_audio(input_vid: Path, temp, audio_params):
    """Extracting audio from source, transcoding if needed."""
    log(f'Audio processing\nParams: {audio_params}\n')
    audio_file = temp / 'audio.mkv'

    # Checking is source have audio track
    check = fr' ffmpeg -y -hide_banner -loglevel error -ss 0 -i "{input_vid}" -t 0 -vn -c:a copy -f null -'
    is_audio_here = len(subprocess.run(check, shell=True, stdout=PIPE, stderr=STDOUT).stdout) == 0

    # If source have audio track - process it
    if is_audio_here:
        cmd = f'ffmpeg -y -hide_banner -loglevel error -i "{input_vid}" -vn ' \
              f'{audio_params} {audio_file}'
        subprocess.run(cmd, shell=True)