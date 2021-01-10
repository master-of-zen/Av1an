#!/bin/env python

import json
import shlex
import subprocess
import sys
from collections import deque

from pathlib import Path
from subprocess import PIPE, STDOUT

import numpy as np
from math import log10, ceil, floor
from math import log as ln

from av1an.logger import log

try:
    import matplotlib
    from matplotlib import pyplot as plt
    matplotlib.use('Agg')
except ImportError:
    matplotlib = None
    plt = None

from av1an.manager.Pipes import process_pipe
from av1an.chunk import Chunk


class VMAF:

    def __init__(self, n_threads=0, model=None, res=None, vmaf_filter=None):
        self.n_threads = f':n_threads={n_threads}' if n_threads else ''
        self.model = f":model_path={model}" if model else ''
        self.res = res if res else "1920x1080"
        self.vmaf_filter = vmaf_filter + ',' if vmaf_filter else ''
        self.validate_vmaf()

    def validate_vmaf(self):
        """
        Test run of ffmpeg for validating that ffmpeg/libmaf/models properly setup
        """

        if self.model or self.n_threads:
            add = f'={self.model}{self.n_threads}'
        else:
            add = ''

        cmd = f' ffmpeg -hide_banner -filter_complex testsrc=duration=1:size=1920x1080:rate=1[B];testsrc=duration=1:size=1920x1080:rate=1[A];[B][A]libvmaf{add} -t 1  -f null - '.split()

        pipe = subprocess.Popen(cmd, stdout=PIPE, stderr=STDOUT, universal_newlines=True)

        encoder_history = deque(maxlen=30)

        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break
            if len(line) == 0:
                continue
            if line:
                encoder_history.append(line)

        if pipe.returncode != 0 and pipe.returncode != -2:
            print(f"\n:: VMAF validation error: {pipe.returncode}")
            print('\n'.join(encoder_history))
            sys.exit()

    @staticmethod
    def read_json(file):
        """
        Reads file and return dictionary of it's contents

        :return: Vmaf file dictionary
        :rtype: dict
        """
        with open(file, 'r') as f:
            fl = json.load(f)
            return fl

    def get_vmaf_motion(self):
        """
        Runs vmaf_motion filter on chunk and returns average score
        """

        cmd = ['ffmpeg', '-loglevel', 'error', '-y', '-hide_banner', '-r', '60', '-i',
              '-', '-vf', 'vmafmotion', '-f', 'null', '-']

        print(cmd)

    def call_vmaf(self, chunk: Chunk, encoded: Path, vmaf_rate: int = None, fl_path: Path = None):
        """
        Runs vmaf for Av1an
        """
        cmd = ''

        if fl_path is None:
            fl_path = chunk.fake_input_path.with_name(encoded.stem).with_suffix('.json')
        fl = fl_path.as_posix()

        cmd_in = ('ffmpeg', '-loglevel', 'error', '-y', '-thread_queue_size', '1024', '-hide_banner',
                  '-r', '60', '-i', encoded.as_posix(), '-r', '60', '-i', '-')

        filter_complex = ('-filter_complex',)

        # Change framerate of comparison to framerate of probe
        select_frames = f"select=not(mod(n\\,{vmaf_rate}))," if vmaf_rate else ''

        distorted = f'[0:v]{select_frames}scale={self.res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];'

        ref = fr'[1:v]{select_frames}{self.vmaf_filter}scale={self.res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];'

        vmaf_filter = f"[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={shlex.quote(fl)}{self.model}{self.n_threads}"

        cmd_out = ('-f', 'null', '-')

        cmd = (*cmd_in, *filter_complex, distorted + ref + vmaf_filter, *cmd_out)

        ffmpeg_gen_pipe = subprocess.Popen(chunk.ffmpeg_gen_cmd,
        stdout=PIPE,
        stderr=STDOUT)

        pipe = subprocess.Popen(cmd,
                                stdin=ffmpeg_gen_pipe.stdout,
                                stdout=PIPE,
                                stderr=STDOUT,
                                universal_newlines=True)
        process_pipe(pipe, chunk)

        return fl_path

    @staticmethod
    def get_percentile(scores, percent):
        """
        Find the percentile of a list of values.
        :param scores: - is a list of values. Note N MUST BE already sorted.
        :param percent: - a float value from 0.0 to 1.0.
        :return: - the percentile of the values
        """
        scores = sorted(scores)
        key = lambda x: x

        k = (len(scores)-1) * percent
        f = floor(k)
        c = ceil(k)
        if f == c:
            return key(scores[int(k)])

        d0 = (scores[int(f)]) * (c-k)
        d1 = (scores[int(c)]) * (k-f)
        return d0+d1

    @staticmethod
    def transform_vmaf(vmaf):
        if vmaf<99.99:
            return -ln(1-vmaf/100)
        else:
            # return -ln(1-99.99/100)
            return 9.210340371976184

    @staticmethod
    def read_vmaf_with_motion_compensation(file, percentile=0):
        """Reads vmaf file with vmaf scores in it and return N percentile score from it.

        :return: N percentile score
        :rtype: float
        """

        jsn = VMAF.read_json(file)

        vmafs = sorted([x['metrics']['vmaf'] for x in jsn['frames']])
        motion = np.average([x['metrics']['motion2'] for x in jsn['frames']])
        print(round(motion, 1))
        percentile = percentile if percentile != 0 else 0.25
        score = VMAF.get_percentile(vmafs, percentile)

        return round(score, 2)

    @staticmethod
    def read_weighted_vmaf(file, percentile=0):
        """Reads vmaf file with vmaf scores in it and return N percentile score from it.

        :return: N percentile score
        :rtype: float
        """

        jsn = VMAF.read_json(file)

        vmafs = sorted([x['metrics']['vmaf'] for x in jsn['frames']])

        percentile = percentile if percentile != 0 else 0.25
        score = VMAF.get_percentile(vmafs, percentile)

        return round(score, 2)

    def get_vmaf_file(self, source: Path, encoded: Path):
        """
        Running vmaf on 2 files and returning file
        """

        if not all((isinstance(source, Path), isinstance(encoded,Path))):
            source = Path(source)
            encoded = Path(encoded)

        fl_path = encoded.with_name(f'{encoded.stem}_vmaflog').with_suffix(".json")

        # call_vmaf takes a chunk, so make a chunk of the entire source
        ffmpeg_gen_cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i',
                          source.as_posix(), '-f', 'yuv4mpegpipe', '-']

        input_chunk = Chunk('', 0, ffmpeg_gen_cmd, '', 0, 0)

        scores = self.call_vmaf(input_chunk, encoded, 0, fl_path=fl_path)
        return scores

    def get_vmaf_json(self, source: Path, encoded: Path):
        """
        Returning dictionary from vmaf json
        """
        fl = self.get_vmaf_file(source, encoded)
        js = self.read_json(fl)
        return js

    def get_vmaf_score(self, source: Path, encoded: Path, percentile=50):
        """
        Returning calculated vmaf score
        Posible to set percentile, default 50

        :rtype: float
        """
        js = self.get_vmaf_json(source, encoded)
        score = np.average([x['metrics']['vmaf'] for x in js['frames']])
        return score

    def plot_vmaf(self, source: Path, encoded: Path, args):
        """
        Making VMAF plot after encode is done
        """

        print(':: VMAF Run..\r', end='')

        fl_path = encoded.with_name(f'{encoded.stem}_vmaflog').with_suffix(".json")

        # call_vmaf takes a chunk, so make a chunk of the entire source
        ffmpeg_gen_cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i',
                          source.as_posix(), *args.pix_format, '-f', 'yuv4mpegpipe', '-']

        input_chunk = Chunk(args.temp, 0, ffmpeg_gen_cmd, '', 0, 0)

        scores = self.call_vmaf(input_chunk, encoded, 0, fl_path=fl_path)

        if not scores.exists():
            print(f'Vmaf calculation failed for chunks:\n {source.name} {encoded.stem}')
            sys.exit()

        file_path = encoded.with_name(f'{encoded.stem}_plot').with_suffix('.png')
        self.plot_vmaf_score_file(scores, file_path)

    def plot_vmaf_score_file(self, scores: Path, plot_path: Path):
        """
        Read vmaf json and plot VMAF values for each frame
        """
        if plt is None:
            log(f'Matplotlib is not installed or could not be loaded, aborting plot_vmaf')
            return

        perc_1 = self.read_weighted_vmaf(scores, 0.01)
        perc_25 = self.read_weighted_vmaf(scores, 0.25)
        perc_75 = self.read_weighted_vmaf(scores, 0.75)
        mean = self.read_weighted_vmaf(scores, 0.50)

        with open(scores) as f:
            file = json.load(f)
            vmafs = [x['metrics']['vmaf'] for x in file['frames']]
            plot_size = len(vmafs)

        figure_width = 3 + round((4 * log10(plot_size)))
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
