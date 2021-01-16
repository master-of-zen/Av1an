import re
import subprocess
import sys
from subprocess import Popen

from av1an.logger import log
from av1an.vapoursynth import compose_vapoursynth_pipe

if sys.platform == "linux":
        from os import mkfifo


def ffmpeg(video, threshold, min_scene_len, total_frames, is_vs, temp):
    """
    Running FFMPEG detection on source video for segmenting.
    Usually the optimal threshold is 0.1 - 0.3 but it can vary a lot
    based on your source content.

    Threshold value increased by x100 for matching with pyscene range
    """


    log(f'Starting FFMPEG detection:\nThreshold: {threshold}, \nIs Vapoursynth input: {is_vs}\n')
    scenes = []
    frame:int = 0

    if is_vs:
        if sys.platform == "linux":
            vspipe_fifo = temp / 'vspipe.y4m'
            mkfifo(vspipe_fifo)
        else:
            vspipe_fifo = None

        vspipe_cmd = compose_vapoursynth_pipe(video, vspipe_fifo)
        vspipe_process = Popen(vspipe_cmd)

    cmd = ['ffmpeg', '-hwaccel', 'auto','-hide_banner', '-i',  str(vspipe_fifo if is_vs else video.as_posix()), '-an', '-sn', '-vf', 'scale=\'min(960,iw):-1:flags=neighbor\',select=\'gte(scene,0)\',metadata=print', '-f', 'null', '-']
    pipe = Popen(cmd,
           stdout=subprocess.PIPE,
           stderr=subprocess.STDOUT,
           universal_newlines=True)

    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break
        if len(line) == 0:
            continue

        if 'frame' in line:
            match = re.findall(r':(\d+)', line)
            if match:
                frame = int(match[0])
                continue

        if 'score' in line:
            matches = re.findall(r"=\s*([\S\s]+)", line)
            if matches:
                score = float(matches[-1]) * 100
                if score > threshold and frame - max(scenes, default=0) > min_scene_len:
                    scenes.append(frame)

    if pipe.returncode != 0 and pipe.returncode != -2:
        print(f"\n:: Error in ffmpeg scenedetection {pipe.returncode}")
        print('\n'.join(scenes))


    if is_vs:
        vspipe_process.wait()

    log(f'Found split points: {len(scenes)}\n')
    log(f'Splits: {scenes}\n')

    return scenes