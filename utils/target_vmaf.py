#!/usr/bin/env python3
import subprocess
from pathlib import Path
import numpy as np
from scipy import interpolate
from matplotlib import pyplot as plt
from math import isnan


def x264_probes(video: Path, ffmpeg: str):
    cmd = f' ffmpeg -y -hide_banner -loglevel error -i {video.as_posix()} ' \
                  f'-r 4 -an {ffmpeg} -c:v libx264 -crf 0 {video.with_suffix(".mp4")}'
    subprocess.run(cmd, shell=True)


def encoding_fork(min_cq, max_cq, steps):
    # Make encoding fork
    q = list(np.unique(np.linspace(min_cq, max_cq, num=steps, dtype=int, endpoint=True)))

    # Moving highest cq to first check, for early skips
    # checking highest first, lowers second, for early skips
    q.insert(0, q.pop(-1))
    return q


def vmaf_probes(probe, fork, ffmpeg):
    params = " aomenc  -q --passes=1 --threads=8 --end-usage=q --cpu-used=6 --cq-level="
    cmd = [[f'ffmpeg -y -hide_banner -loglevel error -i {probe} {ffmpeg}'
            f'{params}{x} -o {probe.with_name(f"v_{x}{probe.stem}")}.ivf - ',
            probe, probe.with_name(f'v_{x}{probe.stem}').with_suffix('.ivf'), x] for x in fork]
    return cmd


def interpolate_data(vmaf_cq: list, vmaf_target):
    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    # Interpolate data
    f = interpolate.interp1d(x, y, kind='cubic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))

    # Getting value closest to target
    tl = list(zip(xnew, f(xnew)))
    vmaf_target_cq = min(tl, key=lambda x: abs(x[1] - vmaf_target))
    return vmaf_target_cq, tl, f, xnew


def plot_probes(x, y, f, tl, min_cq, max_cq, probe, xnew, vmaf_target_cq, frames):
    # Saving plot of vmaf calculation
    plt.plot(x, y, 'x', color='tab:blue')
    plt.plot(xnew, f(xnew), color='tab:blue')
    plt.plot(vmaf_target_cq[0], vmaf_target_cq[1], 'o', color='red')

    for x in range(0, 100):
        plt.axhline(x, color='grey', linewidth=0.4)
        plt.axvline(x, color='grey', linewidth=0.3)

    if x % 5 == 0:
        plt.axhline(x, color='black', linewidth=0.6)

    plt.xlim(min_cq, max_cq)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.xlabel('CQ')
    plt.title(f'Chunk: {probe.stem}, Frames: {frames}')
    plt.tight_layout()
    temp =  temp / probe.stem
    plt.savefig(temp, dpi=300)
    plt.close()