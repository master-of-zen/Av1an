#! /bin/env python

import json
import shlex
import subprocess
import sys
from pathlib import Path
from subprocess import PIPE, STDOUT

import numpy as np
from math import log10

from libAv1an.LibAv1an.run_cmd import process_pipe
from libAv1an.Chunks.chunk import Chunk
from libAv1an.LibAv1an.callbacks import Callbacks

def read_json(file):
    """
    Reads file and return dictionary of it's contents

    :return: Vmaf file dictionary
    :rtype: dict
    """
    with open(file, 'r') as f:
        fl = json.load(f)
        return fl

def read_weighted_vmaf(file, percentile=0):
    """Reads vmaf file with vmaf scores in it and return N percentile score from it.

    :return: N percentile score
    :rtype: float
    """

    jsn = read_json(file)

    vmafs = [x['metrics']['vmaf'] for x in jsn['frames']]

    if percentile == 0:
        # Using 2 standart deviations to weight for bad frames
        mean = np.mean(vmafs)
        dev = np.std(vmafs)
        minimum = np.min(vmafs)

        perc = mean - (2 * dev)

        perc = max(perc, minimum)

    else:
        perc = round(np.percentile(vmafs, percentile), 2)

    return perc


def call_vmaf(chunk: Chunk, encoded: Path, n_threads, model, res,
              fl_path: Path = None, vmaf_filter=None, vmaf_rate=0):
    cmd = ''

    # settings model path
    mod = f":model_path={model}" if model else ''

    # limiting amount of threads for calculation
    n_threads = f':n_threads={n_threads}' if n_threads else ''

    if fl_path is None:
        fl_path = chunk.fake_input_path.with_name(encoded.stem).with_suffix('.json')
    fl = fl_path.as_posix()

    filter = vmaf_filter + ',' if vmaf_filter else ''

    # For vmaf calculation both source and encoded segment scaled to 1080
    # Also it's required to use -r before both files of vmaf calculation to avoid errors

    cmd_in = ('ffmpeg', '-loglevel', 'info', '-y', '-thread_queue_size', '1024', '-hide_banner',
              '-r', '60', '-i', encoded.as_posix(), '-r', '60', '-i', '-')

    filter_complex = ('-filter_complex',)

    # Change framerate of comparison to framerate of probe
    select_frames = f"select=not(mod(n\\,{vmaf_rate}))," if vmaf_rate != 0 else ''

    distorted = f'[0:v]{select_frames}scale={res}:flags=bicubic:\
                  force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];'

    ref = fr'[1:v]{select_frames}{filter}scale={res}:flags=bicubic:'\
          'force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];'

    vmaf_filter = f"[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:\
                    log_path={shlex.quote(fl)}{mod}{n_threads}"

    cmd_out = ('-f', 'null', '-')

    cmd = (*cmd_in, *filter_complex, distorted + ref + vmaf_filter, *cmd_out)

    ffmpeg_gen_pipe = subprocess.Popen(chunk.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(cmd, stdin=ffmpeg_gen_pipe.stdout,
                            stdout=PIPE, stderr=STDOUT, universal_newlines=True)
    process_pipe(pipe)

    return fl_path


def plot_vmaf(source: Path, encoded: Path, args, model, vmaf_res, cb: Callbacks):
    """
    Making VMAF plot after encode is done
    """

    print('Calculating Vmaf...\r', end='')

    fl_path = encoded.with_name(f'{encoded.stem}_vmaflog').with_suffix(".json")

    # call_vmaf takes a chunk, so make a chunk of the entire source
    ffmpeg_gen_cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i',
                      source.as_posix(), *args.pix_format, '-f', 'yuv4mpegpipe', '-']
    input_chunk = Chunk(args.temp, 0, ffmpeg_gen_cmd, '', 0, 0)

    scores = call_vmaf(input_chunk, encoded, 0, model, vmaf_res, fl_path=fl_path)

    if not scores.exists():
        print(f'Vmaf calculation failed for chunks:\n {source.name} {encoded.stem}')
        sys.exit()

    file_path = encoded.with_name(f'{encoded.stem}_plot').with_suffix('.png')
    cb.run_callback("plotvmaffile", scores, file_path)
