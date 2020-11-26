#!/bin/env python

from math import isnan
from math import log as ln

import subprocess
from subprocess import STDOUT, PIPE

import matplotlib
from matplotlib import pyplot as plt

import numpy as np
from scipy import interpolate

from .target_quality import vmaf_probe, weighted_search, get_target_q, \
    adapt_probing_rate
from VMAF import read_weighted_vmaf, transform_vmaf
from Projects import Project
from Av1an.bar import process_pipe
from Chunks.chunk import Chunk
from Av1an.commandtypes import CommandPair, Command
from Av1an.logger import log


def per_shot_target_quality_routine(args: Project, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param args: the Project
    :param chunk: the Chunk
    :return: None
    """
    chunk.per_shot_target_quality_cq = per_shot_target_quality(chunk, args)


def interpolate_data(vmaf_cq: list, target_quality):
    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    # Interpolate data
    f = interpolate.interp1d(x, y, kind='quadratic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))

    # Getting value closest to target
    tl = list(zip(xnew, f(xnew)))
    target_quality_cq = min(tl, key=lambda l: abs(l[1] - target_quality))
    return target_quality_cq, tl, f, xnew


def plot_probes(args, vmaf_cq, chunk: Chunk, frames):
    # Saving plot of vmaf calculation

    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    cq, tl, f, xnew = interpolate_data(vmaf_cq, args.target_quality)
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


def per_shot_target_quality(chunk: Chunk, args: Project):
    vmaf_cq = []
    frames = chunk.frames

    # Adapt probing rate
    if args.probing_rate in (1,2):
        probing_rate = args.probing_rate
    else:
        probing_rate = adapt_probing_rate(args.probing_rate, frames)

    q_list = []
    score = 0

    # Make middle probe
    middle_point = (args.min_q + args.max_q) // 2
    q_list.append(middle_point)
    last_q = middle_point

    score = read_weighted_vmaf(vmaf_probe(chunk, last_q, args, probing_rate))
    vmaf_cq.append((score, last_q))

    if args.probes < 3:
        #Use Euler's method with known relation between cq and vmaf
        vmaf_cq_deriv = -0.18
        ## Formula -ln(1-score/100) = vmaf_cq_deriv*last_q + constant
        #constant = -ln(1-score/100) - vmaf_cq_deriv*last_q
        ## Formula -ln(1-args.vmaf_target/100) = vmaf_cq_deriv*cq + constant
        #cq = (-ln(1-args.vmaf_target/100) - constant)/vmaf_cq_deriv
        next_q = int(round(last_q + (transform_vmaf(args.target_quality) - transform_vmaf(score))/vmaf_cq_deriv))

        #Clamp
        if next_q < args.min_q:
            next_q = args.min_q
        if args.max_q < next_q:
            next_q = args.max_q

        #Single probe cq guess or exit to avoid divide by zero
        if args.probes == 1 or next_q == last_q:
            return next_q

        #Second probe at guessed value
        score_2 = read_weighted_vmaf(vmaf_probe(chunk, next_q, args, probing_rate))

        #Calculate slope
        vmaf_cq_deriv = (transform_vmaf(score_2) - transform_vmaf(score)) / (next_q-last_q)

        #Same deal different slope
        next_q = int(round(next_q+(transform_vmaf(args.target_quality)-transform_vmaf(score_2))/vmaf_cq_deriv))

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
    if score < args.target_quality:
        next_q = args.min_q
        q_list.append(args.min_q)
    else:
        next_q = args.max_q
        q_list.append(args.max_q)

    # Edge case check
    score = read_weighted_vmaf(vmaf_probe(chunk, next_q, args, probing_rate))
    vmaf_cq.append((score, next_q))

    if next_q == args.min_q and score < args.target_quality:
        log(f"Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} VMAF: {round(vmaf_cq[-1][0], 2)}\n\n")
        return next_q

    elif next_q == args.max_q and score > args.target_quality:
        log(f"Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} VMAF: {round(vmaf_cq[-1][0], 2)}\n\n")
        return next_q

    # Set boundary
    if score < args.target_quality:
        vmaf_lower = score
        vmaf_cq_lower = next_q
    else:
        vmaf_upper = score
        vmaf_cq_upper = next_q

    # VMAF search
    for _ in range(args.probes - 2):
        new_point = weighted_search(vmaf_cq_lower, vmaf_lower, vmaf_cq_upper, vmaf_upper, args.target_quality)
        if new_point in [x[1] for x in vmaf_cq]:
            break

        q_list.append(new_point)
        score = read_weighted_vmaf(vmaf_probe(chunk, new_point, args, probing_rate))
        vmaf_cq.append((score, new_point))

        # Update boundary
        if score < args.target_quality:
            vmaf_lower = score
            vmaf_cq_lower = new_point
        else:
            vmaf_upper = score
            vmaf_cq_upper = new_point

    q, q_vmaf = get_target_q(vmaf_cq, args.target_quality)

    log(f'Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n'
        f'Q: {sorted([x[1] for x in vmaf_cq])}\n'
        f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n'
        f'Target Q: {q} VMAF: {round(q_vmaf, 2)}\n\n')

    # Plot Probes
    if args.vmaf_plots and len(vmaf_cq) > 3:
        plot_probes(args, vmaf_cq, chunk, frames)

    return q
