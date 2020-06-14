import numpy as np
import subprocess
import cv2
import re
import os
from pathlib import Path
from math import isnan
from subprocess import PIPE, STDOUT
import statistics
from ast import literal_eval
from psutil import virtual_memory

def terminate():
        os.kill(os.getpid(), 9)


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        return round(min(cpu / 2, ram / 1.5))

    elif encoder == 'svt_av1':
        return round(min(cpu, ram)) // 5

    # fix if workers round up to 0
    if workers == 0:
        return 1


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


def reduce_scenes(scenes):
    """Windows terminal can't handle more than ~500 scenes in length."""
    count = len(scenes)
    interval = int(count / 500 + (count % 500 > 0))
    scenes = scenes[::interval]
    return scenes


def extra_splits(video, frames: list, split_distance):
    frames.append(frame_probe(video))
    # Get all keyframes of original video
    keyframes = get_keyframes(video)

    t = frames[:]
    t.insert(0, 0)
    splits = list(zip(t, frames))
    for i in splits:
        # Getting distance between splits
        distance = (i[1] - i[0])

        if distance > split_distance:
            # Keyframes that between 2 split points
            candidates = [k for k in keyframes if i[1] > k > i[0]]

            if len(candidates) > 0:
                # Getting number of splits that need to be inserted
                to_insert = min((i[1] - i[0]) // split_distance, (len(candidates)))
                for k in range(0, to_insert):
                    # Approximation of splits position
                    aprox_to_place = (((k + 1) * distance) // (to_insert + 1)) + i[0]

                    # Getting keyframe closest to approximated
                    key = min(candidates, key=lambda x: abs(x - aprox_to_place))
                    frames.append(key)
    result = [int(x) for x in sorted(frames)]
    return result