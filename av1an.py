#!/usr/bin/env python3

import time
import socket
from tqdm import tqdm
import sys
import os
import shutil
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


if sys.version_info < (3, 7):
    print('Python 3.7+ required')
    sys.exit()


class Av1an:

    def __init__(self):
        """Av1an - Python all-in-one toolkit for AV1, VP9, VP8 encodes."""
        self.FFMPEG = 'ffmpeg -y -hide_banner -loglevel error'
        self.d = dict()

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

    def arg_parsing(self):
        """Command line parse and sanity checking."""
        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=0, help='Mode 0 - video, Mode 1 - image')
        parser.add_argument('--video_params', '-v', type=str, default='', help='encoding settings')
        parser.add_argument('--input_file', '-i', type=Path, help='Input File')
        parser.add_argument('--encoder', '-enc', type=str, default='aom', help='Choosing encoder')
        parser.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=float, default=30, help='PySceneDetect Threshold')
        parser.add_argument('--temp', type=Path, default=Path('.temp'), help='Set temp folder path')
        parser.add_argument('--logging', '-log', type=str, default=None, help='Enable logging')
        parser.add_argument('--passes', '-p', type=int, default=2, help='Specify encoding passes')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')
        parser.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
        parser.add_argument('--pix_format', '-fmt', type=str, default='yuv420p', help='FFmpeg pixel format')
        parser.add_argument('--scenes', '-s', type=str, default=None, help='File location for scenes')
        parser.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
        parser.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')
        parser.add_argument('--keep', help='Keep temporally folder after encode', action='store_true')
        parser.add_argument('--boost', help='Experimental feature', action='store_true')
        parser.add_argument('-br', default=15, type=int, help='Range/strenght of CQ change')
        parser.add_argument('-bl', default=10, type=int, help='CQ limit for boosting')
        parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        parser.add_argument('--vmaf_path', type=str, default=None, help='Path to vmaf models')
        parser.add_argument('--tg_vmaf', type=float, help='Value of Vmaf to target')
        parser.add_argument('--vmaf_error', type=float, default=0.0, help='Error to compensate to wrong target vmaf')
        parser.add_argument('--vmaf_steps', type=int, default=4, help='Steps between min and max qp for target vmaf')
        parser.add_argument('--min_cq', type=int, default=20, help='Min cq for target vmaf')
        parser.add_argument('--max_cq', type=int, default=63, help='Max cq for target vmaf')
        parser.add_argument('--host', type=str, help='ip of host')

        # Store all vars in dictionary
        self.d = vars(parser.parse_args())

        if not find_executable('ffmpeg'):
            print('No ffmpeg')
            sys.exit()

        # Check if encoder executable is reachable
        if self.d.get('encoder') in ('svt_av1', 'rav1e', 'aom', 'vpx'):
            if self.d.get('encoder') == 'rav1e':
                enc = 'rav1e'
            elif self.d.get('encoder') == 'aom':
                enc = 'aomenc'
            elif self.d.get('encoder') == 'svt_av1':
                enc = 'SvtAv1EncApp'
            elif self.d.get('encoder') == 'vpx':
                enc = 'vpxenc'

            if not find_executable(enc):
                print(f'Encoder {enc} not found')
                sys.exit()
        else:
            print(f'Not valid encoder {self.d.get("encoder")} ')
            sys.exit()

        # Check input file
        if self.d.get('mode') == 2 and self.d.get('input_file'):
            print("Server mode, input file ignored")
        elif self.d.get('mode') == 2:
            pass
        elif self.d.get('mode') != 2 and not self.d.get('input_file'):
            print('No input file')
            sys.exit()
        elif not self.d.get('input_file').exists():
            print(f'No file: {self.d.get("input_file")}')
            sys.exit()

        # Set output file
        if self.d.get('mode') != 2:
            if self.d.get('output_file'):
                self.d['output_file'] = self.d.get('output_file').with_suffix('.mkv')
            else:
                self.d['output_file'] = Path(f'{self.d.get("input_file").stem}_av1.mkv')

        # Changing pixel format, bit format
        self.d['pix_format'] = f' -strict -1 -pix_fmt {self.d.get("pix_format")}'

        self.d['ffmpeg_pipe'] = f' {self.d.get("ffmpeg")} {self.d.get("pix_format")} -f yuv4mpegpipe - |'

        if self.d.get('vmaf_steps') < 4:
            print('Target vmaf require more than 3 probes/steps')
            sys.exit()

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

    def setup(self, input_file: Path):
        """Creating temporally folders when needed."""
        # Make temporal directories, and remove them if already presented
        if self.d.get('temp').exists() and self.d.get('resume'):
            pass
        else:
            if self.d.get('temp').is_dir():
                shutil.rmtree(self.d.get('temp'))
            (self.d.get('temp') / 'split').mkdir(parents=True)
            (self.d.get('temp') / 'encode').mkdir()

        if self.d.get('logging') is os.devnull:
            self.d['logging'] = self.d.get('temp') / 'log.log'

    def extract_audio(self, input_vid: Path):
        """Extracting audio from source, transcoding if needed."""
        audio_file = self.d.get('temp') / 'audio.mkv'
        if audio_file.exists():
            self.log('Reusing Audio File\n')
            return

        # Capture output to check if audio is present

        check = fr'{self.FFMPEG} -ss 0 -i "{input_vid}" -t 0 -vn -c:a copy -f null -'
        is_audio_here = len(self.call_cmd(check, capture_output=True)) == 0

        if is_audio_here:
            self.log(f'Audio processing\n'
                     f'Params: {self.d.get("audio_params")}\n')
            cmd = f'{self.FFMPEG} -i "{input_vid}" -vn ' \
                  f'{self.d.get("audio_params")} {audio_file}'
            self.call_cmd(cmd)

    def get_vmaf(self, source: Path, encoded: Path):
        if self.d.get("vmaf_path"):
            model = f'=model_path={self.d.get("vmaf_path")}'
        else:
            model = ''

        cmd = f'{self.FFMPEG} -i {source.as_posix()} -i {encoded.as_posix()}  ' \
              f'-filter_complex "[0:v][1:v]libvmaf{model}" ' \
              f'-max_muxing_queue_size 1024 -f null - '

        call = self.call_cmd(cmd, capture_output=True)
        result = call.decode().strip().split()
        if 'monotonically' in result:
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
            scene_manager.detect_scenes(frame_source=video_manager, show_progress=True)

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

    def split(self, video, frames):
        """Spliting video by frame numbers, or just copying video."""
        if len(frames) == 0:
            self.log('Copying video for encode\n')
            cmd = f'{self.FFMPEG} -i "{video}" -map_metadata -1 -an -c copy ' \
                  f'-avoid_negative_ts 1 {self.d.get("temp") / "split" / "0.mkv"}'
        else:
            self.log('Splitting video\n')
            cmd = f'{self.FFMPEG} -i "{video}" -map_metadata -1 -an -f segment -segment_frames {frames} ' \
                  f'-c copy -avoid_negative_ts 1 {self.d.get("temp") / "split" / "%04d.mkv"}'

        self.call_cmd(cmd)

    def frame_probe(self, source: Path):
        """Get frame count."""
        cmd = f'ffmpeg -hide_banner  -i "{source.absolute()}" -an  -map 0:v:0 -c:v copy -f null - '
        frames = (self.call_cmd(cmd, capture_output=True)).decode("utf-8")
        frames = int(frames[frames.rfind('frame=') + 6:frames.rfind('fps=')])
        return frames

    def frame_check(self, source: Path, encoded: Path):
        """Checking is source and encoded video frame count match."""
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

    def get_video_queue(self, source_path: Path):
        """Returns sorted list of all videos that need to be encoded. Big first."""
        queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']

        if self.d.get('resume'):
            done_file = self.d.get('temp') / 'done.txt'
            if done_file.exists():
                with open(done_file, 'r') as f:
                    data = [line for line in f]
                    if len(data) > 1:
                        data = literal_eval(data[1])
                        queue = [x for x in queue if x.name not in [x[1] for x in data]]

        queue = sorted(queue, key=lambda x: -x.stat().st_size)

        if len(queue) == 0:
            print('Error: No files found in .temp/split, probably splitting not working')
            sys.exit()

        return queue

    def svt_av1_encode(self, input_files):
        """SVT-AV1 encoding command composition."""
        if not self.d.get("video_params"):
            print('-w -h -fps is required parameters for svt_av1 encoder')
            sys.exit()

        encoder = 'SvtAv1EncApp '
        if self.d.get('passes') == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} ' +
                 f'  {encoder} -i stdin {self.d.get("video_params")} -b {file[1].with_suffix(".ivf")} -',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_1_commands

        if self.d.get('passes') == 2:
            p2i = '-input-stat-file '
            p2o = '-output-stat-file '
            pass_2_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} ' +
                 f'  {encoder} -i stdin {self.d.get("video_params")} {p2o} '
                 f'{file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {self.d.get("ffmpeg_pipe")} '
                 +
                 f'{encoder} -i stdin {self.d.get("video_params")} {p2i} '
                 f'{file[0].with_suffix(".stat")} -b {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_2_commands

    def aom_encode(self, input_files):
        """AOM encoding command composition."""
        if not self.d.get("video_params"):
            self.d["video_params"] = '--threads=4 --cpu-used=6 --end-usage=q --cq-level=40'

        single_p = 'aomenc  -q --passes=1 '
        two_p_1_aom = 'aomenc -q --passes=2 --pass=1'
        two_p_2_aom = 'aomenc  -q --passes=2 --pass=2'

        if self.d.get('passes') == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} ' +
                 f'  {single_p} {self.d.get("video_params")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_1_commands

        if self.d.get('passes') == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")}' +
                 f' {two_p_1_aom} {self.d.get("video_params")} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {self.d.get("ffmpeg_pipe")}' +
                 f' {two_p_2_aom} {self.d.get("video_params")} '
                 f'--fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_2_commands

    def rav1e_encode(self, input_files):
        """Rav1e encoding command composition."""
        if not self.d.get("video_params"):
            self.d["video_params"] = ' --tiles=4 --speed=10'

        if self.d.get('passes') == 1 or self.d.get('passes') == 2:
            pass_1_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} '
                 f' rav1e -  {self.d.get("video_params")}  '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_1_commands

        # 2 encode pass not working with FFmpeg pipes :(
        if self.d.get('passes') == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} '
                 f' rav1e - --first-pass {file[0].with_suffix(".stat")} {self.d.get("video_params")} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 f'-i {file[0]} {self.d.get("ffmpeg_pipe")} '
                 f' rav1e - --second-pass {file[0].with_suffix(".stat")} {self.d.get("video_params")} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_2_commands

    def vpx_encode(self, input_files):
        """VPX encoding command composition."""
        if not self.d.get("video_params"):
            self.d["video_params"] = '--codec=vp9 --threads=4 --cpu-used=1 --end-usage=q --cq-level=40'

        single_p = 'vpxenc  -q --passes=1 '
        two_p_1_vpx = 'vpxenc -q --passes=2 --pass=1'
        two_p_2_vpx = 'vpxenc  -q --passes=2 --pass=2'

        if self.d.get('passes') == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")} ' +
                 f'  {single_p} {self.d.get("video_params")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_1_commands

        if self.d.get('passes') == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.d.get("ffmpeg_pipe")}' +
                 f' {two_p_1_vpx} {self.d.get("video_params")} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {self.d.get("ffmpeg_pipe")}' +
                 f' {two_p_2_vpx} {self.d.get("video_params")} '
                 f'--fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in input_files]
            return pass_2_commands

    def compose_encoding_queue(self, files):
        """Composing encoding queue with splited videos."""
        input_files = [(self.d.get('temp') / "split" / file.name,
                       self.d.get('temp') / "encode" / file.name,
                       file) for file in files]

        if self.d.get('encoder') == 'aom':
            queue = self.aom_encode(input_files)

        elif self.d.get('encoder') == 'rav1e':
            queue = self.rav1e_encode(input_files)

        elif self.d.get('encoder') == 'svt_av1':
            queue = self.svt_av1_encode(input_files)

        elif self.d.get('encoder') == 'vpx':
            queue = self.vpx_encode(input_files)

        self.log(f'Encoding Queue Composed\n'
                 f'Encoder: {self.d.get("encoder").upper()} Queue Size: {len(queue)} Passes: {self.d.get("passes")}\n'
                 f'Params: {self.d.get("video_params")}\n')

        return queue

    def get_brightness(self, video):
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
        brig_geom = round(statistics.geometric_mean([x+1 for x in brightness]), 1)

        return brig_geom

    def man_cq(self, command: str, cq: int):
        """
        If cq == -1 returns current value of cq in command
        Else return command with new cq value
        """
        mt = '--cq-level='
        if cq == -1:
            mt = '--cq-level='
            cq = int(command[command.find(mt) + 11:command.find(mt) + 13])
            return cq
        else:
            cmd = command[:command.find(mt) + 11] + str(cq) + command[command.find(mt) + 13:]
            return cmd

    def boost(self, command: str, br_geom, new_cq=0):
        """Based on average brightness of video decrease(boost) Quantize value for encoding."""
        cq = self.man_cq(command, -1)
        if not new_cq:
            if br_geom < 128:
                new_cq = cq - round((128 - br_geom) / 128 * self.d.get('br'))

                # Cap on boosting
                if new_cq < self.d.get('bl'):
                    new_cq = self.d.get('bl')
            else:
                new_cq = cq
        cmd = self.man_cq(command, new_cq)

        return cmd, new_cq

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
            vm = self.d.get('tg_vmaf')
            if vm:
                plt.hlines(vm, 0, len(x1), colors='red')

            # Save/close
            plt.savefig(self.d.get('input_file').stem)
            plt.close()
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'\nError in vmaf plot: {e}\nAt line: {exc_tb.tb_lineno}\n')

    def target_vmaf(self, source, command):
        tg = self.d.get('tg_vmaf')
        mincq = self.d.get('min_cq')
        maxcq = self.d.get('max_cq')
        steps = self.d.get('vmaf_steps')

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
        cmd = [[f'{self.FFMPEG} -i {probe} {self.d.get("ffmpeg_pipe")} {single_p} '
                f'--threads=4 --end-usage=q --cpu-used=6 --cq-level={x} '
                f'-o {probe.with_name(f"v_{x}{probe.stem}")}.ivf - ',
                probe, probe.with_name(f'v_{x}{probe.stem}').with_suffix('.ivf'), x] for x in q]

        # Encoding probe and getting vmaf
        ls = []
        for i in cmd:
            self.call_cmd(i[0])
            v = self.get_vmaf(i[1], i[2])
            if isinstance(v,str):
                return int(cq), 'Error in vmaf calculation\n'

            ls.append((v, i[3]))
        x = [x[1] for x in ls]
        y = [(float(x[0]) - self.d.get('vmaf_error')) for x in ls]

        # Interpolate data
        f = interpolate.interp1d(x, y, kind='cubic')

        xnew = np.linspace(min(x), max(x), max(x)-min(x))

        # Getting value closest to target
        tl = list(zip(xnew, f(xnew)))
        tg_cq = min(tl, key=lambda x: abs(x[1] - tg))

        # Saving plot of got data
        plt.plot(xnew, f(xnew))
        plt.plot(tg_cq[0], tg_cq[1], 'o')

        for i in range(int(min(x[1] for x in tl)), 100, 1):
            plt.axhline(i, color='grey', linewidth=0.5)

        for i in range(int(min(xnew)), int(max(xnew)) + 1, 1):
            plt.axvline(i, color='grey', linewidth=0.5)

        plt.savefig(probe.stem)
        plt.close()

        return int(tg_cq[0]), f'Target: CQ {int(tg_cq[0])} Vmaf: {round(float(tg_cq[1]), 2)}\n'

    def encode(self, commands):
        """Single encoder command queue and logging output."""
        # Passing encoding params to ffmpeg for encoding.
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly.
        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = self.frame_probe(source)

            if self.d.get('tg_vmaf'):

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
                cmd = rf'{self.FFMPEG} {i}'
                self.call_cmd(cmd)

            self.frame_check(source, target)

            frame_probe = self.frame_probe(target)

            enc_time = round(time.time() - st_time, 2)

            if self.d.get('vmaf'):
                v = self.get_vmaf(source, target)
                if isinstance(v,str):
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
            print(f'Concatenation failed, FFmpeg error')
            self.log(f'Concatenation failed, aborting, error: {e}\n')
            sys.exit()

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        with Pool(self.d.get('workers')) as pool:

            # Reduce if more workers than clips
            self.d['workers'] = min(len(commands), self.d.get('workers'))

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
                        total = self.frame_probe(self.d.get('input_file'))
                self.log(f'Resumed with {done} encoded clips done\n\n')

            else:
                initial = 0
                with open(Path(self.d.get('temp') / 'done.txt'), 'w') as f:
                    total = self.frame_probe(self.d.get('input_file'))
                    f.write(f'{total}\n')

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            print(f'\rQueue: {clips} Workers: {self.d.get("workers")} Passes: {self.d.get("passes")}\n'
                  f'Params: {self.d.get("video_params")}')

            bar = tqdm(total=total, initial=initial, dynamic_ncols=True, unit="fr",
                       leave=False)

            loop = pool.imap_unordered(self.encode, commands)
            self.log(f'Started encoding queue with {self.d.get("workers")} workers\n\n')

            try:
                for enc_frames in loop:
                    bar.update(n=enc_frames)
            except Exception as e:
                _, _, exc_tb = sys.exc_info()
                print(f'Encoding error: {e}\nAt line {exc_tb.tb_lineno}')
                sys.exit()

    def setup_routine(self):
        """
        All pre encoding routine.
        Scene detection, splitting, audio extraction
        """
        if not (self.d.get('resume') and self.d.get('temp').exists()):
            # Check validity of request and create temp folders/files
            self.setup(self.d.get('input_file'))

            self.set_logging()

            # Splitting video and sorting big-first
            framenums = self.scene_detect(self.d.get('input_file'))
            self.split(self.d.get('input_file'), framenums)

            # Extracting audio
            self.extract_audio(self.d.get('input_file'))
        else:
            self.set_logging()

    def video_encoding(self):
        """Encoding video on local machine."""
        self.setup_routine()

        files = self.get_video_queue(self.d.get('temp') / 'split')

        # Make encode queue
        commands = self.compose_encoding_queue(files)

        # Catch Error
        if len(commands) == 0:
            print('Error in making command queue')
            sys.exit()

        # Determine resources if workers don't set
        if self.d.get('workers') != 0:
            self.d['workers'] = self.d.get('workers')
        else:
            self.determine_resources()

        self.encoding_loop(commands)

        if self.d.get('vmaf'):
            self.plot_vmaf()

        self.concatenate_video()

    def receive_file(self, sock:socket, file:Path):
        try:
            sc, _ = sock.accept()
            with open(file.name, 'wb') as f:
                while True:
                    data = sc.recv(1024)
                    while data:
                        f.write(data)
                        data = sc.recv(1024)
        except Exception as i:
            _, _, exc_tb = sys.exc_info()
            print(f'Error: Receiving file failed\n{i} at line: {exc_tb.tb_lineno}')

    def send_file(self, sock:socket, file:Path):
        try:
            self.log(f"Sending {file.name}\n")
            with open(file.absolute().as_posix(), 'rb') as f:
                data = f.read(1024)
                while data:
                    sock.send(data)
                    data = f.read(1024)
            return True

        except Exception as e:
            print(f"Error: Failed to send file\n{e}")

    def master_mode(self):
        """Master mode. Splitting, managing queue, sending chunks, receiving chunks, concat videos."""
        print('Working in master mode')

        # Setup
        self.setup_routine()

        # files = self.get_video_queue(self.d.get('temp') / 'split')

        host = '127.0.0.1'                  # Symbolic name meaning all available interfaces
        port = 40995          # Arbitrary non-privileged port

        # data to send
        args_dict = str(self.d).encode()


        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.connect((host, port))
                print('Connected to: ', host, port)
                s.sendall(args_dict)
                print('Encoding data send')
        except ConnectionRefusedError:
            print(f'Connection refused: {host}:{port}')
        # Creating Queue
        # Sending Chunks to server

    def server(self):
        """Encoder mode: Connecting, Receiving, Encoding."""
        print('Working in server mode')

        host = '127.0.0.1'                 # Symbolic name meaning all available interfaces
        port = self.d.get('host')      # Arbitrary non-privileged port

        print(f'Bind at {host} {port}')

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:

            s.bind((host, port))
            while True:
                s.listen()
                print('Stand by..')
                conn, addr = s.accept()
                print('Accepted connection: ', addr)
                with conn:
                    temp = b''
                    while True:
                        data = conn.recv(1024)
                        if not data:
                            break
                        temp += data
                    print('Received Settings from master:\n', temp)
                    conn.sendall(temp)
        # while chunks
        # receive chunks
        # encode
        # send back

    def main_thread(self):
        """Main."""

        # Start time
        tm = time.time()

        # Parse initial arguments
        self.arg_parsing()

        # Video Mode. Encoding on local machine
        if self.d.get('mode') == 0:
            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s')
        # Master mode
        elif self.d.get('mode') == 1:
            self.master_mode()

        # Encoder mode. Accepting files over network and encode them
        elif self.d.get('mode') == 2:
            self.server()

        else:
            print('No valid work mode')
            exit()

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
