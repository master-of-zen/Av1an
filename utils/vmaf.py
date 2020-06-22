 #!/bin/env python
from pathlib import Path
import subprocess
from subprocess import PIPE, STDOUT
import sys
from matplotlib import pyplot as plt
import numpy as np
from math import  isnan
from scipy import interpolate
import matplotlib
from  utils.utils import terminate
from .logger import log, set_log_file


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


def call_vmaf( source: Path, encoded: Path, model=None, return_file=False):

        if model:
            mod = f":model_path={model}"
        else:
            mod = ''

        # For vmaf calculation both source and encoded segment scaled to 1080
        # for proper vmaf calculation
        fl = source.with_name(encoded.stem).with_suffix('.xml').as_posix()
        cmd = f'ffmpeg -loglevel error -hide_banner -r 60 -i {source.as_posix()} -r 60 -i {encoded.as_posix()}  ' \
              f'-filter_complex "[0:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[scaled1];' \
              f'[1:v]scale=1920:1080:flags=spline:force_original_aspect_ratio=decrease[scaled2];' \
              f'[scaled2][scaled1]libvmaf=log_path={fl}{mod}" -f null - '

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

    xml = call_vmaf(inp, out, model=model, return_file=True)

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

        vmafs = [round(float(x), 3) for x in vmafs if type(x) == float]

    perc_1 = read_vmaf_xml(xml, 1)
    perc_25 = read_vmaf_xml(xml, 25)
    perc_75 = read_vmaf_xml(xml, 75)
    mean = mean = round(sum(vmafs) / len(vmafs), 3)

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

def x264_probes(video: Path, ffmpeg: str):
    cmd = f' ffmpeg -y -hide_banner -loglevel error -i {video.as_posix()} ' \
                  f'-r 2 -an {ffmpeg} -c:v libx264 -crf 0 {video.with_suffix(".mp4")}'
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


def plot_probes(x, y, f, tl, min_cq, max_cq, probe, xnew, vmaf_target_cq, frames, temp):
    # Saving plot of vmaf calculation
    matplotlib.use('agg')
    plt.ioff()
    plt.plot(x, y, 'x', color='tab:blue', alpha=1)
    plt.plot(xnew, f(xnew), color='tab:blue', alpha=1)
    plt.plot(vmaf_target_cq[0], vmaf_target_cq[1], 'o', color='red', alpha=1)
    plt.grid(True)
    plt.xlim(min_cq, max_cq)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.xlabel('CQ')
    plt.title(f'Chunk: {probe.stem}, Frames: {frames}')
    # plt.tight_layout()
    temp =  temp / probe.stem
    plt.tight_layout()
    plt.savefig(temp, dpi=300, format='png',transparent=True)
    plt.close()

