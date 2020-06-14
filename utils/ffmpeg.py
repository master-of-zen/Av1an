#!/bin/env python

import os
import subprocess
from subprocess import PIPE, STDOUT
import shutil

def split(video, temp, frames):
    """Split video by frame numbers, or just copying video."""

    cmd = [
        "ffmpeg", "-hide_banner", "-y",
        "-i", video.absolute().as_posix(),
        "-map", "0:v:0",
        "-an",
        "-c", "copy",
        "-avoid_negative_ts", "1"
    ]

    if len(frames) > 0:
        cmd.extend([
            "-f", "segment",
            "-segment_frames", ','.join([str(x) for x in frames])
        ])
    cmd.append(os.path.join(temp, "split", "%05d.mkv"))
    pipe = subprocess.Popen(cmd, stdout=PIPE, stderr=STDOUT)
    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break
    
def concatenate_video(temp, output, keep=False):
    """With FFMPEG concatenate encoded segments into final file."""
    with open(f'{temp / "concat" }', 'w') as f:

        encode_files = sorted((temp / 'encode').iterdir())
        f.writelines(f"file '{file.absolute()}'\n" for file in encode_files)

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
