import numpy as np
import subprocess
import cv2
import re
from pathlib import Path
from math import isnan
from subprocess import PIPE, STDOUT
import statistics

def read_vmaf_xml(file):
    with open(file, 'r') as f:
        file = f.readlines()
        file = [x.strip() for x in file if 'vmaf="' in x]
        vmafs = []
        for i in file:
            vmf = i[i.rfind('="') + 2: i.rfind('"')]
            vmafs.append(float(vmf))

        vmafs = [round(float(x), 5) for x in vmafs if isinstance(x, float)]
        calc = [x for x in vmafs if isinstance(x, float) and not isnan(x)]
        mean = round(sum(calc) / len(calc), 2)
        perc_1 = round(np.percentile(calc, 1), 2)
        perc_25 = round(np.percentile(calc, 25), 2)
        perc_75 = round(np.percentile(calc, 75), 2)

        return vmafs, mean, perc_1, perc_25, perc_75

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
    """Windows terminal can't handle more than ~600 scenes in length."""
    count = len(scenes)
    interval = int(count / 600 + (count % 600 > 0))
    scenes = scenes[::interval]
    return scenes