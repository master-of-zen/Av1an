#!/bin/env python

from utils.utils import terminate, frame_probe
from utils.vmaf import call_vmaf, read_vmaf_xml
from scipy import interpolate
from pathlib import Path
import subprocess
import numpy as np
from utils.logger import log
from matplotlib import pyplot as plt
import matplotlib
import sys
from math import isnan


def x264_probes(video: Path, ffmpeg: str):
    cmd = f' ffmpeg -y -hide_banner -loglevel error -i {video.as_posix()} ' \
                  f'-r 4 -an {ffmpeg} -c:v libx264 -crf 0 {video.with_suffix(".mp4")}'
    subprocess.run(cmd, shell=True)


def encoding_fork(min_cq, max_cq, steps):
    """Makin list of Q values for probing. Highest Q first check, lowest Q second, for early skips
    """
    q = list(np.unique(np.linspace(min_cq, max_cq, num=steps, dtype=int, endpoint=True)))
    q.insert(0, q.pop(-1))
    return q


def gen_probes_names(probe, q):
    """Make name of vmaf probe
    """
    return probe.with_name(f'v_{q}{probe.stem}').with_suffix('.ivf')


def probe_cmd(probe, q, ffmpeg_pipe, encoder):
    """Generate and return commands for probes at set Q values
    """

    pipe = f'ffmpeg -y -hide_banner -loglevel error -i {probe} {ffmpeg_pipe}'

    if encoder == 'aom':
        params = " aomenc  -q --passes=1 --threads=8 --end-usage=q --cpu-used=6 --cq-level="
    elif encoder == 'x265':
        params = "x265  --log-level 0  --no-progress --y4m --preset faster --crf "

    cmd = f'{pipe} {params}{q} -o {probe.with_name(f"v_{q}{probe.stem}")}.ivf - '

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


def plot_probes(args, x, y, vmaf_cq, vmaf_target, probe, xnew, frames):
    # Saving plot of vmaf calculation
    cq, tl, f, xnew = interpolate_data(vmaf_cq, args.vmaf_target)
    matplotlib.use('agg')
    plt.ioff()
    plt.plot(x, y, 'x', color='tab:blue', alpha=1)
    plt.plot(xnew, f(xnew), color='tab:blue', alpha=1)
    plt.plot(cq[0], cq[1], 'o', color='red', alpha=1)
    plt.grid(True)
    plt.xlim(args.min_cq, args.max_cq)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.title(f'Chunk: {probe.stem}, Frames: {frames}')
    # plt.tight_layout()
    temp = args.temp / probe.stem
    plt.tight_layout()
    plt.savefig(temp, dpi=300, format='png',transparent=True)
    plt.close()


def target_vmaf(source, args):

    frames = frame_probe(source)
    probe = source.with_suffix(".mp4")

    try:
        # Making 4 fps probing file
        x264_probes(source, args.ffmpeg)

        # Making encoding fork
        fork = encoding_fork(args.min_cq, args.max_cq, args.vmaf_steps)

        # Encoding probe and getting vmaf
        vmaf_cq = []
        for i in range(args.vmaf_steps):

            cmd = probe_cmd(probe, fork[i], args.ffmpeg_pipe, args.encoder)
            subprocess.run(cmd, shell=True)

            v = call_vmaf(probe, gen_probes_names(probe, fork[i]), n_threads=args.n_threads, model=args.vmaf_path, return_file=True)
            mean = read_vmaf_xml(v, 25)

            vmaf_cq.append((mean, fork[i]))

            # Early Skip on big CQ
            if i == 0 and round(mean) > args.vmaf_target:
                log(f"File: {source.stem}, Fr: {frames}\n" \
                    f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n" \
                    f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                    f"Target Q: {args.max_cq} Vmaf: {mean}\n\n")

                return args.max_cq

            # Early Skip on small CQ
            if i == 1 and round(mean) < args.vmaf_target:
                log(f"File: {source.stem}, Fr: {frames}\n" \
                    f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n" \
                    f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                    f"Target Q: {args.min_cq} Vmaf: {mean}\n\n")

                return args.min_cq

        x = [x[1] for x in sorted(vmaf_cq)]
        y = [float(x[0]) for x in sorted(vmaf_cq)]

        # Interpolate data
        cq, _, _, xnew = interpolate_data(vmaf_cq, args.vmaf_target)

        if args.vmaf_plots:
            plot_probes(args, x, y, vmaf_cq, args.vmaf_target, probe, xnew, frames)

        log(f'File: {source.stem}, Fr: {frames}\n' \
            f'Q: {sorted([x[1] for x in vmaf_cq])}\n' \
            f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n' \
            f'Target Q: {int(cq[0])} Vmaf: {round(float(cq[1]), 2)}\n\n')

        return int(cq[0])

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in vmaf_target {e} \nAt line {exc_tb.tb_lineno}')
        terminate()
