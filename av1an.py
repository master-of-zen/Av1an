#!/usr/bin/env python3

import time
import json
import re
from tqdm import tqdm
import sys
import os
import shutil
import atexit
from distutils.spawn import find_executable
from ast import literal_eval
from psutil import virtual_memory
import argparse
import subprocess
from subprocess import PIPE, STDOUT
from pathlib import Path
import cv2
import numpy as np
from scipy import interpolate
from math import isnan
import matplotlib.pyplot as plt
from multiprocessing.managers import BaseManager
import concurrent
import concurrent.futures
from utils.aom_keyframes import find_aom_keyframes
from utils.pyscenedetect import pyscene
from utils.utils import read_vmaf_xml, get_brightness, frame_probe, get_keyframes, get_cq, man_cq, reduce_scenes

# Todo: Separation, Clip encoder objects, Threading instead of multiprocessing.

if sys.version_info < (3, 6):
    print('Python 3.6+ required')
    sys.exit()

if sys.platform == 'linux':
    def restore_term():
        os.system("stty sane")
    atexit.register(restore_term)


# Stuff for updating encoded progress in real-time
class MyManager(BaseManager):
    pass


def Manager():
    m = MyManager()
    m.start()
    return m


class Counter():
    def __init__(self, total, initial):
        self.tqdm_bar = tqdm(total=total, initial=initial, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)

    def update(self, value):
        self.tqdm_bar.update(value)


MyManager.register('Counter', Counter)


class Av1an:

    def __init__(self):
        """Av1an - Python framework for AV1, VP9, VP8 encodes."""
        self.d = dict()
        self.encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc'}

    def terminate(self):
        os.kill(os.getpid(), 9)

    def log(self, info):
        """Default logging function, write to file."""
        with open(self.d.get('logging'), 'a') as log:
            log.write(time.strftime('%X') + ' ' + info)

    def call_cmd(self, cmd, capture_output=False):
        """Calling system shell, if capture_output=True output string will be returned."""
        if capture_output:
            return subprocess.run(cmd, shell=True, stdout=PIPE, stderr=STDOUT).stdout

        with open(self.d.get('logging'), 'a') as log:
            subprocess.run(cmd, shell=True, stdout=log, stderr=log)

    def check_executables(self):
        if not find_executable('ffmpeg'):
            print('No ffmpeg')
            self.terminate()

        # Check if encoder executable is reachable
        if self.d.get('encoder') in self.encoders:
            enc = self.encoders.get(self.d.get('encoder'))

            if not find_executable(enc):
                print(f'Encoder {enc} not found')
                self.terminate()
        else:
            print(f'Not valid encoder {self.d.get("encoder")} ')
            self.terminate()

    def process_inputs(self):
        # Check input file for being valid
        if not self.d.get('input'):
            print('No input file')
            self.terminate()

        inputs = self.d.get('input')

        if inputs[0].is_dir():
            inputs = [x for x in inputs[0].iterdir() if x.suffix in (".mkv", ".mp4", ".mov", ".avi", ".flv", ".m2ts")]

        valid = np.array([i.exists() for i in inputs])

        if not all(valid):
            print(f'File(s) do not exist: {", ".join([str(inputs[i]) for i in np.where(not valid)[0]])}')
            self.terminate()

        if len(inputs) > 1:
            self.d['queue'] = inputs
        else:
            self.d['input'] = inputs[0]

    def config(self):
        """Creation and reading of config files with saved settings"""
        cfg = self.d.get('config')
        if cfg:
            if cfg.exists():
                with open(cfg) as f:
                    c: dict = dict(json.load(f))
                    self.d.update(c)

            else:
                with open(cfg, 'w') as f:
                    c = dict()
                    c['video_params'] = self.d.get('video_params')
                    c['encoder'] = self.d.get('encoder')
                    c['ffmpeg'] = self.d.get('ffmpeg')
                    c['audio_params'] = self.d.get('audio_params')
                    json.dump(c, f)

        # Changing pixel format, bit format
        self.d['pix_format'] = f'-strict -1 -pix_fmt {self.d.get("pix_format")}'
        self.d['ffmpeg_pipe'] = f' {self.d.get("ffmpeg")} {self.d.get("pix_format")} -f yuv4mpegpipe - |'

        # Make sure that vmaf calculated after encoding
        if self.d.get('vmaf_target'):
            self.d['vmaf'] = True

        if self.d.get("vmaf_path"):
            if not Path(self.d.get("vmaf_path")).exists():
                print(f'No such model: {Path(self.d.get("vmaf_path")).as_posix()}')
                self.terminate()

    def arg_parsing(self):
        """Command line parsing"""
        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=0, help='0 - local, 1 - master, 2 - encoder')

        # Input/Output/Temp
        parser.add_argument('--input', '-i', nargs='+', type=Path, help='Input File')
        parser.add_argument('--temp', type=Path, default=Path('.temp'), help='Set temp folder path')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')

        # Splitting
        parser.add_argument('--split_method', type=str, default='pyscene', help='Specify splitting method')
        parser.add_argument('--extra_split', '-xs', type=int, default=0, help='Number of frames after which make split')


        # PySceneDetect split
        parser.add_argument('--scenes', '-s', type=str, default=None, help='File location for scenes')
        parser.add_argument('--threshold', '-tr', type=float, default=50, help='PySceneDetect Threshold')

        # Encoding
        parser.add_argument('--passes', '-p', type=int, default=2, help='Specify encoding passes')
        parser.add_argument('--video_params', '-v', type=str, default='', help='encoding settings')
        parser.add_argument('--encoder', '-enc', type=str, default='aom', help='Choosing encoder')
        parser.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
        parser.add_argument('-cfg', '--config', type=Path, help='Parameters file. Save/Read: '
                                                                'Video, Audio, Encoder, FFmpeg parameteres')

        # FFmpeg params
        parser.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
        parser.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
        parser.add_argument('--pix_format', '-fmt', type=str, default='yuv420p', help='FFmpeg pixel format')

        # Misc
        parser.add_argument('--logging', '-log', type=str, default=None, help='Enable logging')
        parser.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
        parser.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')
        parser.add_argument('--keep', help='Keep temporally folder after encode', action='store_true')

        # Boost
        parser.add_argument('--boost', help='Experimental feature, decrease CQ of clip based on brightness.'
                                            'Darker = lower CQ', action='store_true')
        parser.add_argument('--boost_range', default=15, type=int, help='Range/strength of CQ change')
        parser.add_argument('--boost_limit', default=10, type=int, help='CQ limit for boosting')

        # Grain
        # Todo: grain stuf
        # parser.add_argument('--grain', help='Exprimental feature, adds generated grain based on video brightness',
        #                    action='store_true')
        # parser.add_argument('--grain_range')

        # Vmaf
        parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        parser.add_argument('--vmaf_path', type=Path, default=None, help='Path to vmaf models')

        # Target Vmaf
        parser.add_argument('--vmaf_target', type=float, help='Value of Vmaf to target')
        parser.add_argument('--vmaf_steps', type=int, default=4, help='Steps between min and max qp for target vmaf')
        parser.add_argument('--min_cq', type=int, default=25, help='Min cq for target vmaf')
        parser.add_argument('--max_cq', type=int, default=50, help='Max cq for target vmaf')
        parser.add_argument('--vmaf_plots', help='Make plots of probes in temp folder', action='store_true')

        # Server parts
        parser.add_argument('--host', nargs='+', type=str, help='ips of encoders')

        # Store all vars in dictionary
        self.d = vars(parser.parse_args())

    def outputs_filenames(self):
        if self.d.get('output_file'):
            self.d['output_file'] = self.d.get('output_file').with_suffix('.mkv')
        else:
            self.d['output_file'] = Path(f'{self.d.get("input").stem}_av1.mkv')

    def determine_resources(self):
        """Returns number of workers that machine can handle with selected encoder."""

        # If set by user, skip
        if self.d.get('workers') != 0:
            return

        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.d.get('encoder') == 'aom' or self.d.get('encoder') == 'rav1e' or self.d.get('encoder') == 'vpx':
            self.d['workers'] = round(min(cpu / 2, ram / 1.5))

        elif self.d.get('encoder') == 'svt_av1':
            self.d['workers'] = round(min(cpu, ram)) // 5

        # fix if workers round up to 0
        if self.d.get('workers') == 0:
            self.d['workers'] = 1

    def set_logging(self):
        """Setting logging file location"""
        if self.d.get('logging'):
            self.d['logging'] = f"{self.d.get('logging')}.log"
        else:
            self.d['logging'] = self.d.get('temp') / 'log.log'

        self.log(f"Av1an Started\nCommand:\n{' '.join(sys.argv)}\n")

    def setup(self):
        """Creating temporally folders when needed."""
        # Make temporal directories, and remove them if already presented
        if not self.d.get('resume'):
            if self.d.get('temp').is_dir():
                shutil.rmtree(self.d.get('temp'))

        (self.d.get('temp') / 'split').mkdir(parents=True, exist_ok=True)
        (self.d.get('temp') / 'encode').mkdir(exist_ok=True)

        if self.d.get('logging') is os.devnull:
            self.d['logging'] = self.d.get('temp') / 'log.log'

    def extract_audio(self, input_vid: Path):
        """Extracting audio from source, transcoding if needed."""
        input_vid = self.d.get('input')
        audio_params = self.d.get("audio_params")
        audio_file = self.d.get('temp') / 'audio.mkv'
        if audio_file.exists():
            self.log('Reusing Audio File\n')
            return

        # Checking is source have audio track
        check = fr' ffmpeg -y -hide_banner -loglevel error -ss 0 -i "{input_vid}" -t 0 -vn -c:a copy -f null -'
        is_audio_here = len(self.call_cmd(check, capture_output=True)) == 0

        # If source have audio track - process it
        if is_audio_here:
            self.log(f'Audio processing\n'
                     f'Params: {self.d.get("audio_params")}\n')
            cmd = f'ffmpeg -y -hide_banner -loglevel error -i "{input_vid}" -vn ' \
                  f'{audio_params} {audio_file}'
            self.call_cmd(cmd)

    def call_vmaf(self, source: Path, encoded: Path, file=False):

        model: Path = self.d.get("vmaf_path")
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

        call = self.call_cmd(cmd, capture_output=True)
        if file:
            return fl

        call = call.decode().strip()
        vmf = call.split()[-1]
        try:
            vmf = float(vmf)
        except ValueError:
            vmf = 0
        return vmf

    def split(self, frames):
        """Split video by frame numbers, or just copying video."""

        video = self.d.get('input')

        cmd = [
            "ffmpeg", "-hide_banner", "-y",
            "-i", video.absolute().as_posix(),
            "-map", "0:v:0",
            "-an",
            "-c", "copy",
            "-avoid_negative_ts", "1"
        ]

        if len(frames) > 0:
            cmd.extend([
                "-f", "segment",
                "-segment_frames", frames
            ])
        cmd.append(os.path.join(self.d.get("temp"), "split", "%05d.mkv"))
        pipe = subprocess.Popen(cmd, stdout=PIPE, stderr=STDOUT)
        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break

    def frame_check(self, source: Path, encoded: Path):
        """Checking is source and encoded video frame count match."""
        try:
            status_file = Path(self.d.get("temp") / 'done.json')
            with status_file.open() as f:
                d = json.load(f)

            if self.d.get("no_check"):
                s1 = frame_probe(source)
                d['done'][source.name] = s1
                with status_file.open('w') as f:
                    json.dump(d, f)
                    return

            s1, s2 = [frame_probe(i) for i in (source, encoded)]

            if s1 == s2:
                d['done'][source.name] = s1
                with status_file.open('w') as f:
                    json.dump(d, f)
            else:
                print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')
        except IndexError:
            print('Encoding failed, check validity of your encoding settings/commands and start again')
            self.terminate()
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'\nError frame_check: {e}\nAt line: {exc_tb.tb_lineno}\n')

    def get_video_queue(self, source_path: Path):
        """Returns sorted list of all videos that need to be encoded. Big first."""
        queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']

        done_file = self.d.get('temp') / 'done.json'
        if self.d.get('resume') and done_file.exists():
            try:
                with open(done_file) as f:
                    data = json.load(f)
                data = data['done'].keys()
                queue = [x for x in queue if x.name not in data]
            except Exception as e:
                _, _, exc_tb = sys.exc_info()
                print(f'Error at resuming {e}\nAt line {exc_tb.tb_lineno}')

        queue = sorted(queue, key=lambda x: -x.stat().st_size)

        if len(queue) == 0:
            # TODO: this could also be because we're resuming but everything
            # is done.
            print('Error: No files found in .temp/split, probably splitting not working')
            self.terminate()

        return queue

    def svt_av1_encode(self, inputs):
        """SVT-AV1 encoding command composition."""
        encoder = 'SvtAv1EncApp'
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")
        passes = self.d.get('passes')
        commands = []
        if not params:
            print('-w -h -fps is required parameters for svt_av1 encoder')
            self.terminate()

        if passes == 1:
            commands = [
                (f'-i {file[0]} {pipe} ' +
                 f'  {encoder} -i stdin {params} -b {file[1].with_suffix(".ivf")} -',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        if passes == 2:
            p2i = '-input-stat-file '
            p2o = '-output-stat-file '
            commands = [
                (f'-i {file[0]} {pipe} {encoder} -i stdin {params} {p2o} '
                 f'{file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {pipe} '
                 f'{encoder} -i stdin {params} {p2i} {file[0].with_suffix(".stat")} -b '
                 f'{file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        return commands

    def aom_vpx_encode(self, inputs):
        """AOM encoding command composition."""
        enc = self.encoders.get(self.d.get('encoder'))
        single_p = f'{enc} --passes=1 '
        two_p_1 = f'{enc} --passes=2 --pass=1'
        two_p_2 = f'{enc} --passes=2 --pass=2'
        passes = self.d.get('passes')
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")
        commands = []
        if not params:
            if enc == 'vpxenc':
                p = '--codec=vp9 --threads=4 --cpu-used=1 --end-usage=q --cq-level=40'
                self.d["video_params"], params = p, p
            if enc == 'aomenc':
                p = '--threads=4 --cpu-used=6 --end-usage=q --cq-level=40'
                self.d["video_params"], params = p, p

        if passes == 1:
            commands = [
                (f'-i {file[0]} {pipe} {single_p} {params} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        if passes == 2:
            commands = [
                (f'-i {file[0]} {pipe} {two_p_1} {params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {pipe} {two_p_2} {params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        return commands

    def rav1e_encode(self, inputs):
        """Rav1e encoding command composition."""
        passes = self.d.get('passes')
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")
        commands = []

        if not self.d.get("video_params"):
            self.d["video_params"] = ' --tiles 8 --speed 10 --quantizer 100'

        if passes in (1, 2):
            commands = [
                (f'-i {file[0]} {pipe} '
                 f' rav1e -  {params}  '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        # 2 encode pass not working with FFmpeg pipes :(
        if passes == 2:
            commands = [
                (f'-i {file[0]} {pipe} '
                 f' rav1e - --first-pass {file[0].with_suffix(".stat")} {params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 f'-i {file[0]} {pipe} '
                 f' rav1e - --second-pass {file[0].with_suffix(".stat")} {params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        return commands

    def compose_encoding_queue(self, files):
        """Composing encoding queue with split videos."""
        inputs = [(self.d.get('temp') / "split" / file.name,
                   self.d.get('temp') / "encode" / file.name,
                   file) for file in files]

        if self.d.get('encoder') in ('aom', 'vpx'):
            queue = self.aom_vpx_encode(inputs)

        elif self.d.get('encoder') == 'rav1e':
            queue = self.rav1e_encode(inputs)

        elif self.d.get('encoder') == 'svt_av1':
            queue = self.svt_av1_encode(inputs)

        self.log(f'Encoding Queue Composed\n'
                 f'Encoder: {self.d.get("encoder").upper()} Queue Size: {len(queue)} Passes: {self.d.get("passes")}\n'
                 f'Params: {self.d.get("video_params")}\n')

        # Catch Error
        if len(queue) == 0:
            print('Error in making command queue')
            self.terminate()

        return queue

    def boost(self, command: str, br_geom, new_cq=0):
        """Based on average brightness of video decrease(boost) Quantize value for encoding."""
        b_limit = self.d.get('boost_limit')
        b_range = self.d.get('boost_range')
        try:
            cq = get_cq(command)
            if not new_cq:
                if br_geom < 128:
                    new_cq = cq - round((128 - br_geom) / 128 * b_range)
                    new_cq = max(b_limit, new_cq)

                else:
                    new_cq = cq
            cmd = man_cq(command, new_cq)

            return cmd, new_cq

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')

    def plot_vmaf(self):

        if not self.d.get("vmaf"):
            return
        print('Calculating Vmaf...\r', end='')
        if self.d.get("vmaf_path"):
            model = f'model_path={self.d.get("vmaf_path")}'
        else:
            model = ''

        inp: Path = self.d.get('input')
        out: Path = self.d.get('output_file')
        xml: str = "vmaf.xml"

        # For vmaf calculation both source and encoded segment scaled to 1080
        # for proper vmaf calculation
        cmd = f'ffmpeg -hide_banner -r 60 -i {inp.as_posix()} -r 60 -i {out.as_posix()}  ' \
              f'-filter_complex "[0:v]scale=-1:1080:flags=spline[scaled1];' \
              f'[1:v]scale=-1:1080:flags=spline[scaled2];' \
              f'[scaled2][scaled1]libvmaf=log_path={xml}:{model}" -f null - '
        self.call_cmd(cmd, capture_output=True)

        if not Path(xml).exists():
            print(f'Vmaf calculation failed for files:\n {inp.stem} {out.stem}')
            self.terminate()

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
        file_name = str(self.d.get('output_file').stem) + '_plot.png'
        plt.savefig(file_name, dpi=500)

    def target_vmaf(self, source):
        # TODO speed up for vmaf stuff
        # TODO reduce complexity

        if self.d.get('vmaf_steps') < 4:
            print('Target vmaf require more than 3 probes/steps')
            self.terminate()

        vmaf_target = self.d.get('vmaf_target')
        mincq = self.d.get('min_cq')
        maxcq = self.d.get('max_cq')
        steps = self.d.get('vmaf_steps')
        frames = frame_probe(source)
        probe = source.with_suffix(".mp4")
        plot_probes = self.d.get('vmaf_plots')
        ffmpeg = self.d.get('ffmpeg')

        try:
            # Making 4 fps probing file
            cmd = f' ffmpeg -y -hide_banner -loglevel error -i {source.as_posix()} ' \
                  f'-r 4 -an {ffmpeg} -c:v libx264 -crf 0 {source.with_suffix(".mp4")}'
            self.call_cmd(cmd)

            # Make encoding fork
            q = list(np.unique(np.linspace(mincq, maxcq, num=steps, dtype=int, endpoint=True)))

            # Moving highest cq to first check, for early skips
            # checking highest first, lowers second, for early skips
            q.insert(0, q.pop(-1))
            # Encoding probes, 1 pass, highest speed
            single_p = 'aomenc  -q --passes=1 '
            params = "--threads=8 --end-usage=q --cpu-used=6 --cq-level="
            cmd = [[f'ffmpeg -y -hide_banner -loglevel error -i {probe} {self.d.get("ffmpeg_pipe")} {single_p} '
                    f'{params}{x} -o {probe.with_name(f"v_{x}{probe.stem}")}.ivf - ',
                    probe, probe.with_name(f'v_{x}{probe.stem}').with_suffix('.ivf'), x] for x in q]

            # Encoding probe and getting vmaf
            ls = []
            pr = []
            for count, i in enumerate(cmd):
                self.call_cmd(i[0])

                v = self.call_vmaf(i[1], i[2], file=True)
                _, mean, perc_1, perc_25, perc_75 = read_vmaf_xml(v)

                pr.append(round(mean, 1))
                ls.append((round(mean, 3), i[3]))

                # Early Skip on big CQ
                if count == 0 and round(mean) > vmaf_target:
                    self.log(f"File: {source.stem}, Fr: {frames}\n"
                             f"Probes: {pr}, Early Skip High CQ\n"
                             f"Target CQ: {maxcq}\n\n")
                    return maxcq, f'Target: CQ {maxcq} Vmaf: {round(mean, 2)}\n'

                # Early Skip on small CQ
                if count == 1 and round(mean) < vmaf_target:
                    self.log(f"File: {source.stem}, Fr: {frames}\n"
                             f"Probes: {pr}, Early Skip Low CQ\n"
                             f"Target CQ: {mincq}\n\n")
                    return mincq, f'Target: CQ {mincq} Vmaf: {round(mean, 2)}\n'

            x = [x[1] for x in sorted(ls)]
            y = [float(x[0]) for x in sorted(ls)]

            # Interpolate data
            f = interpolate.interp1d(x, y, kind='cubic')
            xnew = np.linspace(min(x), max(x), max(x) - min(x))

            # Getting value closest to target
            tl = list(zip(xnew, f(xnew)))
            vmaf_target_cq = min(tl, key=lambda x: abs(x[1] - vmaf_target))

            if plot_probes:
                # Saving plot of got data
                plt.plot(x, y, 'x', color='tab:blue')
                plt.plot(xnew, f(xnew), color='tab:blue')
                plt.plot(vmaf_target_cq[0], vmaf_target_cq[1], 'o', color='red')

                for x in range(0, 100):
                    plt.axhline(x, color='grey', linewidth=0.4)
                    plt.axvline(x, color='grey', linewidth=0.3)

                    if x % 5 == 0:
                        plt.axhline(x, color='black', linewidth=0.6)

                plt.xlim(mincq, maxcq)
                vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
                plt.ylim(min(vmafs), max(vmafs) + 1)
                plt.ylabel('VMAF')
                plt.xlabel('CQ')
                plt.title(f'Chunk: {probe.stem}, Frames: {frames}')
                plt.tight_layout()
                temp = self.d.get('temp') / probe.stem
                plt.savefig(temp, dpi=300)
                plt.close()

            self.log(f"File: {source.stem}, Fr: {frames}\n"
                     f"Probes: {sorted(pr)}\n"
                     f"Target CQ: {round(vmaf_target_cq[0])}\n\n")

            return int(vmaf_target_cq[0]), f'Target: CQ {int(vmaf_target_cq[0])} Vmaf: {round(float(vmaf_target_cq[1]), 2)}\n'

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in vmaf_target {e} \nAt line {exc_tb.tb_lineno}')
            self.terminate()

    def encode(self, commands):
        """Single encoder command queue and logging output."""
        counter = commands[1]
        commands = commands[0]
        encoder = self.d.get('encoder')
        # Passing encoding params to ffmpeg for encoding.
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly.
        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = frame_probe(source)

            # Target Vmaf Mode
            if self.d.get('vmaf_target'):
                tg_cq, tg_vf = self.target_vmaf(source)

                cm1 = man_cq(commands[0], tg_cq)

                if self.d.get('passes') == 2:
                    cm2 = man_cq(commands[1], tg_cq)
                    commands = (cm1, cm2) + commands[2:]
                else:
                    commands = cm1 + commands[1:]

            else:
                tg_vf = ''

            # Boost
            if self.d.get('boost'):
                br = get_brightness(source.absolute().as_posix())

                com0, cq = self.boost(commands[0], br)

                if self.d.get('passes') == 2:
                    com1, _ = self.boost(commands[1], br, cq)
                    commands = (com0, com1) + commands[2:]
                else:
                    commands = com0 + commands[1:]

                boost = f'Avg brightness: {br}\nAdjusted CQ: {cq}\n'
            else:
                boost = ''

            self.log(f'Enc: {source.name}, {frame_probe_source} fr\n{tg_vf}{boost}\n')

            # Queue execution
            for i in commands[:-1]:
                f, e = i.split('|')
                f = " ffmpeg -y -hide_banner -loglevel error " + f
                f, e = f.split(), e.split()
                try:
                    frame = 0

                    ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
                    pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                                            stderr=STDOUT,
                                            universal_newlines=True)
                    if encoder in ('aom', 'vpx'):
                        while True:
                            line = pipe.stdout.readline().strip()
                            if len(line) == 0 and pipe.poll() is not None:
                                break
                            if 'Pass 2/2' in line or 'Pass 1/1' in line:
                                match = re.search(r"frame.*?\/([^ ]+?) ", line)
                                if match:
                                    new = int(match.group(1))
                                    if new > frame:
                                        counter.update(new - frame)
                                        frame = new

                    elif encoder == 'rav1e':
                        while True:
                            line = pipe.stdout.readline().strip()
                            if len(line) == 0 and pipe.poll() is not None:
                                break
                            match = re.search(r"encoded.*? ([^ ]+?) ", line)
                            if match:
                                new = int(match.group(1))
                                if new > frame:
                                    counter.update(new - frame)
                                    frame = new

                    elif encoder == 'svt_av1':
                        while True:
                            line = pipe.stdout.readline().strip()
                            if len(line) == 0 and pipe.poll() is not None:
                                break
                        counter.update(frame_probe_source)

                except Exception as e:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')

            self.frame_check(source, target)

            frame_probe_fr = frame_probe(target)

            enc_time = round(time.time() - st_time, 2)

            self.log(f'Done: {source.name} Fr: {frame_probe_fr}\n'
                     f'Fps: {round(frame_probe_fr / enc_time, 4)} Time: {enc_time} sec.\n\n')
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')

    def concatenate_video(self):
        """With FFMPEG concatenate encoded segments into final file."""
        with open(f'{self.d.get("temp") / "concat" }', 'w') as f:

            encode_files = sorted((self.d.get('temp') / 'encode').iterdir())
            f.writelines(f"file '{file.absolute()}'\n" for file in encode_files)

        # Add the audio file if one was extracted from the input
        audio_file = self.d.get('temp') / "audio.mkv"
        if audio_file.exists():
            audio = f'-i {audio_file} -c:a copy'
        else:
            audio = ''

        try:
            cmd = f' ffmpeg -y -hide_banner -loglevel error -f concat -safe 0 -i {self.d.get("temp") / "concat"} ' \
                  f'{audio} -c copy -y "{self.d.get("output_file")}"'
            concat = self.call_cmd(cmd, capture_output=True)
            if len(concat) > 0:
                raise Exception

            self.log('Concatenated\n')

            # Delete temp folders
            if not self.d.get('keep'):
                shutil.rmtree(self.d.get('temp'))

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, FFmpeg error\nAt line: {exc_tb.tb_lineno}\nError:{str(concat)}')
            self.log(f'Concatenation failed, aborting, error: {e}\n')
            self.terminate()

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        try:
            enc_path = self.d.get('temp') / 'split'
            done_path = self.d.get('temp') / 'done.json'

            if self.d.get('resume') and done_path.exists():
                self.log('Resuming...\n')

                with open(done_path) as f:
                    data = json.load(f)

                total = data['total']
                done = len(data['done'])
                initial = sum(data['done'].values())

                self.log(f'Resumed with {done} encoded clips done\n\n')
            else:
                initial = 0
                total = frame_probe(self.d.get('input'))
                d = {'total': total, 'done': {}}
                with open(done_path, 'w') as f:
                    json.dump(d, f)

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            w = min(self.d.get('workers'), clips)

            print(f'\rQueue: {clips} Workers: {w} Passes: {self.d.get("passes")}\n'
                  f'Params: {self.d.get("video_params").strip()}')

            with concurrent.futures.ThreadPoolExecutor(max_workers=self.d.get('workers')) as executor:
                counter = Manager().Counter(total, initial)
                future_cmd = {executor.submit(self.encode, (cmd, counter)): cmd for cmd in commands}
                for future in concurrent.futures.as_completed(future_cmd):
                    future_cmd[future]
                    try:
                        future.result()
                    except Exception as exc:
                        print(f'Encoding error: {exc}')
                        self.terminate()
        except KeyboardInterrupt:
            self.terminate()

    def aom_keyframes(self):
        """[Get frame numbers for splits from aomenc 1 pass stat file]
        """
        video: Path = self.d.get("input")
        stat_file = self.d.get('temp') / 'keyframes.log'

        f, e = f'ffmpeg -y -hide_banner -loglevel error -i {video.as_posix()}   -strict -1 -pix_fmt yuv420p -f yuv4mpegpipe - | aomenc --passes=2 --pass=1 --threads=12 --cpu-used=0 --end-usage=q --cq-level=40 --fpf={stat_file.as_posix()} -o {os.devnull} -'.split('|')
        f, e = f.split(), e.split()

        # Getting Frame Count from Metadata
        video = cv2.VideoCapture(video.as_posix())
        total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
        video.release()

        tqdm_bar = tqdm(total=total, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)

        ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
        pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                                stderr=STDOUT, universal_newlines=True)
        frame = 0
        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break
            match = re.search(r"frame.*?\/([^ ]+?) ", line)
            if match:
                new = int(match.group(1))
                if new > frame:
                    tqdm_bar.update(new - frame)
                frame = new

        keyframes = find_aom_keyframes(stat_file)
        keyframes = ','.join(keyframes[1:])

        return keyframes

    def extra_split(self, frames):
        if len(frames) > 0:
            f = literal_eval(frames)
            if len(f) > 1:
                f = list(f)
        else:
            f = []
        f.append(frame_probe(self.d.get('input')))
        split_distance = self.d.get('extra_split')

        # Get all keyframes of original video
        keyframes = get_keyframes(self.d.get('input'))

        t = f[:]
        t.insert(0, 0)
        splits = list(zip(t, f))
        for i in splits:
            # Getting distance between splits
            distance = (i[1] - i[0])

            if distance > split_distance:
                # Keyframes that between 2 split points
                candidates = [k for k in keyframes if i[1] > k > i[0]]

                if len(candidates) > 0:
                    # Getting number of splits that need to be inserted
                    to_insert = min((i[1] - i[0]) // split_distance, (len(candidates)))
                    for k in range(0, to_insert):
                        # Approximation of splits position
                        aprox_to_place = (((k + 1) * distance) // (to_insert + 1)) + i[0]

                        # Getting keyframe closest to approximated
                        key = min(candidates, key=lambda x: abs(x - aprox_to_place))
                        f.append(key)
        self.log(f'Applying extra splits\nSplit distance: {split_distance}\nNew splits:{len(f)}\n')
        result = [str(x) for x in sorted(f)]
        result = ','.join(result)
        return result

    def split_routine(self):

        if self.d.get('scenes') == '0':
            self.log('Skipping scene detection\n')
            return ''

        split_method = self.d.get('split_method')
        sc = ''

        scenes = self.d.get('scenes')
        video = self.d.get('input')

        if scenes:
            scenes = Path(scenes)
            if scenes.exists():
                # Read stats from CSV file opened in read mode:
                with scenes.open() as stats_file:
                    stats = stats_file.read().strip()
                    self.log('Using Saved Scenes\n')
                    return stats

        # Splitting using PySceneDetect
        if split_method == 'pyscene':
            queue_fix = not self.d.get('queue')
            threshold = self.d.get("threshold")
            self.log(f'Starting scene detection Threshold: {threshold}\n')
            try:
                sc = pyscene(video, threshold, progress_show=queue_fix )
            except Exception as e:
                self.log(f'Error in PySceneDetect: {e}\n')
                print(f'Error in PySceneDetect{e}\n')
                self.terminate()

        # Splitting based on aom keyframe placement
        elif split_method == 'aom_keyframes':
            try:
                sc = self.aom_keyframes()
            except:
                self.log('Error in aom_keyframes')
                print('Error in aom_keyframes')
                self.terminate()
        else:
            print(f'No valid split option: {split_method}\nValid options: "pyscene", "aom_keyframes"')
            self.terminate()



        self.log(f'Found scenes: {len(sc)}\n')

        # Fix for windows character limit
        if sys.platform != 'linux':
            if len(sc) > 600:
                sc = reduce_scenes(sc)

        # Write scenes to file

        if scenes:
            Path(scenes).write_text(sc)

        return sc

    def setup_routine(self):
        """
        All pre encoding routine.
        Scene detection, splitting, audio extraction
        """
        if self.d.get('resume') and (self.d.get('temp') / 'done.json').exists():
            self.set_logging()

        else:
            self.setup()
            self.set_logging()

            # Splitting video and sorting big-first

            framenums = self.split_routine()

            if self.d.get('extra_split'):
                framenums = self.extra_split(framenums)

            self.split(framenums)

            # Extracting audio
            self.extract_audio(self.d.get('input'))

    def video_encoding(self):
        """Encoding video on local machine."""
        self.outputs_filenames()
        self.setup_routine()

        files = self.get_video_queue(self.d.get('temp') / 'split')

        # Make encode queue
        commands = self.compose_encoding_queue(files)

        # Determine resources if workers don't set
        self.determine_resources()

        self.encoding_loop(commands)

        self.concatenate_video()

        self.plot_vmaf()

    def main_queue(self):
        # Video Mode. Encoding on local machine
        tm = time.time()
        if self.d.get('queue'):
            for file in self.d.get('queue'):
                tm = time.time()
                self.d['input'] = file
                print(f'Encoding: {file}')
                self.d['output_file'] = None
                self.video_encoding()
                print(f'Finished: {round(time.time() - tm, 1)}s\n')
        else:
            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s')

    def main_thread(self):
        """Main."""
        self.arg_parsing()

        # Read/Set parameters
        self.config()

        # Check all executables
        self.check_executables()

        self.process_inputs()
        self.main_queue()


def main():
    # Main thread
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


if __name__ == '__main__':
    main()
