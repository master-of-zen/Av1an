#!/usr/bin/env python3

import time
import pickle
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
from multiprocessing import Pool
import multiprocessing
import subprocess
from pathlib import Path
import cv2
import numpy as np
import statistics
from scipy import interpolate
import matplotlib.pyplot as plt
from scenedetect.video_manager import VideoManager
from scenedetect.scene_manager import SceneManager
from scenedetect.detectors import ContentDetector
from multiprocessing.managers import BaseManager

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


class Counter(object):
    def __init__(self, total, initial):
        self.bar = tqdm(total=total, initial=initial, dynamic_ncols=True, unit="fr", leave=False)

    def update(self, value):
        self.bar.update(value)


MyManager.register('Counter', Counter)


class Av1an:

    def __init__(self):
        """Av1an - Python all-in-one toolkit for AV1, VP9, VP8 encodes."""
        self.FFMPEG = 'ffmpeg -y -hide_banner -loglevel error '
        self.d = dict()
        self.encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc'}

    @staticmethod
    def get_cq(self, command):
        """Return cq values from command"""
        matches = re.findall(r"--cq-level= *([^ ]+?) ", command)
        return int(matches[-1])

    @staticmethod
    def man_cq(command: str, cq: int):
        """Return command with new cq value"""
        mt = '--cq-level='
        cmd = command[:command.find(mt) + 11] + str(cq) + command[command.find(mt) + 13:]
        return cmd

    @staticmethod
    def frame_probe(source: Path):
        """Get frame count."""
        cmd = ["ffmpeg", "-hide_banner", "-i", source.absolute(), "-map", "0:v:0", "-f", "null", "-"]
        r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        matches = re.findall(r"frame= *([^ ]+?) ", r.stderr.decode("utf-8") + r.stdout.decode("utf-8"))
        return int(matches[-1])

    @staticmethod
    def get_brightness(video):
        """Getting average brightness value for single video."""
        brightness = []
        cap = cv2.VideoCapture(video)
        try:
            while True:
                # Capture frame-by-frame
                _, frame = cap.read()

                # Our operations on the frame come here
                gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)

                # Display the resulting frame
                mean = cv2.mean(gray)
                brightness.append(mean[0])
                if cv2.waitKey(1) & 0xFF == ord('q'):
                    break
        except cv2.error:
            pass

        # When everything done, release the capture
        cap.release()
        brig_geom = round(statistics.geometric_mean([x + 1 for x in brightness]), 1)

        return brig_geom

    def log(self, info):
        """Default logging function, write to file."""
        with open(self.d.get('logging'), 'a') as log:
            log.write(time.strftime('%X') + ' ' + info)

    def call_cmd(self, cmd, capture_output=False):
        """Calling system shell, if capture_output=True output string will be returned."""
        if capture_output:
            return subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).stdout

        with open(self.d.get('logging'), 'a') as log:
            subprocess.run(cmd, shell=True, stdout=log, stderr=log)

    def check_executables(self):
        if not find_executable('ffmpeg'):
            print('No ffmpeg')
            sys.exit()

        # Check if encoder executable is reachable
        if self.d.get('encoder') in self.encoders:
            enc = self.encoders.get(self.d.get('encoder'))

            if not find_executable(enc):
                print(f'Encoder {enc} not found')
                sys.exit()
        else:
            print(f'Not valid encoder {self.d.get("encoder")} ')
            sys.exit()

    def process_inputs(self):
        # Check input file for being valid
        if not self.d.get('input'):
            print('No input file')
            sys.exit()

        inputs = self.d.get('input')
        valid = np.array([i.exists() for i in inputs])

        if not all(valid):
            print(f'File(s) do not exist: {", ".join([str(inputs[i]) for i in np.where(valid == False)[0]])}')
            sys.exit()

        if len(inputs) > 1:
            self.d['queue'] = inputs
        else:
            self.d['input'] = inputs[0]

    def read_config(self):
        """Creation and reading of config files with saved settings"""
        cfg = self.d.get('config')
        if cfg:
            if cfg.exists():
                with open(cfg, 'rb') as f:
                    c: dict = dict(pickle.load(f))
                    self.d.update(c)

            else:
                with open(cfg, 'wb') as f:
                    c = dict()
                    c['video_params'] = self.d.get('video_params')
                    c['encoder'] = self.d.get('encoder')
                    c['ffmpeg'] = self.d.get('ffmpeg')
                    c['audio_params'] = self.d.get('audio_params')
                    pickle.dump(c, f)

    def arg_parsing(self):
        """Command line parse and sanity checking."""
        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=0, help='0 - local, 1 - master, 2 - encoder')

        # Input/Output/Temp
        parser.add_argument('--input', '-i', nargs='+', type=Path, help='Input File')  # nargs = '+'
        parser.add_argument('--temp', type=Path, default=Path('.temp'), help='Set temp folder path')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')

        # PySceneDetect
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
        parser.add_argument('--boost', help='Experimental feature', action='store_true')
        parser.add_argument('--boost_range', default=15, type=int, help='Range/strenght of CQ change')
        parser.add_argument('--boost_limit', default=10, type=int, help='CQ limit for boosting')

        # Vmaf
        parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        parser.add_argument('--vmaf_path', type=str, default=None, help='Path to vmaf models')

        # Target Vmaf
        parser.add_argument('--vmaf_target', type=float, help='Value of Vmaf to target')
        parser.add_argument('--vmaf_error', type=float, default=0.0, help='Error to compensate to wrong target vmaf')
        parser.add_argument('--vmaf_steps', type=int, default=5, help='Steps between min and max qp for target vmaf')
        parser.add_argument('--min_cq', type=int, default=20, help='Min cq for target vmaf')
        parser.add_argument('--max_cq', type=int, default=60, help='Max cq for target vmaf')

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
        """Setting logging file."""
        if self.d.get('logging'):
            self.d['logging'] = f"{self.d.get('logging')}.log"
        else:
            self.d['logging'] = self.d.get('temp') / 'log.log'

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
        audio_file = self.d.get('temp') / 'audio.mkv'
        if audio_file.exists():
            self.log('Reusing Audio File\n')
            return

        # Checking is source have audio track
        check = fr'{self.FFMPEG} -ss 0 -i "{input_vid}" -t 0 -vn -c:a copy -f null -'
        is_audio_here = len(self.call_cmd(check, capture_output=True)) == 0

        # If source have audio track - process it
        if is_audio_here:
            self.log(f'Audio processing\n'
                     f'Params: {self.d.get("audio_params")}\n')
            cmd = f'{self.FFMPEG} -i "{input_vid}" -vn ' \
                  f'{self.d.get("audio_params")} {audio_file}'
            self.call_cmd(cmd)

    def call_vmaf(self, source: Path, encoded: Path):
        if self.d.get("vmaf_path"):
            model = f'model_path={self.d.get("vmaf_path")}'
        else:
            model = ''

        # For vmaf calculation both source and encoded segment scaled to 1080
        # for proper vmaf calculation
        cmd = f'ffmpeg -hide_banner -i {source.as_posix()} -i {encoded.as_posix()}  ' \
              f'-filter_complex "[0:v]scale=-1:1080:flags=spline[scaled1];' \
              f'[1:v]scale=-1:1080:flags=spline[scaled2];' \
              f'[scaled2][scaled1]libvmaf=log_path={source.with_name(encoded.stem).as_posix()}.xml:{model}" -f null - '

        call = self.call_cmd(cmd, capture_output=True)
        result = call.decode().strip().split()
        return result

    def get_vmaf(self, source: Path, encoded: Path):

        result = self.call_vmaf(source, encoded)

        if 'monotonically' in result:
            self.log(''.join(result))
            return 'Nan. Bad dts'
        try:
            res = float(result[-1])
            return res
        except ValueError:
            return 'Nan'

    def reduce_scenes(self, scenes):
        """Windows terminal can't handle more than ~600 scenes in length."""
        if len(scenes) > 600:
            scenes = scenes[::2]
            self.reduce_scenes(scenes)
        return scenes

    def scene_detect(self, video: Path):
        """
        Running PySceneDetect detection on source video for segmenting.
        Optimal threshold settings 15-50
        """
        # Skip scene detection if the user choose to
        if self.d.get('scenes') == '0':
            self.log('Skipping scene detection\n')
            return ''

        try:
            video_manager = VideoManager([str(video)])
            scene_manager = SceneManager()
            scene_manager.add_detector(ContentDetector(threshold=self.d.get('threshold')))
            base_timecode = video_manager.get_base_timecode()

            # If stats file exists, load it.
            scenes = self.d.get('scenes')
            if scenes:
                scenes = Path(scenes)
                if scenes.exists():
                    # Read stats from CSV file opened in read mode:
                    with scenes.open() as stats_file:
                        stats = stats_file.read()
                        self.log('Using Saved Scenes\n')
                        return stats

            # Work on whole video
            video_manager.set_duration()

            # Set downscale factor to improve processing speed.
            video_manager.set_downscale_factor()

            # Start video_manager.
            video_manager.start()

            # Perform scene detection on video_manager.
            self.log(f'Starting scene detection Threshold: {self.d.get("threshold")}\n')

            # Fix for cli batch encoding
            progress = False if self.d.get('queue') else True

            scene_manager.detect_scenes(frame_source=video_manager, show_progress=progress)

            # Obtain list of detected scenes.
            scene_list = scene_manager.get_scene_list(base_timecode)

            self.log(f'Found scenes: {len(scene_list)}\n')

            scenes = [str(scene[0].get_frames()) for scene in scene_list]

            # Fix for windows character limit
            if sys.platform != 'linux':
                scenes = self.reduce_scenes(scenes)

            scenes = ','.join(scenes[1:])

            # We only write to the stats file if a save is required:
            if self.d.get('scenes'):
                Path(self.d.get('scenes')).write_text(scenes)
            return scenes

        except Exception as e:
            self.log(f'Error in PySceneDetect: {e}\n')
            print(f'Error in PySceneDetect{e}\n')
            sys.exit()

    def split(self, video: Path, frames):
        """Split video by frame numbers, or just copying video."""

        cmd = [
            "ffmpeg", "-y",
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
        subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    def frame_check(self, source: Path, encoded: Path):
        """Checking is source and encoded video frame count match."""
        try:
            status_file = Path(self.d.get("temp") / 'done.txt')

            if self.d.get("no_check"):
                s1 = self.frame_probe(source)
                with status_file.open('a') as done:
                    done.write(f'({s1}, "{source.name}"), ')
                    return

            s1, s2 = [self.frame_probe(i) for i in (source, encoded)]

            if s1 == s2:
                with status_file.open('a') as done:
                    done.write(f'({s1}, "{source.name}"), ')
            else:
                print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'\nError frame_check: {e}\nAt line: {exc_tb.tb_lineno}\n')

    def get_video_queue(self, source_path: Path):
        """Returns sorted list of all videos that need to be encoded. Big first."""
        queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']

        if self.d.get('resume'):
            done_file = self.d.get('temp') / 'done.txt'
            try:
                if done_file.exists():
                    with open(done_file, 'r') as f:
                        data = [line for line in f]
                        data = literal_eval(data[1])
                        queue = [x for x in queue if x.name not in [x[1] for x in data]]
            except Exception as e:
                _, _, exc_tb = sys.exc_info()
                print(f'Error at resuming {e}\nAt line {exc_tb.tb_lineno}')

        queue = sorted(queue, key=lambda x: -x.stat().st_size)

        if len(queue) == 0:
            print('Error: No files found in .temp/split, probably splitting not working')
            sys.exit()

        return queue

    def svt_av1_encode(self, inputs):
        """SVT-AV1 encoding command composition."""
        encoder = 'SvtAv1EncApp'
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")
        passes = self.d.get('passes')

        if not params:
            print('-w -h -fps is required parameters for svt_av1 encoder')
            sys.exit()

        if passes == 1:
            pass_1_commands = [
                (f'-i {file[0]} {pipe} ' +
                 f'  {encoder} -i stdin {params} -b {file[1].with_suffix(".ivf")} -',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_1_commands

        if passes == 2:
            p2i = '-input-stat-file '
            p2o = '-output-stat-file '
            pass_2_commands = [
                (f'-i {file[0]} {pipe} {encoder} -i stdin {params} {p2o} '
                 f'{file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {pipe} '
                 f'{encoder} -i stdin {params} {p2i} {file[0].with_suffix(".stat")} -b {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_2_commands

    def aom_vpx_encode(self, inputs):
        """AOM encoding command composition."""
        enc = self.encoders.get(self.d.get('encoder'))
        single_p = f'{enc} --passes=1 '
        two_p_1 = f'{enc} --passes=2 --pass=1'
        two_p_2 = f'{enc} --passes=2 --pass=2'
        passes = self.d.get('passes')
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")

        if not params:
            if enc == 'vpxenc':
                p = '--codec=vp9 --threads=4 --cpu-used=1 --end-usage=q --cq-level=40'
                self.d["video_params"], params = p, p
            if enc == 'aomenc':
                p = '--threads=4 --cpu-used=6 --end-usage=q --cq-level=40'
                self.d["video_params"], params = p, p

        if passes == 1:
            pass_1_commands = [
                (f'-i {file[0]} {pipe} {single_p} {params} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_1_commands

        if passes == 2:
            pass_2_commands = [
                (f'-i {file[0]} {pipe} {two_p_1} {params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {pipe} {two_p_2} {params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_2_commands

    def rav1e_encode(self, inputs):
        """Rav1e encoding command composition."""
        passes = self.d.get('passes')
        pipe = self.d.get("ffmpeg_pipe")
        params = self.d.get("video_params")

        if not self.d.get("video_params"):
            self.d["video_params"] = ' --tiles=4 --speed=10 --quantizer 100'

        if passes == 1 or passes == 2:
            pass_1_commands = [
                (f'-i {file[0]} {pipe} '
                 f' rav1e -  {params}  '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_1_commands

        # 2 encode pass not working with FFmpeg pipes :(
        if passes == 2:
            pass_2_commands = [
                (f'-i {file[0]} {pipe} '
                 f' rav1e - --first-pass {file[0].with_suffix(".stat")} {params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 f'-i {file[0]} {pipe} '
                 f' rav1e - --second-pass {file[0].with_suffix(".stat")} {params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]
            return pass_2_commands

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
            sys.exit()

        return queue

    def boost(self, command: str, br_geom, new_cq=0):
        """Based on average brightness of video decrease(boost) Quantize value for encoding."""
        b_limit = self.d.get('boost_limit')
        b_range = self.d.get('boost_range')
        try:
            cq = self.get_cq(command)
            if not new_cq:
                if br_geom < 128:
                    new_cq = cq - round((128 - br_geom) / 128 * b_range)
                    new_cq = max(b_limit, new_cq)

                else:
                    new_cq = cq
            cmd = self.man_cq(command, new_cq)

            return cmd, new_cq

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')

    def plot_vmaf(self):
        with open(self.d.get('temp') / 'vmaf.txt', 'r') as f:
            data = literal_eval(f.read())
            d = sorted(data, key=lambda x: x[0])

        try:
            plot_data = []
            for point in d:
                vmaf = point[2]
                if isinstance(vmaf, str):
                    vmaf = None
                frames = point[1]
                for i in range(frames):
                    plot_data.append(vmaf)

            x1 = range(len(plot_data))
            y1 = plot_data

            # Plot
            plt.plot(x1, y1)
            real_y = [i for i in y1 if i]

            if len(real_y) == 0:
                print('No valid vmaf values')
                plt.close()
                return

            plt.ylim((int(min(real_y)), 100))
            for i in range(int(min(real_y)), 100, 1):
                plt.axhline(i, color='grey', linewidth=0.5)
            vm = self.d.get('vmaf_target')
            if vm:
                plt.hlines(vm, 0, len(x1), colors='red')
            plt.hlines(sum(real_y) / len(real_y), 0, len(x1), colors='blue')

            # Save/close
            plt.ylabel('VMAF')
            plt.xlabel('Frames')
            plt.title(f'{self.d.get("input").stem}, {frames} ')
            plt.tight_layout()
            plt.savefig(self.d.get('input').stem, dpi=600)
            plt.close()

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'\nError in vmaf plot: {e}\nAt line: {exc_tb.tb_lineno}\n')

    def target_vmaf(self, source, command):
        try:
            if self.d.get('vmaf_steps') < 5:
                print('Target vmaf require more than 4 probes/steps')
                sys.exit()

            tg = self.d.get('vmaf_target')
            mincq = self.d.get('min_cq')
            maxcq = self.d.get('max_cq')
            steps = self.d.get('vmaf_steps')
            frames = self.frame_probe(source)

            # Making 3fps probing file
            cq = self.man_cq(command, -1)
            probe = source.with_suffix(".mp4")
            cmd = f'{self.FFMPEG} -i {source.absolute().as_posix()} ' \
                  f'-r 3 -an -c:v libx264 -crf 0 {source.with_suffix(".mp4")}'
            self.call_cmd(cmd)

            # Make encoding fork
            q = np.unique(np.linspace(mincq, maxcq, num=steps, dtype=int, endpoint=True))

            # Encoding probes
            single_p = 'aomenc  -q --passes=1 '
            params = "--threads=4 --end-usage=q --cpu-used=6 --cq-level="
            cmd = [[f'{self.FFMPEG} -i {probe} {self.d.get("ffmpeg_pipe")} {single_p} '
                    f'{params}{x} '
                    f'-o {probe.with_name(f"v_{x}{probe.stem}")}.ivf - ',
                    probe, probe.with_name(f'v_{x}{probe.stem}').with_suffix('.ivf'), x] for x in q]

            # Encoding probe and getting vmaf
            ls = []
            pr = []
            for i in cmd:
                self.call_cmd(i[0])
                v = self.get_vmaf(i[1], i[2])
                if isinstance(v, str):
                    return int(cq), 'Error in vmaf calculation\n'
                pr.append(round(v, 1))
                ls.append((v, i[3]))
            x = [x[1] for x in ls]
            y = [float(x[0]) for x in ls]

            # Interpolate data
            f = interpolate.interp1d(x, y, kind='cubic')

            xnew = np.linspace(min(x), max(x), max(x) - min(x))

            # Getting value closest to target
            tl = list(zip(xnew, f(xnew)))
            tg_cq = min(tl, key=lambda x: abs(x[1] - tg))

            # Try control encode
            # Get full fps fast encode to get error, and when adjust graph
            # Based on error and return adjusted value

            probe_name = Path(f"{source.with_name(f'x_{source.stem}')}.ivf")
            run_cmd = f" {self.FFMPEG} -i {source.absolute().as_posix()} {self.d.get('ffmpeg_pipe')} {single_p} " \
                      f"{params}{int(tg_cq[0])} -o {probe_name} -"
            self.call_cmd(run_cmd)

            new_vmaf = self.get_vmaf(source, probe_name)

            if isinstance(new_vmaf, str):
                return int(cq), 'Error in vmaf calculation'
            # If real is lower - number negative
            difference = -(new_vmaf - float(tg_cq[1]))

            """
            if difference > 10:
                print('Invalidate difference: ', round(difference, 2), 'Reset to 0')
                difference = 0
            else:
                print('Difference: ', round(difference, 2))
            """

            y2 = [v - difference for v in y]
            new_interpolate = interpolate.interp1d(x, y2, kind='cubic')
            new_line = list(zip(xnew, new_interpolate(xnew)))
            # New target cq for vmaf
            new_tg_cq = min(new_line, key=lambda x: abs(x[1] - tg))

            # Saving plot of got data
            # Plot first
            plt.plot(x, y, 'x', color='blue')
            plt.plot(xnew, f(xnew), color='blue')
            plt.plot(tg_cq[0], tg_cq[1], 'o', color='blue')

            # Plot corrected
            plt.plot(xnew, new_interpolate(xnew), color='green')
            plt.plot(new_tg_cq[0], new_tg_cq[1], 'o', color='green')
            plt.plot(x, y2, 'x', color='green')

            mn = [x[1] for x in tl] + [x[1] for x in new_line]
            mn = [int(x) for x in mn]
            mn = min(mn)
            for i in range(mn, 100, 1):
                plt.axhline(i, color='grey', linewidth=0.5)

            for i in range(int(min(xnew)), int(max(xnew)) + 1, 5):
                plt.axvline(i, color='grey', linewidth=0.5)
            plt.ylabel('vmaf')
            plt.xlabel('cq')
            plt.title(f'Chunk: {probe.stem}, Frames: {frames}')  # Add frame count
            plt.tight_layout()
            plt.savefig(probe.stem, dpi=300)
            plt.close()
            self.log(f"File: {source.stem}, {frames}\n"
                     f"New vmaf: {new_vmaf}\n"
                     f"Probes: {pr}"
                     f"Target CQ: {round(tg_cq[0])}, Dif: {round(difference, 2)}\n")
            return int(tg_cq[0]), f'Target: CQ {int(new_tg_cq[0])} Vmaf: {round(float(new_tg_cq[1]), 2)}\n'

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in vmaf_target {e} \nAt line {exc_tb.tb_lineno}')

    def encode(self, commands):
        counter = commands[1]
        commands = commands[0]
        """Single encoder command queue and logging output."""
        encoder = self.d.get('encoder')
        # Passing encoding params to ffmpeg for encoding.
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly.
        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = self.frame_probe(source)

            if self.d.get('vmaf_target'):

                # Make sure that vmaf calculated after encoding
                self.d['vmaf'] = True

                tg_cq, tg_vf = self.target_vmaf(source, commands[0])

                cm1 = self.man_cq(commands[0], tg_cq)

                if self.d.get('passes') == 2:
                    cm2 = self.man_cq(commands[1], tg_cq)
                    commands = (cm1, cm2) + commands[2:]
                else:
                    commands = cm1 + commands[1:]

            else:
                tg_vf = ''

            if self.d.get('boost'):
                br = self.get_brightness(source.absolute().as_posix())

                com0, cq = self.boost(commands[0], br)

                if self.d.get('passes') == 2:
                    com1, _ = self.boost(commands[1], br, cq)
                    commands = (com0, com1) + commands[2:]
                else:
                    commands = com0 + commands[1:]

                boost = f'Avg brightness: {br}\nAdjusted CQ: {cq}\n'
            else:
                boost = ''

            self.log(f'Enc:  {source.name}, {frame_probe_source} fr\n{tg_vf}{boost}\n')

            # Queue execution
            for i in commands[:-1]:
                f, e = i.split('|')
                f = self.FFMPEG + f
                f, e = f.split(), e.split()

                try:
                    frame = 0

                    ffmpeg_pipe = subprocess.Popen(f, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                    pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=subprocess.PIPE,
                                            stderr=subprocess.STDOUT,
                                            universal_newlines=True)
                    if encoder == 'aom' or encoder == 'vpx':
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
                    if encoder == 'rav1e':
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
                    if encoder == 'svt_av1':
                        while True:
                            line = pipe.stdout.readline().strip()
                            if len(line) == 0 and pipe.poll() is not None:
                                break
                        counter.update(frame_probe_source)

                except Exception as e:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')

            self.frame_check(source, target)

            frame_probe = self.frame_probe(target)

            enc_time = round(time.time() - st_time, 2)

            if self.d.get('vmaf'):
                v = self.get_vmaf(source, target)
                if isinstance(v, str):
                    vmaf = f'Vmaf: {v}\n'
                    v = None
                else:
                    vmaf = f'Vmaf: {round(v, 2)}\n'
                    v = round(v, 2)

                with open(self.d.get('temp') / 'vmaf.txt', 'a') as f:
                    f.write(f'({str(int(source.stem))},{frame_probe_source},{v}),')

            else:
                vmaf = ''

            self.log(f'Done: {source.name} Fr: {frame_probe}\n'
                     f'Fps: {round(frame_probe / enc_time, 4)} Time: {enc_time} sec.\n{vmaf}\n')
            return self.frame_probe(source)
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
            cmd = f'{self.FFMPEG} -f concat -safe 0 -i {self.d.get("temp") / "concat"} ' \
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
            sys.exit()

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        enc_path = self.d.get('temp') / 'split'
        done_path = self.d.get('temp') / 'done.txt'

        if self.d.get('resume') and done_path.exists():

            self.log('Resuming...\n')
            with open(done_path, 'r') as f:
                lines = [line for line in f]
                if len(lines) > 1:
                    data = literal_eval(lines[-1])
                    total = int(lines[0])
                    done = len([x[1] for x in data])
                    initial = sum([int(x[0]) for x in data])
                else:
                    done = 0
                    initial = 0
                    total = self.frame_probe(self.d.get('input'))
            self.log(f'Resumed with {done} encoded clips done\n\n')

        else:
            initial = 0
            with open(Path(self.d.get('temp') / 'done.txt'), 'w') as f:
                total = self.frame_probe(self.d.get('input'))
                f.write(f'{total}\n')

        clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
        w = min(self.d.get('workers'), clips)

        print(f'\rQueue: {clips} Workers: {w} Passes: {self.d.get("passes")}\n'
              f'Params: {self.d.get("video_params")}')

        with Pool(w) as pool:
            manager = Manager()
            counter = manager.Counter(total, initial)
            commands = [(x, counter) for x in commands]
            loop = pool.imap_unordered(self.encode, commands)
            self.log(f'Started encoding queue with {self.d.get("workers")} workers\n\n')

            try:
                for _ in loop:
                    pass
            except Exception as e:
                _, _, exc_tb = sys.exc_info()
                print(f'Encoding error: {e}\nAt line {exc_tb.tb_lineno}')
                sys.exit()

    def setup_routine(self):
        """
        All pre encoding routine.
        Scene detection, splitting, audio extraction
        """
        if self.d.get('resume') and (self.d.get('temp') / 'done.txt').exists():
            self.set_logging()

        else:
            self.setup()
            self.set_logging()

            # Splitting video and sorting big-first
            framenums = self.scene_detect(self.d.get('input'))
            self.split(self.d.get('input'), framenums)

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
        if self.d.get('workers') != 0:
            self.d['workers'] = self.d.get('workers')
        else:
            self.determine_resources()

        self.encoding_loop(commands)

        if self.d.get('vmaf'):
            self.plot_vmaf()

        self.concatenate_video()

    def main_thread(self):
        """Main."""
        # Start time
        tm = time.time()

        # Parse initial arguments
        self.arg_parsing()
        self.read_config()
        self.check_executables()
        self.process_inputs()

        # Changing pixel format, bit format
        self.d['pix_format'] = f'-strict -1 -pix_fmt {self.d.get("pix_format")}'
        self.d['ffmpeg_pipe'] = f' {self.d.get("ffmpeg")} {self.d.get("pix_format")} -f yuv4mpegpipe - |'

        # Video Mode. Encoding on local machine
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


def main():
    # Windows fix for multiprocessing
    multiprocessing.freeze_support()

    # Main thread
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


if __name__ == '__main__':
    main()
