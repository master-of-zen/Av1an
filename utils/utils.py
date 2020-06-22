import atexit
import subprocess
import cv2
import re
import os
from pathlib import Path
from subprocess import PIPE
import statistics
import json
import sys
import shutil
from .logger import log, set_log_file
import numpy as np

def startup_check():
    if sys.version_info < (3, 6):
        print('Python 3.6+ required')
        sys.exit()
    if sys.platform == 'linux':
        def restore_term():
            os.system("stty sane")
        atexit.register(restore_term)

def terminate():
        os.kill(os.getpid(), 9)

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

    if len(inputs) > 1:
        return inputs, None
    else:
        return None, inputs[0]

def get_keyframes(file):
    """ Read file info and return list of all keyframes """
    keyframes = []

    ff = ["ffmpeg", "-hide_banner", "-i", file,
    "-vf", "select=eq(pict_type\,PICT_TYPE_I)",
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


def get_cq(command):
    """Return cq values from command"""
    matches = re.findall(r"--cq-level= *([^ ]+?) ", command)
    return int(matches[-1])


def man_cq(command: str, cq: int):
    """Return command with new cq value"""
    mt = '--cq-level='
    cmd = command[:command.find(mt) + 11] + str(cq) + command[command.find(mt) + 13:]
    return cmd


def frame_probe_fast(source: Path):
    video = cv2.VideoCapture(source.as_posix())
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    return total


def frame_probe(source: Path):
    """Get frame count."""
    cmd = ["ffmpeg", "-hide_banner", "-i", source.absolute(), "-map", "0:v:0", "-f", "null", "-"]
    r = subprocess.run(cmd, stdout=PIPE, stderr=PIPE)
    matches = re.findall(r"frame=\s*([0-9]+)\s", r.stderr.decode("utf-8") + r.stdout.decode("utf-8"))
    return int(matches[-1])


def frame_check(source: Path, encoded: Path, temp, check):
        """Checking is source and encoded video frame count match."""
        try:
            status_file = Path(temp / 'done.json')
            with status_file.open() as f:
                d = json.load(f)

            if check:
                s1 = frame_probe(source)
                d['done'][source.name] = s1
                with status_file.open('w') as f:
                    json.dump(d, f)
                    return

            s1, s2 = [frame_probe(i) for i in (source, encoded)]

            if s1 == s2:
                d['done'][source.name] = s1
                with status_file.open('w') as f:
                    json.dump(d, f)
            else:
                print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')
        except IndexError:
            print('Encoding failed, check validity of your encoding settings/commands and start again')
            terminate()
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'\nError frame_check: {e}\nAt line: {exc_tb.tb_lineno}\n')


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
