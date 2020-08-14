#!/bin/env python


from scipy import interpolate
from pathlib import Path
import subprocess
from subprocess import PIPE, STDOUT
import numpy as np
from matplotlib import pyplot as plt
import matplotlib
import sys
from math import isnan
import os
from collections import deque

from .arg_parse import Args
from .chunk import Chunk
from .utils import terminate, man_q
from .bar import make_pipes, process_pipe
from .utils import terminate
from .ffmpeg import frame_probe
from .vmaf import call_vmaf, read_vmaf_json
from .logger import log


def target_vmaf_routine(args: Args, chunk: Chunk):
    """
    Applies target vmaf to this chunk. Determines what the cq value should be and adjusts the pass_cmds
    to match

    :param args: the Args
    :param chunk: the Chunk
    :return: None
    """
    tg_cq = target_vmaf(chunk, args)
    chunk.pass_cmds = (man_q(command, tg_cq) for command in chunk.pass_cmds)


def gen_probes_names(chunk: Chunk, q):
    """Make name of vmaf probe
    """
    return chunk.fake_input_path.with_name(f'v_{q}{chunk.name}').with_suffix('.ivf')


def probe_cmd(chunk: Chunk, q, ffmpeg_pipe, encoder, vmaf_rate):
    """Generate and return commands for probes at set Q values
    """
    pipe = ('ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', '-vf', f'select=not(mod(n\,{vmaf_rate}))', *ffmpeg_pipe)

    probe_name = gen_probes_names(chunk, q).with_suffix('.ivf').as_posix()

    if encoder == 'aom':
        params = ('aomenc',  '--passes=1', '--threads=8', '--end-usage=q', '--cpu-used=6', f'--cq-level={q}')
        cmd = (pipe, (*params, '-o', probe_name, '-'))

    elif encoder == 'x265':
        params = ('x265',  '--log-level', '0', '--no-progress', '--y4m', '--preset', 'faster', '--crf', f'{q}')
        cmd = (pipe, (*params, '-o', probe_name, '-'))

    elif encoder == 'rav1e':
        params = ('rav1e', '-s', '10', '--tiles', '8', '--quantizer', f'{q}')
        cmd = (pipe, (*params, '-o', probe_name, '-'))

    elif encoder == 'vpx':
        params = ('vpxenc', '--passes=1', '--pass=1', '--codec=vp9', '--threads=4', '--cpu-used=9', '--end-usage=q', f'--cq-level={q}')
        cmd = (pipe, (*params, '-o', probe_name, '-'))

    elif encoder == 'svt_av1':
        params = ('SvtAv1EncApp', '-i', 'stdin', '--preset', '8', '--rc', '0', '--qp', f'{q}')
        cmd = (pipe, (*params, '-b', probe_name, '-'))

    elif encoder == 'x264':
        params = ('x264', '--log-level', 'error', '--demuxer', 'y4m', '-', '--no-progress', '--preset', 'slow', '--crf', f'{q}')
        cmd = (pipe, (*params, '-o', probe_name, '-'))

    return cmd


def get_target_q(scores, vmaf_target):
    x = [x[1] for x in sorted(scores)]
    y = [float(x[0]) for x in sorted(scores)]
    f = interpolate.interp1d(x, y, kind='quadratic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))
    tl = list(zip(xnew, f(xnew)))
    q = min(tl, key=lambda x: abs(x[1] - vmaf_target))

    return int(q[0]), round(q[1],3)


def interpolate_data(vmaf_cq: list, vmaf_target):
    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    # Interpolate data
    f = interpolate.interp1d(x, y, kind='quadratic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))

    # Getting value closest to target
    tl = list(zip(xnew, f(xnew)))
    vmaf_target_cq = min(tl, key=lambda x: abs(x[1] - vmaf_target))
    return vmaf_target_cq, tl, f, xnew


def plot_probes(args, vmaf_cq, chunk: Chunk, frames):
    # Saving plot of vmaf calculation

    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    cq, tl, f, xnew = interpolate_data(vmaf_cq, args.vmaf_target)
    matplotlib.use('agg')
    plt.ioff()
    plt.plot(xnew, f(xnew), color='tab:blue', alpha=1)
    plt.plot(x, y, 'p', color='tab:green', alpha=1)
    plt.plot(cq[0], cq[1], 'o', color='red', alpha=1)
    plt.grid(True)
    plt.xlim(args.min_q, args.max_q)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.title(f'Chunk: {chunk.name}, Frames: {frames}')
    plt.xticks(np.arange(args.min_q, args.max_q + 1, 1.0))
    temp = args.temp / chunk.name
    plt.savefig(f'{temp}.png', dpi=200, format='png')
    plt.close()


def vmaf_probe(chunk: Chunk, q, args):

    cmd = probe_cmd(chunk, q, args.ffmpeg_pipe, args.encoder, args.vmaf_rate)

    pipe = make_pipes(chunk.ffmpeg_gen_cmd, cmd)
    process_pipe(pipe)

    file = call_vmaf(chunk, gen_probes_names(chunk, q), args.n_threads, args.vmaf_path, args.vmaf_res, vmaf_rate=args.vmaf_rate)
    score = read_vmaf_json(file, 20)

    return score


def get_closest(q_list, q, positive=True):
    """Returns closest value from the list, ascending or descending
    """
    if positive:
        q_list = [x for x in q_list if x > q]
    else:
        q_list = [x for x in q_list if x < q]

    return min(q_list, key=lambda x:abs(x-q))


def weighted_search(num1, vmaf1, num2, vmaf2, target):
    """
    Returns weighted value closest to searched
    """
    dif1 = abs(target - vmaf2)
    dif2 = abs(target - vmaf1)

    tot = dif1 + dif2

    new_point = int(round(num1 * (dif1 / tot ) + (num2 * (dif2 / tot))))
    return new_point


def target_vmaf_search(chunk: Chunk, frames, args):

    vmaf_cq = []
    q_list = []
    score = 0

    # Make middle probe
    middle_point = (args.min_q + args.max_q) // 2
    q_list.append(middle_point)
    last_q = middle_point

    score = vmaf_probe(chunk, last_q, args)
    vmaf_cq.append((score, last_q))

    # Branch
    if score < args.vmaf_target:
        next_q = args.min_q
        q_list.append(args.min_q)
    else:
        next_q = args.max_q
        q_list.append(args.max_q)

    # Edge case check
    score = vmaf_probe(chunk, next_q, args)
    vmaf_cq.append((score, next_q))

    if next_q == args.min_q and score < args.vmaf_target:
        return vmaf_cq, True

    elif next_q == args.max_q and score > args.vmaf_target:
        return vmaf_cq, True

    for _ in range(args.vmaf_steps - 2 ):
        new_point = weighted_search(vmaf_cq[-2][1], vmaf_cq[-2][0], vmaf_cq[-1][1], vmaf_cq[-1][0], args.vmaf_target)
        if new_point in [x[1] for x in vmaf_cq]:
            return vmaf_cq, False

        last_q = new_point

        q_list.append(new_point)
        score = vmaf_probe(chunk, new_point, args)
        next_q = get_closest(q_list, last_q, positive=score >= args.vmaf_target)
        vmaf_cq.append((score, new_point))

    return vmaf_cq, False


def target_vmaf(chunk: Chunk, args: Args):

    frames = chunk.frames
    vmaf_cq = []

    try:
        vmaf_cq, skip = target_vmaf_search(chunk, frames, args)
        if skip or len(vmaf_cq) == 2:
            if vmaf_cq[-1][1] == args.max_q:
                log(f"Chunk: {chunk.name}, Fr: {frames}\n" \
                    f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n" \
                    f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                    f"Target Q: {args.max_q} Vmaf: {vmaf_cq[-1][0]}\n\n")

            else:
                log(f"Chunk: {chunk.name}, Fr: {frames}\n" \
                    f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n" \
                    f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                    f"Target Q: {args.min_q} Vmaf: {vmaf_cq[-1][0]}\n\n")

            return vmaf_cq[-1][1]


        q, q_vmaf = get_target_q(vmaf_cq, args.vmaf_target )

        log(f'Chunk: {chunk.name}, Fr: {frames}\n' \
            f'Q: {sorted([x[1] for x in vmaf_cq])}\n' \
            f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n' \
            f'Target Q: {q} Vmaf: {q_vmaf}\n\n')

        if args.vmaf_plots:
            plot_probes(args, vmaf_cq, chunk, frames)

        return q

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in vmaf_target {e} \nAt line {exc_tb.tb_lineno}')
        terminate()
