#! /bin/env python

import subprocess
import sys
from math import isnan
from pathlib import Path
from subprocess import PIPE, STDOUT
from matplotlib import pyplot as plt
import json
import numpy as np

from .utils import terminate


def read_vmaf_json(file, percentile):
    """Reads vmaf file with vmaf scores in it and return N percentile score from it.

    :return: N percentile score
    :rtype: float
    """
    with open(file, 'r') as f:
        file = json.load(f)
        vmafs = list({x['metrics']['vmaf'] for x in file['frames']})

    vmafs = [float(x) for x in vmafs if isinstance(x, float)]
    calc = [x for x in vmafs if isinstance(x, float) and not isnan(x)]
    perc = round(np.percentile(calc, percentile), 2)
    return perc


def call_vmaf(source: Path, encoded: Path, n_threads, model):

    if model:
        mod = f":model_path={model}"
    else:
        mod = ''

    if n_threads:
        n_threads = f':n_threads={n_threads}'
    else:
        n_threads = ''

    # For vmaf calculation both source and encoded segment scaled to 1080
    # for proper vmaf calculation
    # Also it's required to use -r before both files of vmaf calculation to avoid errors
    fl = source.with_name(encoded.stem).with_suffix('.json').as_posix()
    cmd = f'ffmpeg -loglevel error -hide_banner -r 60 -i {encoded.as_posix()} -r 60 -i  {source.as_posix()}  ' \
          f'-filter_complex "[0:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[distorted];' \
          f'[1:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[ref];' \
          f'[distorted][ref]libvmaf=log_fmt="json":log_path={fl}{mod}{n_threads}" -f null - '

    c = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT)
    call = c.stdout
    # print(c.stdout.decode())
    if 'error' in call.decode().lower():
        print('\n\nERROR IN VMAF CALCULATION\n\n',call.decode())
        terminate()

    return fl


def plot_vmaf(inp: Path, out: Path, model):

    print('Calculating Vmaf...\r', end='')

    scores = call_vmaf(inp, out, 0, model)

    if not Path(scores).exists():
        print(f'Vmaf calculation failed for files:\n {inp.stem} {out.stem}')
        sys.exit()

    perc_1 = read_vmaf_json(scores, 1)
    perc_25 = read_vmaf_json(scores, 25)
    perc_75 = read_vmaf_json(scores, 75)
    mean = read_vmaf_json(scores, 50)

    with open(scores) as f:
        file = json.load(f)
        vmafs = list({x['metrics']['vmaf'] for x in file['frames']})
        plot_size = len(vmafs)

    plt.figure(figsize=(15, 4))

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
    file_name = str(out.stem) + '_plot.png'
    plt.savefig(file_name, dpi=500)




