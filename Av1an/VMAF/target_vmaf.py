#!/bin/env python

from math import isnan

import subprocess
from subprocess import STDOUT, PIPE

import matplotlib
from matplotlib import pyplot as plt

import numpy as np
from scipy import interpolate

from Av1an.arg_parse import Args
from Av1an.bar import process_pipe
from Av1an.chunk import Chunk
from Av1an.commandtypes import CommandPair, Command
from Av1an.logger import log
from .vmaf import call_vmaf, read_vmaf_json


def target_vmaf_routine(args: Args, chunk: Chunk):
    """
    Applies target vmaf to this chunk. Determines what the cq value should be and sets the
    vmaf_target_cq for this chunk

    :param args: the Args
    :param chunk: the Chunk
    :return: None
    """
    chunk.vmaf_target_cq = target_vmaf(chunk, args)


def gen_probes_names(chunk: Chunk, q):
    """Make name of vmaf probe
    """
    return chunk.fake_input_path.with_name(f'v_{q}{chunk.name}').with_suffix('.ivf')


def probe_cmd(chunk: Chunk, q, ffmpeg_pipe, encoder, vmaf_rate) -> CommandPair:
    """
    Generate and return commands for probes at set Q values
    These are specifically not the commands that are generated
    by the user or encoder defaults, since these
    should be faster than the actual encoding commands.
    These should not be moved into encoder classes at this point.
    """
    pipe = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', '-vf', f'select=not(mod(n\\,{vmaf_rate}))',
            *ffmpeg_pipe]

    probe_name = gen_probes_names(chunk, q).with_suffix('.ivf').as_posix()

    if encoder == 'aom':
        params = ['aomenc', '--passes=1', '--threads=8',
                  '--end-usage=q', '--cpu-used=6', f'--cq-level={q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'x265':
        params = ['x265', '--log-level', '0', '--no-progress',
                  '--y4m', '--preset', 'faster', '--crf', f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'rav1e':
        params = ['rav1e', '-s', '10', '--tiles', '8', '--quantizer', f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'vpx':
        params = ['vpxenc', '--passes=1', '--pass=1', '--codec=vp9',
                  '--threads=8', '--cpu-used=9', '--end-usage=q',
                  f'--cq-level={q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'svt_av1':
        params = ['SvtAv1EncApp', '-i', 'stdin',
                  '--preset', '8', '--rc', '0', '--qp', f'{q}']
        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])

    elif encoder == 'svt_vp9':
        params = ['SvtVp9EncApp', '-i', 'stdin',
                  '-enc-mode', '8', '-q', f'{q}']
        # TODO: pipe needs to output rawvideo
        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])

    elif encoder == 'x264':
        params = ['x264', '--log-level', 'error', '--demuxer', 'y4m',
                  '-', '--no-progress', '--preset', 'slow', '--crf',
                  f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    return cmd


def make_pipes(ffmpeg_gen_cmd: Command, command: CommandPair):

    ffmpeg_gen_pipe = subprocess.Popen(ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)
    ffmpeg_pipe = subprocess.Popen(command[0], stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(command[1], stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                            stderr=STDOUT,
                            universal_newlines=True)

    return pipe


def get_target_q(scores, vmaf_target):
    """
    Interpolating scores to get Q closest to target VMAF
    Interpolation type for 2 probes changes to linear
    """
    x = [x[1] for x in sorted(scores)]
    y = [float(x[0]) for x in sorted(scores)]

    if len(x) > 2:
        interpolation = 'quadratic'
    else:
        interpolation = 'linear'
    f = interpolate.interp1d(x, y, kind=interpolation)
    xnew = np.linspace(min(x), max(x), max(x) - min(x))
    tl = list(zip(xnew, f(xnew)))
    q = min(tl, key=lambda l: abs(l[1] - vmaf_target))

    return int(q[0]), round(q[1], 3)


def interpolate_data(vmaf_cq: list, vmaf_target):
    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    # Interpolate data
    f = interpolate.interp1d(x, y, kind='quadratic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))

    # Getting value closest to target
    tl = list(zip(xnew, f(xnew)))
    vmaf_target_cq = min(tl, key=lambda l: abs(l[1] - vmaf_target))
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


def vmaf_probe(chunk: Chunk, q, args: Args):
    """
    Make encoding probe to get VMAF that Q returns

    :param chunk: the Chunk
    :param q: Value to make probe
    :param args: the Args
    :return :
    """


    cmd = probe_cmd(chunk, q, args.ffmpeg_pipe, args.encoder, args.vmaf_rate)
    pipe = make_pipes(chunk.ffmpeg_gen_cmd, cmd)
    process_pipe(pipe)

    file = call_vmaf(chunk, gen_probes_names(chunk, q), args.n_threads, args.vmaf_path, args.vmaf_res,
                     vmaf_rate=args.vmaf_rate)
    score = read_vmaf_json(file, 20)

    return score


def get_closest(q_list, q, positive=True):
    """
    Returns closest value from the list, ascending or descending

    :param q_list: list of q values that been already used
    :param q:
    :param positive: search direction, positive - only values bigger than q
    :return: q value from list
    """
    if positive:
        q_list = [x for x in q_list if x > q]
    else:
        q_list = [x for x in q_list if x < q]

    return min(q_list, key=lambda x: abs(x - q))


def weighted_search(num1, vmaf1, num2, vmaf2, target):
    """
    Returns weighted value closest to searched

    :param num1: Q of first probe
    :param vmaf1: VMAF of first probe
    :param num2: Q of second probe
    :param vmaf2: VMAF of first probe
    :param target: VMAF target
    :return: Q for new probe
    """

    dif1 = abs(target - vmaf2)
    dif2 = abs(target - vmaf1)

    tot = dif1 + dif2

    new_point = int(round(num1 * (dif1 / tot) + (num2 * (dif2 / tot))))
    return new_point


def target_vmaf(chunk: Chunk, args: Args):
    vmaf_cq = []
    frames = chunk.frames
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
        log(f"Chunk: {chunk.name}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} Vmaf: {vmaf_cq[-1][0]}\n\n")
        return next_q

    elif next_q == args.max_q and score > args.vmaf_target:
        log(f"Chunk: {chunk.name}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} Vmaf: {vmaf_cq[-1][0]}\n\n")
        return next_q

    # VMAF search
    for _ in range(args.vmaf_steps - 2):
        new_point = weighted_search(vmaf_cq[-2][1], vmaf_cq[-2][0], vmaf_cq[-1][1], vmaf_cq[-1][0], args.vmaf_target)
        if new_point in [x[1] for x in vmaf_cq]:
            break
        last_q = new_point

        q_list.append(new_point)
        score = vmaf_probe(chunk, new_point, args)
        next_q = get_closest(q_list, last_q, positive=score >= args.vmaf_target)
        vmaf_cq.append((score, new_point))

    q, q_vmaf = get_target_q(vmaf_cq, args.vmaf_target)

    log(f'Chunk: {chunk.name}, Fr: {frames}\n'
        f'Q: {sorted([x[1] for x in vmaf_cq])}\n'
        f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n'
        f'Target Q: {q} Vmaf: {q_vmaf}\n\n')

    # Plot Probes
    if args.vmaf_plots and len(vmaf_cq) > 3:
        plot_probes(args, vmaf_cq, chunk, frames)

    return q

