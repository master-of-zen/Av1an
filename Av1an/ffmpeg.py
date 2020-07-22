#!/bin/env python

import json
import subprocess
from pathlib import Path
from subprocess import PIPE, STDOUT
from .utils import terminate
from .logger import log
from threading import Lock
import re
import sys


def frame_probe(source: Path):
    """Get frame count."""
    cmd = ["ffmpeg", "-hide_banner", "-i", source.as_posix(), "-map", "0:v:0", "-f", "null", "-"]
    r = subprocess.run(cmd, stdout=PIPE, stderr=PIPE)
    matches = re.findall(r"frame=\s*([0-9]+)\s", r.stderr.decode("utf-8") + r.stdout.decode("utf-8"))
    return int(matches[-1])


def get_keyframes(file: Path):
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


doneFileLock = Lock()
def write_progress_file(file, chunk, frames):
    doneFileLock.acquire()
    with file.open() as f:
        d = json.load(f)
    d['done'][chunk.name] = frames
    with file.open('w') as f:
        json.dump(d, f)


def frame_check(source: Path, encoded: Path, temp, check):
    """Checking is source and encoded video frame count match."""
    try:
        status_file = Path(temp / 'done.json')

        if check:
            s1 = frame_probe(source)
            write_progress_file(status_file, source, s1)
        else:
            s1, s2 = [frame_probe(i) for i in (source, encoded)]
            if s1 == s2:
                write_progress_file(status_file, source, s1)
            else:
                print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')

    except IndexError:
        print('Encoding failed, check validity of your encoding settings/commands and start again')
        terminate()
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'\nError frame_check: {e}\nAt line: {exc_tb.tb_lineno}\n')
    finally:
        if doneFileLock.locked():
            doneFileLock.release()


def concatenate_video(temp, output, encoder):
    """With FFMPEG concatenate encoded segments into final file."""

    log('Concatenating\n')

    with open(f'{temp / "concat" }', 'w') as f:

        encode_files = sorted((temp / 'encode').iterdir())
        # Replace all the ' with '/'' so ffmpeg can read the path correctly
        f.writelines("file '" + str(file.absolute()).replace('\'','\'\\\'\'') + "'\n" for file in encode_files)

    # Add the audio file if one was extracted from the input
    audio_file = temp / "audio.mkv"
    if audio_file.exists():
        audio = f'-i {audio_file} -c:a copy -map 1'
    else:
        audio = ''

    if encoder == 'x265':

        cmd = f' ffmpeg -y -fflags +genpts  -hide_banner -loglevel error -f concat -safe 0 -i {temp / "concat"} ' \
            f'{audio} -c copy -movflags frag_keyframe+empty_moov -map 0  -f mp4 - | ffmpeg -y -hide_banner -loglevel error -i - -c copy {output} '
        concat = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT).stdout

    else:
        cmd = f' ffmpeg -y -hide_banner -loglevel error -f concat -safe 0 -i {temp / "concat"} ' \
            f'{audio} -c copy -map 0  -y "{output}"'

        concat = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT).stdout


    if len(concat) > 0:
        log(concat.decode())
        print(concat.decode())
        raise Exception


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
