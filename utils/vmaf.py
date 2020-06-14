 #!/bin/env python
from pathlib import Path
import subprocess
from subprocess import PIPE, STDOUT
import sys
from matplotlib import pyplot as plt
import numpy as np
from math import  isnan


def read_vmaf_xml(file):
    with open(file, 'r') as f:
        file = f.readlines()
        file = [x.strip() for x in file if 'vmaf="' in x]
        vmafs = []
        for i in file:
            vmf = i[i.rfind('="') + 2: i.rfind('"')]
            vmafs.append(float(vmf))

        vmafs = [round(float(x), 5) for x in vmafs if isinstance(x, float)]
        calc = [x for x in vmafs if isinstance(x, float) and not isnan(x)]
        mean = round(sum(calc) / len(calc), 2)
        perc_1 = round(np.percentile(calc, 1), 2)
        perc_25 = round(np.percentile(calc, 25), 2)
        perc_75 = round(np.percentile(calc, 75), 2)

        return vmafs, mean, perc_1, perc_25, perc_75


def call_vmaf( source: Path, encoded: Path, model=None, return_file=False):

        if model:
            mod = f":model_path={model}"
        else:
            mod = ''

        # For vmaf calculation both source and encoded segment scaled to 1080
        # for proper vmaf calculation
        fl = source.with_name(encoded.stem).with_suffix('.xml').as_posix()
        cmd = f'ffmpeg -hide_banner -r 60 -i {source.as_posix()} -r 60 -i {encoded.as_posix()}  ' \
              f'-filter_complex "[0:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[scaled1];' \
              f'[1:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[scaled2];' \
              f'[scaled2][scaled1]libvmaf=log_path={fl}{mod}" -f null - '

        call = subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT).stdout

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

    xml = call_vmaf(inp, out, model=model, return_file=True)

    if not Path(xml).exists():
            print(f'Vmaf calculation failed for files:\n {inp.stem} {out.stem}')
            sys.exit()

    vmafs, mean, perc_1, perc_25, perc_75 = read_vmaf_xml(xml)

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
