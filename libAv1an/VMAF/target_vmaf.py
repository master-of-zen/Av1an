#!/bin/env python

from math import log as ln

import subprocess
from subprocess import STDOUT, PIPE

import numpy as np
from scipy import interpolate

from libAv1an.LibAv1an.args import Args
from libAv1an.LibAv1an.run_cmd import process_pipe
from libAv1an.Chunks.chunk import Chunk
from libAv1an.LibAv1an.commandtypes import CommandPair, Command
from libAv1an.LibAv1an.callbacks import Callbacks
from libAv1an.VMAF.vmaf import call_vmaf, read_weighted_vmaf


def target_vmaf_routine(args: Args, chunk: Chunk, cb: Callbacks):
    """
    Applies target vmaf to this chunk. Determines what the cq value should be and sets the
    vmaf_target_cq for this chunk

    :param args: the Args
    :param chunk: the Chunk
    :param cb: Callback data
    :return: None
    """
    chunk.vmaf_target_cq = target_vmaf(chunk, args, cb)


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

    file = call_vmaf(chunk, gen_probes_names(chunk, q), args.n_threads, args.vmaf_path, args.vmaf_res, vmaf_filter=args.vmaf_filter,
                     vmaf_rate=args.vmaf_rate)
    score = read_weighted_vmaf(file)

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

def transform_vmaf(vmaf):
    if vmaf < 99.99:
        return -ln(1-vmaf/100)
    else:
        # return -ln(1-99.99/100)
        return 9.210340371976184

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

    dif1 = abs(transform_vmaf(target) - transform_vmaf(vmaf2))
    dif2 = abs(transform_vmaf(target) - transform_vmaf(vmaf1))

    tot = dif1 + dif2

    new_point = int(round(num1 * (dif1 / tot) + (num2 * (dif2 / tot))))
    return new_point


def target_vmaf(chunk: Chunk, args: Args, cb: Callbacks):
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

    if args.vmaf_steps < 3:
        #Use Euler's method with known relation between cq and vmaf
        vmaf_cq_deriv = -0.18
        ## Formula -ln(1-score/100) = vmaf_cq_deriv*last_q + constant
        #constant = -ln(1-score/100) - vmaf_cq_deriv*last_q
        ## Formula -ln(1-args.vmaf_target/100) = vmaf_cq_deriv*cq + constant
        #cq = (-ln(1-args.vmaf_target/100) - constant)/vmaf_cq_deriv
        next_q = int(round(last_q + (transform_vmaf(args.vmaf_target)-transform_vmaf(score))/vmaf_cq_deriv))

        #Clamp
        if next_q < args.min_q:
            next_q = args.min_q
        if args.max_q < next_q:
            next_q = args.max_q

        #Single probe cq guess or exit to avoid divide by zero
        if args.vmaf_steps == 1 or next_q == last_q:
            return next_q

        #Second probe at guessed value
        score_2 = vmaf_probe(chunk, next_q, args)

        #Calculate slope
        vmaf_cq_deriv = (transform_vmaf(score_2)-transform_vmaf(score))/(next_q-last_q)

        #Same deal different slope
        next_q = int(round(next_q+(transform_vmaf(args.vmaf_target)-transform_vmaf(score_2))/vmaf_cq_deriv))

        #Clamp
        if next_q < args.min_q:
            next_q = args.min_q
        if args.max_q < next_q:
            next_q = args.max_q

        return next_q

    # Initialize search boundary
    vmaf_lower = score
    vmaf_upper = score
    vmaf_cq_lower = last_q
    vmaf_cq_upper = last_q

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
        cb.run_callback("log", f"Chunk: {chunk.name}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} Vmaf: {vmaf_cq[-1][0]}\n\n")
        return next_q

    elif next_q == args.max_q and score > args.vmaf_target:
        cb.run_callback("log", f"Chunk: {chunk.name}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} Vmaf: {vmaf_cq[-1][0]}\n\n")
        return next_q

    # Set boundary
    if score < args.vmaf_target:
        vmaf_lower = score
        vmaf_cq_lower = next_q
    else:
        vmaf_upper = score
        vmaf_cq_upper = next_q

    # VMAF search
    for _ in range(args.vmaf_steps - 2):
        new_point = weighted_search(vmaf_cq_lower, vmaf_lower, vmaf_cq_upper, vmaf_upper, args.vmaf_target)
        if new_point in [x[1] for x in vmaf_cq]:
            break

        q_list.append(new_point)
        score = vmaf_probe(chunk, new_point, args)
        vmaf_cq.append((score, new_point))

        # Update boundary
        if score < args.vmaf_target:
            vmaf_lower = score
            vmaf_cq_lower = new_point
        else:
            vmaf_upper = score
            vmaf_cq_upper = new_point

    q, q_vmaf = get_target_q(vmaf_cq, args.vmaf_target)

    cb.run_callback("log", f'Chunk: {chunk.name}, Fr: {frames}\n'
        f'Q: {sorted([x[1] for x in vmaf_cq])}\n'
        f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n'
        f'Target Q: {q} Vmaf: {q_vmaf}\n\n')

    # Plot Probes
    if len(vmaf_cq) > 3:
        cb.run_callback("plotvmaf", args.vmaf_target, args.min_q, args.max_q, args.temp, vmaf_cq, chunk.name, frames)

    return q

