#! /bin/env python

import subprocess
import sys
from math import isnan
from pathlib import Path
from subprocess import PIPE, STDOUT
from matplotlib import pyplot as plt
import json
import numpy as np
from .bar import process_pipe
from .chunk import Chunk
from .utils import terminate


def read_vmaf_json(file, percentile):
    """Reads vmaf file with vmaf scores in it and return N percentile score from it.

    :return: N percentile score
    :rtype: float
    """
    with open(file, 'r') as f:
        file = json.load(f)
        vmafs = [x['metrics']['vmaf'] for x in file['frames']]
    perc = round(np.percentile(vmafs, percentile), 2)
    return perc


def call_vmaf(chunk: Chunk, encoded: Path, n_threads, model, res, fl_path: Path = None, vmaf_rate=0):

    cmd = ''
    # settings model path
    if model:
        mod = f":model_path={model}"
    else:
        mod = ''

    # limiting amount of threads for calculation
    if n_threads:
        n_threads = f':n_threads={n_threads}'
    else:
        n_threads = ''

    if fl_path is None:
        fl_path = chunk.fake_input_path.with_name(encoded.stem).with_suffix('.json')
    fl = fl_path.as_posix()

    # Change framerate of comparison to framerate of probe
    if vmaf_rate != 0:
        select_frames = f"select=not(mod(n\,{vmaf_rate})),"
    else:
        select_frames = ''

    # For vmaf calculation both source and encoded segment scaled to 1080
    # Also it's required to use -r before both files of vmaf calculation to avoid errors

    cmd = f"ffmpeg -loglevel info -y -thread_queue_size 1024 -hide_banner -r 60 -i {encoded.as_posix()} -r 60 -i - "

    filter_complex = ' -filter_complex '

    distorted = f'\"[0:v]{select_frames}scale={res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];'

    ref = fr"[1:v]{select_frames}scale={res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];"

    vmaf_filter = f"[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={fl}{mod}{n_threads}\" -f null - "

    cmd = cmd + filter_complex + distorted + ref + vmaf_filter

    ffmpeg_gen_pipe = subprocess.Popen(chunk.ffmpeg_gen_cmd.split(), stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(cmd, stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT, shell=True, universal_newlines=True)
    process_pipe(pipe)


    return fl_path


def plot_vmaf(source: Path, encoded: Path, args, model, vmaf_res):

    print('Calculating Vmaf...\r', end='')

    fl_path = encoded.with_name(f'{encoded.stem}_vmaflog').with_suffix(".json")

    # call_vmaf takes a chunk, so make a chunk of the entire source
    ffmpeg_gen_cmd = f'ffmpeg -y -hide_banner -loglevel error -i {source.as_posix()} {args.pix_format} -f yuv4mpegpipe -'
    input_chunk = Chunk(args.temp, 0, ffmpeg_gen_cmd, '', 0, 0)

    scores = call_vmaf(input_chunk, encoded, 0, model, vmaf_res, fl_path=fl_path)

    if not scores.exists():
        print(f'Vmaf calculation failed for chunks:\n {source.name} {encoded.stem}')
        sys.exit()

    file_path = encoded.with_name(f'{encoded.stem}_plot').with_suffix('.png')
    plot_vmaf_score_file(scores, file_path)


def plot_vmaf_score_file(scores: Path, plot_path: Path):

    perc_1 = read_vmaf_json(scores, 1)
    perc_25 = read_vmaf_json(scores, 25)
    perc_75 = read_vmaf_json(scores, 75)
    mean = read_vmaf_json(scores, 50)

    with open(scores) as f:
        file = json.load(f)
        vmafs = [x['metrics']['vmaf'] for x in file['frames']]
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
    plt.savefig(plot_path, dpi=500)
