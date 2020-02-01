#!/usr/bin/env python

import sys
import os
import shutil
import csv
from os.path import join
from psutil import virtual_memory
import argparse
import time
from shutil import rmtree
from math import ceil
from multiprocessing import Pool
# Fix frame cuts


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
        self.here = os.getcwd()
        self.FFMPEG = 'ffmpeg -hide_banner -loglevel error'
        self.pix_format = 'yuv420p'
        self.encoder = 'aom'
        self.encode_pass = 2
        self.threshold = 30
        self.workers = 0
        self.mode = 0
        self.ffmpeg_pipe = None
        self.force_fps = None
        self.logging = None
        self.args = None
        self.encoding_params = ''
        self.video_filter = ''
        self.output_file = ''
        self.scenes = ''
        self.audio = ''
        # OS specific NULL pointer

        if sys.platform == 'linux':
            self.null = '&> /dev/null'
            self.point = '&>'
        else:
            self.point = '>'
            self.null = '> nul 2>&1'

    def call_cmd(self, cmd):
        cmd = rf'{cmd} {self.logging}'
        os.system(cmd)

    def arg_parsing(self):

        # Command line parse and assigning defined and user defined params

        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=self.mode, help='Mode 0 - video, Mode 1 - image')
        parser.add_argument('--encoding_params', '-e', type=str, default=self.encoding_params, help='encoding settings')
        parser.add_argument('--file_path', '-i', type=str, default='bruh.mp4', help='Input File', required=True)
        parser.add_argument('--encoder', '-enc', type=str, default=self.encoder, help='Choosing encoder')
        parser.add_argument('--workers', '-t', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default=self.audio, help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=int, default=self.threshold, help='PySceneDetect Threshold')
        parser.add_argument('--logging', '-log', type=str, default=self.logging, help='Enable logging')
        parser.add_argument('--encode_pass', '-p', type=int, default=self.encode_pass, help='Specify encoding passes')
        parser.add_argument('--output_file', '-o', type=str, default='', help='Specify output file')
        parser.add_argument('--force_fps', '-fps', type=int, default=0, help='Force fps of output file')
        parser.add_argument('--video_filter', '-vf', type=str, default=self.video_filter, help='FFmpeg video options')
        parser.add_argument('--pix_format', '-fmt', type=str, default=self.pix_format, help='FFmpeg pixel format')
        parser.add_argument('--scenes', '-s', type=str, default=self.scenes, help='File location for scenes')

        # Test
        try:
            import scenedetect
        except ImportError:
            print('PySceneDetect not found. Please check installation')
            exit()

        # Pass command line args that were passed
        self.args = parser.parse_args()

        # Set scenes if provided
        self.scenes = self.args.scenes

        # Set encoder if provided
        self.encoder = self.args.encoder.strip()
        if self.encoder not in ('svt_av1', 'rav1e', 'aom'):
            print(f'Not valid encoder {self.encoder}')
            exit()

        # Set mode (Video/Picture)
        self.mode = self.args.mode

        # Number of encoder passes
        self.encode_pass = self.args.encode_pass

        # Adding filter
        if self.args.video_filter != self.video_filter:
            self.video_filter = f' -vf {self.args.video_filter} '

        # Forcing FPS option
        if self.args.force_fps == 0:
            self.force_fps = ''
        else:
            self.force_fps = f' -r {self.args.force_fps}'

        # Changing pixel format, bit format
        if self.args.pix_format != self.pix_format:
            self.pix_format = f' -strict -1 -pix_fmt {self.args.pix_format}'
        else:
            self.pix_format = f'-pix_fmt {self.args.pix_format}'

        self.ffmpeg_pipe = f' {self.video_filter} {self.force_fps} {self.pix_format} -f yuv4mpegpipe - |'

        # Setting logging depending on OS
        if self.logging != self.args.logging:
            if sys.platform == 'linux':
                self.logging = f'&>> {self.args.logging}.log'
        else:
            self.logging = self.null

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

    def setup(self, input_file):
        if not os.path.exists(input_file):
            print("File don't exist")
            exit()

        # Make temporal directories, and remove them if already presented
        if os.path.isdir(join(os.getcwd(), ".temp")):
            rmtree(join(self.here, ".temp"))

        os.makedirs(join(self.here, '.temp', 'split'))
        os.makedirs(join(self.here, '.temp', 'encode'))

    def extract_audio(self, input_vid):

        # Extracting audio from video file
        # Encoding audio if needed
        try:
            default_audio_params = '-c:a copy'
            ffprobe = 'ffprobe -hide_banner -loglevel error -show_streams -select_streams a'

            # Generate file with audio check
            check = fr'{ffprobe} -i {join(self.here,input_vid)} {self.point} {join(self.here,".temp","audio_check.txt")}'
            os.system(check)

            is_audio_here = os.path.getsize(join(self.here, ".temp", "audio_check.txt"))

            if is_audio_here > 0 and self.args.audio_params == '':
                cmd = f'{self.FFMPEG} -i {join(self.here, input_vid)} ' \
                      f'-vn {default_audio_params} {join(os.getcwd(), ".temp", "audio.mkv")}'
                self.call_cmd(cmd)
                self.audio = f'-i {join(self.here, ".temp", "audio.mkv")} {default_audio_params}'

            elif is_audio_here > 0 and len(self.args.audio_params) > 1:
                cmd = f'{self.FFMPEG} -i {join(self.here, input_vid)} -vn ' \
                      f'{self.args.audio_params} {join(os.getcwd(), ".temp", "audio.mkv")}'
                self.call_cmd(cmd)
                self.audio = f'-i {join(self.here, ".temp", "audio.mkv")} {default_audio_params}'
            else:
                self.audio = ''
        except FileNotFoundError:
            print('Audio stream file not found. Error in audio extraction')
            exit()

    def scenedetect(self, video, output):
        # PySceneDetect used split video by scenes and pass it to encoder
        # Optimal threshold settings 15-50

        cmd2 = f'scenedetect -i {video}  --output {output} detect-content ' \
               f'--threshold {self.threshold} list-scenes '
        self.call_cmd(cmd2)

    def split(self, video, timecodes):
        cmd = f'{self.FFMPEG} -i {video} -map_metadata -1  -an -f segment -segment_times {timecodes} ' \
              f'-c copy -avoid_negative_ts 1  .temp/split/%04d.mkv'
        self.call_cmd(cmd)

    def read_csv(self, file_path):
        with open(file_path) as csv_file:
            stamps = next(csv.reader(csv_file))
            stamps = stamps[1:]
            stamps = ','.join(stamps)
            return stamps

    def split_video(self, input_vid):

        if self.scenes:

            file_path = join(self.here, self.scenes)
            if os.path.exists(file_path):
                stamps = self.read_csv(file_path)
                self.split(input_vid, stamps)
            else:
                print(f'\rSpliting video..', end='\r')
                self.scenedetect(input_vid, '.')
                video = '.'.join(input_vid.split('.')[:-1])
                file_path = f'{join(self.here, f"{video}-Scenes.csv")}'

                stamps = self.read_csv(file_path)
                self.split(input_vid, stamps)
        else:
            print(f'\rSpliting video..', end='\r')

            self.scenedetect(input_vid, '.temp')
            video = '.'.join(input_vid.split('.')[:-1])
            file_path = f'{join(self.here, ".temp", f"{video}-Scenes.csv")}'

            stamps = self.read_csv(file_path)
            self.split(input_vid, stamps)

    def get_video_queue(self, source_path):

        # Returns sorted list of all videos that need to be encoded. Big first

        videos = []
        for root, dirs, files in os.walk(source_path):
            for file in files:
                f = os.path.getsize(os.path.join(root, file))
                videos.append([file, f])

        videos = [i[0] for i in sorted(videos, key=lambda x: -x[1])]
        print(f'Clips: {len(videos)} ', end='')
        return videos

    def svt_av1_encode(self, file_paths):
        # Single pass:
        # ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        # SvtAv1EncApp -i - -w 1920 -h 1080 -fps 24 -rc 2 -tbr 10000 -enc-mode 5 -b output.ivf

        if self.args.encoding_params == '':
            print('-w -h -fps is required parameters for svt_av1 encoder')
            exit()
        else:
            self.encoding_params = self.args.encoding_params
        encoder = 'SvtAv1EncApp '
        if self.encode_pass == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} -b {file[1]} -', file[2])
                for file in file_paths]
            return pass_1_commands

        if self.encode_pass == 2:
            p2i = '-input-stat-file '
            p2o = ' -output-stat-file '
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} {p2o} {file[0]}.stat -b {file[0]}.bk - ',
                 f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.encoding_params} {p2i} {file[0]}.stat -b {file[1]} - ',
                 file[2])
                for file in file_paths]
            return pass_2_commands

    def aom_encode(self, file_paths):

        # 1_pass Aomenc:
        # ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        # aomenc -q   --passes=1 --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 -o output

        # 2_pass Aomenc:
        # ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        # aomenc -q --passes=2 --pass=1  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o /dev/null -

        # ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        # aomenc -q --passes=2 --pass=2  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o output -

        if self.args.encoding_params == '':
            self.encoding_params = '--cpu-used=6 --end-usage=q --cq-level=40'
        else:
            self.encoding_params = self.args.encoding_params

        single_pass = 'aomenc  --passes=1 '
        two_pass_1_aom = 'aomenc  --passes=2 --pass=1'
        two_pass_2_aom = 'aomenc  --passes=2 --pass=2'

        if self.encode_pass == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {single_pass} {self.encoding_params} -o {file[1]} - ', file[2])
                for file in file_paths]
            return pass_1_commands

        if self.encode_pass == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_1_aom} {self.encoding_params} --fpf={file[0]}.log -o /dev/null - ',
                 f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_2_aom} {self.encoding_params} --fpf={file[0]}.log -o {file[1]} - ',
                 file[2])
                for file in file_paths]
            return pass_2_commands

    def rav1e_encode(self, file_paths):

        # rav1e Single Pass:
        # ffmpeg - i input - pix_fmt yuv420p - f yuv4mpegpipe - |
        # rav1e - --speed= 5  -output output.ivf

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

        file_paths = [(f'{join(os.getcwd(), ".temp", "split", file_name)}',
                       f'{join(os.getcwd(), ".temp", "encode", file_name)}',
                       file_name) for file_name in files]

        if self.encoder == 'aom':
            return self.aom_encode(file_paths)

        elif self.encoder == 'rav1e':
            return self.rav1e_encode(file_paths)

        elif self.encoder == 'svt_av1':
            return self.svt_av1_encode(file_paths)

        else:
            print(self.encoder)
            print(f'No valid encoder : "{self.encoder}"')
            exit()

    def encode(self, commands):

        # Passing encoding params to ffmpeg for encoding
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly

        for i in commands[:-1]:
            cmd = rf'{self.FFMPEG} -an {i}'
            self.call_cmd(cmd)

    def concatenate_video(self, input_video):

        # Using FFMPEG to concatenate all encoded videos to 1 file.
        # Reading all files in A-Z order and saving it to concat.txt

        with open(f'{join(self.here, ".temp", "concat.txt")}', 'w') as f:

            for root, firs, files in os.walk(join(self.here, '.temp', 'encode')):
                for file in sorted(files):
                    f.write(f"file '{join(root, file)}'\n")

        concat = join(self.here, ".temp", "concat.txt")
        try:
            is_audio_here = os.path.getsize(join(self.here, ".temp", "audio_check.txt"))
            if is_audio_here:
                self.audio = f'-i {join(self.here, ".temp", "audio.mkv")} -c copy'
        except FileNotFoundError:
            print('Error: audio file for concatenation not found')

        if self.output_file == self.args.output_file:
            self.output_file = f'{input_video.split(".")[0]}_av1.mkv'
        else:
            self.output_file = f'{join(self.here, self.args.output_file)}.mkv'

        try:
            cmd = f'{self.FFMPEG} -f concat -safe 0 -i {concat} {self.audio} -y {self.output_file}'
            self.call_cmd(cmd)
        except:
            print('Concatenation failed')
            exit()

    def image(self, image_path):
        print('Encoding Image..', end='')

        image_pipe = rf'{self.FFMPEG} -i {image_path} -pix_fmt yuv420p10le -f yuv4mpegpipe -strict -1 - | '
        output = rf'{"".join(image_path.split(".")[:-1])}.ivf'
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
            exit()

    def main(self):

        # Parse initial arguments
        self.arg_parsing()
        # Video Mode
        if self.mode == 0:
            # Check validity of request and create temp folders/files
            self.setup(self.args.file_path)

            # Extracting audio
            self.extract_audio(self.args.file_path)

            # Splitting video and sorting big-first
            self.split_video(self.args.file_path)
            files = self.get_video_queue('.temp/split')

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

            self.concatenate_video(self.args.file_path)

            # Delete temp folders
            rmtree(join(self.here, ".temp"))

        if self.mode == 1:
            self.image(self.args.file_path)


if __name__ == '__main__':

    # Main thread
    start = time.time()

    av1an = Av1an()
    av1an.main()

    print(f'\nCompleted in {round(time.time()-start, 1)} seconds')

    # Prevent linux terminal from hanging
    if sys.platform == 'linux':
        os.popen('stty sane', 'r')
