#!/usr/bin/env python3

# Todo:
# Benchmarking
# Add conf file

import sys
import os
import shutil
from os.path import join
from psutil import virtual_memory
import argparse
import time
from math import ceil
from multiprocessing import Pool
import multiprocessing
import subprocess
from pathlib import Path
from typing import Optional

try:
    import scenedetect
except ImportError:
    print('PySceneDetect not found. Please check installation')
    sys.exit()


from scenedetect.video_manager import VideoManager
from scenedetect.scene_manager import SceneManager
from scenedetect.detectors import ContentDetector


if sys.version_info < (3, 7):
    print('Av1an requires at least Python 3.7 to run.')
    sys.exit()


class ProgressBar:

    # Progress Bar for tracking encoding progress

    def __init__(self, tasks):
        self.bar_iteration: int = 0
        self.tasks = tasks

        # Print empty bar on initialization
        self.print()

    def print(self):
        terminal_size, _ = shutil.get_terminal_size((80, 20))
        bar_length = terminal_size - (2 * len(str(self.tasks))) - 13

        if self.bar_iteration == 0:
            percent = 0
            fill_size = 0
        else:
            percent = round(100 * (self.bar_iteration / self.tasks), 1)
            fill_size = int(bar_length * self.bar_iteration // self.tasks)

        end = f'{percent}% {self.bar_iteration}/{self.tasks}'
        in_bar = ('█' * fill_size) + '-' * (bar_length - fill_size)

        print(f'\r|{in_bar}| {end} ', end='')

    def tick(self):
        self.bar_iteration += 1
        self.print()


class Av1an:

    def __init__(self):
        self.temp_dir = Path('.temp')
        self.FFMPEG = 'ffmpeg -hide_banner -loglevel error'
        self.pix_format = 'yuv420p'
        self.encoder = 'aom'
        self.encode_pass = 2
        self.threshold = 30
        self.workers = 0
        self.mode = 0
        self.ffmpeg_pipe = None
        self.ffmpeg_com = None
        self.logging = None
        self.args = None
        self.encoding_params = ''
        self.output_file: Optional[Path] = None
        self.pyscene = ''
        self.scenes: Optional[Path] = None
        self.audio = ''
        # OS specific NULL pointer

        if sys.platform == 'linux':
            self.point = '&>'
        else:
            self.point = '>'

    def call_cmd(self, cmd):
        with open(self.logging, 'a') as log:
            subprocess.call(cmd, shell=True, stdout=log, stderr=log)

    def arg_parsing(self):

        # Command line parse and assigning defined and user defined params

        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=self.mode, help='Mode 0 - video, Mode 1 - image')
        parser.add_argument('--encoding_params', '-e', type=str, default=self.encoding_params, help='encoding settings')
        parser.add_argument('--file_path', '-i', type=Path, help='Input File', required=True)
        parser.add_argument('--encoder', '-enc', type=str, default=self.encoder, help='Choosing encoder')
        parser.add_argument('--workers', '-t', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default=self.audio, help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=float, default=self.threshold, help='PySceneDetect Threshold')
        parser.add_argument('--logging', '-log', type=str, default=self.logging, help='Enable logging')
        parser.add_argument('--encode_pass', '-p', type=int, default=self.encode_pass, help='Specify encoding passes')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')
        parser.add_argument('--ffmpeg_com', '-ff', type=str, default='', help='FFmpeg commands')
        parser.add_argument('--pix_format', '-fmt', type=str, default=self.pix_format, help='FFmpeg pixel format')
        parser.add_argument('--scenes', '-s', type=Path, default=self.scenes, help='File location for scenes')

        # Pass command line args that were passed
        self.args = parser.parse_args()

        # Set scenes if provided
        self.scenes = self.args.scenes

        self.threshold = self.args.threshold

        # Set encoder if provided
        self.encoder = self.args.encoder.strip()
        if self.encoder not in ('svt_av1', 'rav1e', 'aom'):
            print(f'Not valid encoder {self.encoder}')
            sys.exit()

        # Set mode (Video/Picture)
        self.mode = self.args.mode

        # Number of encoder passes
        self.encode_pass = self.args.encode_pass

        # Set output file
        if self.args.output_file is None:
            self.output_file = Path(f'{self.args.file_path.stem}_av1.mkv')
        else:
            self.output_file = self.args.output_file.with_suffix('.mkv')

        # Forcing FPS option
        if self.args.ffmpeg_com == 0:
            self.ffmpeg_com = ''
        else:
            self.ffmpeg_com = self.args.ffmpeg_com

        # Changing pixel format, bit format
        if self.args.pix_format != self.pix_format:
            self.pix_format = f' -strict -1 -pix_fmt {self.args.pix_format}'
        else:
            self.pix_format = f'-pix_fmt {self.args.pix_format}'

        self.ffmpeg_pipe = f' {self.ffmpeg_com} {self.pix_format} -f yuv4mpegpipe - |'

        # Setting logging file
        if self.args.logging:
            self.logging = f"{self.args.logging}.log"
        else:
            self.logging = os.devnull

    def determine_resources(self):

        # Returns number of workers that machine can handle with selected encoder

        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.args.workers != 0:
            self.workers = self.args.workers

        elif self.encoder == 'aom' or 'rav1e':
            self.workers = ceil(min(cpu, ram/1.5))

        elif self.encoder == 'svt_av1':
            self.workers = ceil(min(cpu, ram)) // 5

        # fix if workers round up to 0
        if self.workers == 0:
            self.workers += 1

    def setup(self, input_file: Path):
        if not input_file.exists():
            print("File don't exist")
            sys.exit()

        # Make temporal directories, and remove them if already presented
        if self.temp_dir.is_dir():
            shutil.rmtree(self.temp_dir)

        (self.temp_dir / 'split').mkdir(parents=True)
        (self.temp_dir / 'encode').mkdir()

    def extract_audio(self, input_vid: Path):

        # Extracting audio from video file
        # Encoding audio if needed
        try:
            default_audio_params = '-c:a copy'
            ffprobe = 'ffprobe -hide_banner -loglevel error -show_streams -select_streams a'

            # Generate file with audio check
            check = fr'{ffprobe} -i {input_vid} {self.point} {self.temp_dir / "audio_check.txt"}'
            os.system(check)

            is_audio_here = (self.temp_dir / "audio_check.txt").stat().st_size

            if is_audio_here > 0 and self.args.audio_params == '':
                cmd = f'{self.FFMPEG} -i {input_vid} ' \
                      f'-vn {default_audio_params} {self.temp_dir / "audio.mkv"}'
                self.call_cmd(cmd)
                self.audio = f'-i {self.temp_dir / "audio.mkv"} {default_audio_params}'

            elif is_audio_here > 0 and len(self.args.audio_params) > 1:
                cmd = f'{self.FFMPEG} -i {input_vid} -vn ' \
                      f'{self.args.audio_params} {self.temp_dir / "audio.mkv"}'
                self.call_cmd(cmd)
                self.audio = f'-i {self.temp_dir / "audio.mkv"} {default_audio_params}'
            else:
                self.audio = ''
        except FileNotFoundError:
            print('Audio stream file not found. Error in audio extraction')
            sys.exit()

    def scenedetect(self, video: Path):
        # PySceneDetect used split video by scenes and pass it to encoder
        # Optimal threshold settings 15-50
        video_manager = VideoManager([str(video)])
        scene_manager = SceneManager()
        scene_manager.add_detector(ContentDetector(threshold=self.threshold))
        base_timecode = video_manager.get_base_timecode()

        try:
            # If stats file exists, load it.
            if self.scenes and self.scenes.exists():
                # Read stats from CSV file opened in read mode:
                with open(self.scenes(), 'r') as stats_file:
                    stats = stats_file.read()
                    return stats

            # Set video_manager duration to read frames from 00:00:00 to 00:00:20.
            video_manager.set_duration()

            # Set downscale factor to improve processing speed.
            video_manager.set_downscale_factor()

            # Start video_manager.
            video_manager.start()

            # Perform scene detection on video_manager.
            scene_manager.detect_scenes(frame_source=video_manager)

            # Obtain list of detected scenes.
            scene_list = scene_manager.get_scene_list(base_timecode)
            # Like FrameTimecodes, each scene in the scene_list can be sorted if the
            # list of scenes becomes unsorted.

            scenes = []
            for i, scene in enumerate(scene_list):
                scenes.append(scene[0].get_timecode())

            scenes = ','.join(scenes[1:])

            # We only write to the stats file if a save is required:
            if self.scenes:
                with open(self.scenes, 'w') as stats_file:
                    stats_file.write(scenes)
                    return scenes
            else:
                return scenes
        except:
            print('Error in PySceneDetect')
            sys.exit()

    def split(self, video, timecodes):

        # Splits video with provided timecodes
        # If video is single scene, just copy video

        if len(timecodes) == 0:
            cmd = f'{self.FFMPEG} -i {video} -map_metadata 0 -an -c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "0.mkv"}'
        else:
            cmd = f'{self.FFMPEG} -i {video} -map_metadata 0 -an -f segment -segment_times {timecodes} ' \
                  f'-c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "%04d.mkv"}'

        self.call_cmd(cmd)

    def get_video_queue(self, source_path: Path):

        # Returns sorted list of all videos that need to be encoded. Big first
        videos = [file for file in sorted(source_path.iterdir(), key=lambda f: -f.stat().st_size)]
        print(f'Clips: {len(videos)} ', end='')
        return videos

    def svt_av1_encode(self, file_paths):
        
        if self.args.encoding_params == '':
            print('-w -h -fps is required parameters for svt_av1 encoder')
            sys.exit()
        else:
            self.encoding_params = self.args.encoding_params
        encoder = 'SvtAv1EncApp '
        if self.encode_pass == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} -b {file[1]}.ivf -', file[2])
                for file in file_paths]
            return pass_1_commands

        if self.encode_pass == 2:
            p2i = '-input-stat-file '
            p2o = ' -output-stat-file '
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} {p2o} {file[0]}.stat -b {file[0]}.bk - ',
                 f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} {p2i} {file[0]}.stat -b {file[1]}.ivf - ',
                 file[2])
                for file in file_paths]
            return pass_2_commands

    def aom_encode(self, file_paths):

        if self.args.encoding_params == '':
            self.encoding_params = '--cpu-used=6 --end-usage=q --cq-level=40'
        else:
            self.encoding_params = self.args.encoding_params

        single_pass = 'aomenc  --verbose --passes=1 '
        two_pass_1_aom = 'aomenc  --verbose --passes=2 --pass=1'
        two_pass_2_aom = 'aomenc  --verbose --passes=2 --pass=2'

        if self.encode_pass == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {single_pass} {self.encoding_params} -o {file[1]}.ivf - ', file[2])
                for file in file_paths]
            return pass_1_commands

        if self.encode_pass == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_1_aom} {self.encoding_params} --fpf={file[0]}.log -o {os.devnull} - ',
                 f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_2_aom} {self.encoding_params} --fpf={file[0]}.log -o {file[1]}.ivf - ',
                 file[2])
                for file in file_paths]
            return pass_2_commands

    def rav1e_encode(self, file_paths):

        if self.args.encoding_params == '':
            self.encoding_params = '--speed=5'
        else:
            self.encoding_params = self.args.encoding_params
        if self.encode_pass == 1 or self.encode_pass == 2:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' 
                 f' rav1e -  {self.encoding_params}  '
                 f'--output {file[1]}.ivf', f'{file[2]}.ivf ')
                for file in file_paths]
            return pass_1_commands
        if self.encode_pass == 2:

            # 2 encode pass not working with FFmpeg pipes :(
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' 
                 f' rav1e - --first-pass {file[0]}.stat {self.encoding_params} '
                 f'--output {file[0]}.ivf',
                 f'-i {file[0]} {self.ffmpeg_pipe} ' 
                 f' rav1e - --second-pass {file[0]}.stat {self.encoding_params} '
                 f'--output {file[1]}.ivf',
                 f'{file[2]}.ivf')
                for file in file_paths]

            return pass_2_commands

    def compose_encoding_queue(self, files):
        file_paths = [(f'{self.temp_dir / "split" / file.name}',
                       f'{self.temp_dir / "encode" / file.name}',
                       str(file)) for file in files]

        if self.encoder == 'aom':
            return self.aom_encode(file_paths)

        elif self.encoder == 'rav1e':
            return self.rav1e_encode(file_paths)

        elif self.encoder == 'svt_av1':
            return self.svt_av1_encode(file_paths)

        else:
            print(self.encoder)
            print(f'No valid encoder : "{self.encoder}"')
            sys.exit()

    def encode(self, commands):

        # Passing encoding params to ffmpeg for encoding
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly

        for i in commands[:-1]:
            cmd = rf'{self.FFMPEG} {i}'
            self.call_cmd(cmd)

    def concatenate_video(self):

        # Using FFMPEG to concatenate all encoded videos to 1 file.
        # Reading all files in A-Z order and saving it to concat.txt

        concat = self.temp_dir / "concat.txt"
        with open(f'{concat}', 'w') as f:
            # Write all files that need to be concatenated
            # Their path must be relative to the directory where "concat.txt" is
            encode_files = sorted((self.temp_dir / 'encode').iterdir())
            f.writelines(f"file '{file.relative_to(self.temp_dir)}'\n" for file in encode_files)

        try:
            is_audio_here = (self.temp_dir / "audio_check.txt").stat().st_size
            if is_audio_here:
                self.audio = f'-i {self.temp_dir / "audio.mkv"} -c copy'
        except FileNotFoundError:
            print('Error: audio file for concatenation not found')

        try:
            cmd = f'{self.FFMPEG} -f concat -safe 0 -i {concat} {self.audio} -y {self.output_file}'
            self.call_cmd(cmd)
        except:
            print('Concatenation failed')
            sys.exit()

    def image(self, image_path: Path):
        print('Encoding Image..', end='')

        image_pipe = rf'{self.FFMPEG} -i {image_path} -pix_fmt yuv420p10le -f yuv4mpegpipe -strict -1 - | '
        output = image_path.with_suffix('.ivf')
        if self.encoder == 'aom':
            aom = ' aomenc --passes=1 --pass=1 --end-usage=q  -b 10 --input-bit-depth=10 '
            cmd = (rf' {image_pipe} ' +
                   rf'{aom} {self.encoding_params} -o {output} - ')
            self.call_cmd(cmd)

        elif self.encoder == 'rav1e':
            cmd = (rf' {image_pipe} ' +
                   rf' rav1e {self.encoding_params} - -o {output} ')
            self.call_cmd(cmd)
        else:
            print(f'Not valid encoder: {self.encoder}')
            sys.exit()

    def main(self):

        # Parse initial arguments
        self.arg_parsing()
        # Video Mode
        if self.mode == 0:
            # Check validity of request and create temp folders/files
            self.setup(self.args.file_path)

            # Splitting video and sorting big-first
            timestamps = self.scenedetect(self.args.file_path)
            self.split(self.args.file_path, timestamps)
            files = self.get_video_queue(self.temp_dir / 'split')

            # Extracting audio
            self.extract_audio(self.args.file_path)

            # Determine resources
            self.determine_resources()

            # Make encode queue
            commands = self.compose_encoding_queue(files)

            # Reduce number of workers if needed
            self.workers = min(len(commands), self.workers)

            # Creating threading pool to encode bunch of files at the same time
            print(f'Workers: {self.workers}\nParams: {self.encoding_params}')

            # Progress bar
            bar = ProgressBar(len(files))
            pool = Pool(self.workers)
            for i, _ in enumerate(pool.imap_unordered(self.encode, commands), 1):
                bar.tick()

            self.concatenate_video()

            # Delete temp folders
            shutil.rmtree(self.temp_dir)

        if self.mode == 1:
            self.image(self.args.file_path)


if __name__ == '__main__':

    # Windows fix for multiprocessing
    multiprocessing.freeze_support()

    # Main thread
    start = time.time()

    av1an = Av1an()
    av1an.main()

    print(f'\nCompleted in {round(time.time()-start, 1)} seconds')

    # Prevent linux terminal from hanging
    if sys.platform == 'linux':
        os.popen('stty sane', 'r')
