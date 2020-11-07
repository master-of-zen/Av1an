import matplotlib
from libAv1an.Chunks.chunk import Chunk
from matplotlib import pyplot as plt
import numpy as np
from math import isnan

from libAv1an.VMAF.target_vmaf import interpolate_data
from libAv1an.LibAv1an.callbacks import Callbacks
from Av1an.bar import new_tqdm_bar, svt_vp9_bar, update_tqdm_bar, start_counter, end_counter
from Av1an.logger import *
from libAv1an.VMAF.vmaf import read_vmaf_json
from libAv1an.LibAv1an.args import Args
import json

import math


def add_callbacks(args: Args):
    c = Callbacks()
    c.subscribe("log", log)
    c.subscribe("newtask", new_tqdm_bar)
    c.subscribe("svtvp9update", svt_vp9_bar)
    c.subscribe("newframes", update_tqdm_bar)
    c.subscribe("terminate", terminate)
    if args.vmaf_plots:
        c.subscribe("plotvmaf", plot_probes)

    c.subscribe("plotvmaffile", plot_vmaf_score_file)
    c.subscribe("logready", set_log)
    c.subscribe("startencode", start_counter)
    c.subscribe("endencode", end_counter)
    return c


def plot_vmaf_score_file(scores: Path, plot_path: Path):
    """
    Read vmaf json and plot VMAF values for each frame
    """

    perc_1 = read_vmaf_json(scores, 1)
    perc_25 = read_vmaf_json(scores, 25)
    perc_75 = read_vmaf_json(scores, 75)
    mean = read_vmaf_json(scores, 50)

    with open(scores) as f:
        file = json.load(f)
        vmafs = [x['metrics']['vmaf'] for x in file['frames']]
        plot_size = len(vmafs)

    figure_width = 3 + round((4 * math.log10(plot_size)))
    plt.figure(figsize=(figure_width, 5))

    plt.plot([1, plot_size], [perc_1, perc_1], '-', color='red')
    plt.annotate(f'1%: {perc_1}', xy=(0, perc_1), color='red')

    plt.plot([1, plot_size], [perc_25, perc_25], ':', color='orange')
    plt.annotate(f'25%: {perc_25}', xy=(0, perc_25), color='orange')

    plt.plot([1, plot_size], [perc_75, perc_75], ':', color='green')
    plt.annotate(f'75%: {perc_75}', xy=(0, perc_75), color='green')

    plt.plot([1, plot_size], [mean, mean], ':', color='black')
    plt.annotate(f'Mean: {mean}', xy=(0, mean), color='black')

    for i in range(0, 100):
        plt.axhline(i, color='grey', linewidth=0.4)
        if i % 5 == 0:
            plt.axhline(i, color='black', linewidth=0.6)

    plt.plot(range(plot_size), vmafs,
             label=f'Frames: {plot_size}\nMean:{mean}\n'
                   f'1%: {perc_1} \n25%: {perc_25} \n75%: {perc_75}', linewidth=0.7)
    plt.ylabel('VMAF')
    plt.legend(loc="lower right", markerscale=0, handlelength=0, fancybox=True, )
    plt.ylim(int(perc_1), 100)
    plt.tight_layout()
    plt.margins(0)

    # Save
    plt.savefig(plot_path, dpi=250)


def plot_probes(vmaf_target, min_q, max_q, vmaf_cq, tmp, chunk: Chunk, frames):
    # Saving plot of vmaf calculation

    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    cq, tl, f, xnew = interpolate_data(vmaf_cq, vmaf_target)
    matplotlib.use('agg')
    plt.ioff()
    plt.plot(xnew, f(xnew), color='tab:blue', alpha=1)
    plt.plot(x, y, 'p', color='tab:green', alpha=1)
    plt.plot(cq[0], cq[1], 'o', color='red', alpha=1)
    plt.grid(True)
    plt.xlim(min_q, max_q)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.title(f'Chunk: {chunk.name}, Frames: {frames}')
    plt.xticks(np.arange(min_q, max_q + 1, 1.0))
    temp = tmp / chunk.name
    plt.savefig(f'{temp}.png', dpi=200, format='png')
    plt.close()


def terminate(exitcode):
    if exitcode != 0:
        sys.exit(exitcode)
