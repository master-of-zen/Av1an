#!/usr/bin/env python3
import subprocess
from pathlib import Path
import numpy as np

def make_probes(video: Path, ffmpeg: str):
    cmd = f' ffmpeg -y -hide_banner -loglevel error -i {video.as_posix()} ' \
                  f'-r 4 -an {ffmpeg} -c:v libx264 -crf 0 {video.with_suffix(".mp4")}'
    subprocess.run(cmd, shell=True)

def make_encoding_fork(min_cq, max_cq, steps):
    # Make encoding fork
    q = list(np.unique(np.linspace(min_cq, max_cq, num=steps, dtype=int, endpoint=True)))

    # Moving highest cq to first check, for early skips
    # checking highest first, lowers second, for early skips
    q.insert(0, q.pop(-1))
    return q

def make_vmaf_probes(probe, fork, ffmpeg):
    params = " aomenc  -q --passes=1 --threads=8 --end-usage=q --cpu-used=6 --cq-level="
    cmd = [[f'ffmpeg -y -hide_banner -loglevel error -i {probe} {ffmpeg}'
            f'{params}{x} -o {probe.with_name(f"v_{x}{probe.stem}")}.ivf - ',
            probe, probe.with_name(f'v_{x}{probe.stem}').with_suffix('.ivf'), x] for x in fork]
    return cmd

def interpolate_data():
    pass