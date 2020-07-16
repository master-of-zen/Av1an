#! /bin/env python

import subprocess
import sys
from math import isnan
from pathlib import Path
from subprocess import PIPE, STDOUT
from matplotlib import pyplot as plt

import numpy as np

from utils.utils import terminate


def read_vmaf_xml(file, percentile):
    with open(file, 'r') as f:
        file = f.readlines()
        file = [x.strip() for x in file if 'vmaf="' in x]
        vmafs = []
        for i in file:
            vmf = i[i.rfind('="') + 2: i.rfind('"')]
            vmafs.append(float(vmf))

        vmafs = [float(x) for x in vmafs if isinstance(x, float)]
        calc = [x for x in vmafs if isinstance(x, float) and not isnan(x)]
        perc = round(np.percentile(calc, percentile), 2)

        return perc


def call_vmaf(source: Path, encoded: Path, model, n_threads, return_file=False):

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
    fl = source.with_name(encoded.stem).with_suffix('.xml').as_posix()
    cmd = f'ffmpeg -loglevel error -hide_banner -r 60 -i {encoded.as_posix()} -r 60 -i  {source.as_posix()}  ' \
          f'-filter_complex "[0:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[distorted];' \
          f'[1:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[ref];' \
          f'[distorted][ref]libvmaf=log_path={fl}{mod}{n_threads}" -f null - '

    c = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT)
    call = c.stdout
    # print(c.stdout.decode())
    if 'error' in call.decode().lower():
        print('\n\nERROR IN VMAF CALCULATION\n\n',call.decode())
        terminate()

    if return_file:
        return fl

    call = call.decode().strip()
    vmf = call.split()[-1]
    try:
        vmf = float(vmf)
    except ValueError:
        vmf = 0
    return vmf


def plot_vmaf(inp: Path, out: Path, model=None):

    print('Calculating Vmaf...\r', end='')

    xml = call_vmaf(inp, out, n_threads=0, model=model, return_file=True)

    if not Path(xml).exists():
        print(f'Vmaf calculation failed for files:\n {inp.stem} {out.stem}')
        sys.exit()

    with open(xml, 'r') as fl:
        f = fl.readlines()
        f = [x.strip() for x in f if 'vmaf="' in x]
        vmafs = []
        for i in f:
            vmf = i[i.rfind('="') + 2: i.rfind('"')]
            vmafs.append(float(vmf))

        vmafs = [round(float(x), 3) for x in vmafs if isinstance(x, float)]

    perc_1 = read_vmaf_xml(xml, 1)
    perc_25 = read_vmaf_xml(xml, 25)
    perc_75 = read_vmaf_xml(xml, 75)
    mean = round(sum(vmafs) / len(vmafs), 3)

    # Plot
    plt.figure(figsize=(15, 4))

    for i in range(0, 100):
        plt.axhline(i, color='grey', linewidth=0.4)

        if i % 5 == 0:
            plt.axhline(i, color='black', linewidth=0.6)

    plt.plot(range(len(vmafs)), vmafs,
            label=f'Frames: {len(vmafs)}\nMean:{mean}\n'
            f'1%: {perc_1} \n25%: {perc_25} \n75%: {perc_75}', linewidth=0.7)
    plt.ylabel('VMAF')
    plt.legend(loc="lower right", markerscale=0, handlelength=0, fancybox=True, )
    plt.ylim(int(perc_1), 100)
    plt.tight_layout()
    plt.margins(0)

    # Save
    file_name = str(out.stem) + '_plot.png'
    plt.savefig(file_name, dpi=500)




