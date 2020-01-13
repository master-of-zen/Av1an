#!/usr/bin/env python
"""
mkvmerge required (python-pymkv)
ffmpeg required
TODO:
DONE make encoding queue with limiting by workers
DONE make concatenating videos after encoding
DONE make passing your arguments for encoding,
2-pass encode by default for better quality
make separate audio and encode it separately,
"""
import os
import shutil
from os.path import join
from psutil import virtual_memory
from subprocess import Popen, call
import argparse
import time
from shutil import rmtree
from math import ceil
from multiprocessing import Pool
try:
    import scenedetect
except ImportError:
    print('ERROR: No PyScenedetect installed, try: sudo pip install scenedetect')


FFMPEG = 'ffmpeg -hide_banner -loglevel error '


class ProgressBar:
    """
    Progress Bar for tracking encoding progress
    """

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
        self.workers = 0
        self.encoder = 'aomenc'
        self.args = None
        self.audio = None
        self.threshold = 20
        self.logging = '&> /dev/null'

    def arg_parsing(self):
        """
        Command line parser
        Have default params
        """
        default_encode_aomenc = '--cpu-used=6 --end-usage=q --cq-level=40'
        default_audio = '-c:a copy'

        parser = argparse.ArgumentParser()
        parser.add_argument('--encoding_params', '-e', type=str, default=default_encode_aomenc,
                            help='encoding settings')
        parser.add_argument('--file_path', '-i', type=str, default='bruh.mp4', help='Input File', required=True)
        parser.add_argument('--encoder', '-enc', type=str, default='aomenc', help='Choosing encoder')
        parser.add_argument('--workers', '-t', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default=default_audio, help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=int, default=self.threshold, help='PySceneDetect Threshold')
        parser.add_argument('--logging', '-log', type=str, default=self.logging, help='Enable logging. ')

        self.args = parser.parse_args()

        if self.logging != self.args.logging:
            self.logging = f'&>> {self.args.logging}.log'
            os.system(f'echo " Av1an Logging "> {self.args.logging}.log')

    def determine_resources(self):
        """
        Returns number of workers that machine can handle with selected encoder
        :return: int
        """
        self.encoder = self.args.encoder.strip()
        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.args.workers != 0:
            self.workers = self.args.workers

        elif self.encoder == 'aomenc':
            self.workers = ceil(min(cpu, ram/1.5))

        elif self.encoder == 'rav1e':
            self.workers = ceil(min(cpu, ram/1.2)) // 3
        else:
            print('Error: no valid encoder')
            exit()

    def setup(self, input_file):

        if not os.path.exists(input_file):
            print("File don't exist")
            exit()

        # Make temporal directories, and remove them if already presented
        if os.path.isdir(join(os.getcwd(), ".temp")):
            rmtree(join(self.here, ".temp"))

        os.makedirs(join(self.here, '.temp', 'split'))
        os.makedirs(join(self.here, '.temp', 'encode'))

    def extract_audio(self, input_vid, audio_params):
        """
        Extracting audio from video file
        Encoding audio if needed
        """
        ffprobe = 'ffprobe -hide_banner -loglevel error -show_streams -select_streams a'
        check = fr'{ffprobe} -i {join(self.here,input_vid)} &> {join(self.here,".temp","audio_check.txt")}'

        os.system(check)

        cmd = f'{FFMPEG} -i {join(self.here,input_vid)} -vn {audio_params} {join(os.getcwd(),".temp","audio.mkv")}'
        Popen(cmd, shell=True).wait()

    def split_video(self, input_vid):
        """
        PySceneDetect used split video by scenes and pass it to encoder
        Optimal threshold settings 15-50
        """
        cmd2 = f'scenedetect -i {input_vid}  --output .temp/split detect-content --threshold {self.threshold} list-scenes  split-video -c {self.logging}'
        os.system(cmd2)
        print(f'\rVideo {input_vid} splitted')

    def get_video_queue(self, source_path):
        """
        Returns sorted list of all videos that need to be encoded. Big first
        """
        videos = []
        for root, dirs, files in os.walk(source_path):
            for file in files:
                f = os.path.getsize(os.path.join(root, file))
                videos.append([file, f])

        videos = sorted(videos, key=lambda x: -x[1])
        print(f'Splited videos: {len(videos)}')
        return videos

    def encode(self, commands):
        """
        Passing encoding params to ffmpeg for encoding
        TODO:
        Replace ffmpeg with aomenc because ffmpeg libaom doen't work with parameters properly
        """
        for i in commands[:-1]:
            cmd = rf'{FFMPEG} -an {i}  {self.logging}'
            os.system(cmd)

    def concatenate_video(self, input_video):
        """
        Using FFMPEG to concatenate all encoded videos to 1 file.
        Reading all files in A-Z order and saving it to concat.txt
        """
        with open(f'{join(self.here, ".temp", "concat.txt")}', 'w') as f:

            for root, firs, files in os.walk(join(self.here, '.temp', 'encode')):
                for file in sorted(files):
                    f.write(f"file '{join(root, file)}'\n")

        concat = join(self.here, ".temp", "concat.txt")

        audio = f'-i {join(self.here, ".temp", "audio.mkv")}'
        output = f'{input_video.split(".")[0]}_av1.mkv'

        cmd = f'{FFMPEG} -f concat -safe 0 -i {concat} {audio} -c copy -y {output}'
        Popen(cmd, shell=True).wait()

    def compose_encoding_queue(self, encoding_params, files, encoder):
        """
        Composing encoding commands
        Examples:
        1_pass Aomenc:
        ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        aomenc -q   --passes=1 --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 -o output_file

        2_pass Aomenc:
        ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        aomenc -q --passes=2 --pass=1  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o /dev/null -

        ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
        aomenc -q --passes=2 --pass=2  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o output_file -

        rav1e:
        ffmpeg -i bruh.mp4 -pix_fmt yuv420p -f yuv4mpegpipe - |
         rav1e - --speed=5 --tile-rows 2 --tile-cols 2 --output  output.ivf
        """
        file_paths = [(f'{join(os.getcwd(), ".temp", "split", file_name)}',
                       f'{join(os.getcwd(), ".temp", "encode", file_name)}',
                       file_name) for file_name in files]

        ffmpeg_pipe = '-pix_fmt yuv420p -f yuv4mpegpipe - |'
        if encoder == 'aomenc':
            single_pass = 'aomenc -q --passes=1 '
            two_pass_1_aom = '--passes=2 --pass=1'
            two_pass_2_aom = '--passes=2 --pass=2'

            pass_1_commands = [
                (f'-i {file[0]} {ffmpeg_pipe}' +
                 f'  {single_pass} {encoding_params} -o {file[1]} -',  file[2])
                for file in file_paths]

            pass_2_commands = [
                (f'-i {file[0]} {ffmpeg_pipe}' +
                 f' aomenc -q {two_pass_1_aom} {encoding_params} --fpf={file[0]}.log -o /dev/null -',
                 f'-i {file[0]} {ffmpeg_pipe}' +
                 f' aomenc -q {two_pass_2_aom} {encoding_params} --fpf={file[0]}.log -o {file[1]} -'
                 , file[2])
                for file in file_paths]

            return pass_2_commands

        if encoder == 'rav1e':
            pass_1_commands = [(f'-i {file[0]} {ffmpeg_pipe}' +
                                f' rav1e -  {encoding_params}  --output {file[1]}.ivf', f'{file[2]}.ivf')
                               for file in file_paths]
            return pass_1_commands

    def main(self):

        # Parse initial arguments
        self.arg_parsing()

        # Check validity of request and create temp folders/files
        self.setup(self.args.file_path)

        # Extracting audio
        self.extract_audio(self.args.file_path, self.args.audio_params)

        # Splitting video and sorting big-first
        self.split_video(self.args.file_path)
        vid_queue = self.get_video_queue('.temp/split')
        files = [i[0] for i in vid_queue[:-1]]

        # Determine resources
        self.determine_resources()

        # Make encode queue
        commands = self.compose_encoding_queue(self.args.encoding_params, files, self.args.encoder)

        # Creating threading pool to encode bunch of files at the same time
        print(f'Starting encoding with {self.workers} workers. \nParameters:{self.args.encoding_params}\nEncoding..')

        # Progress bar
        bar = ProgressBar(len(vid_queue))
        pool = Pool(self.workers)
        for i, _ in enumerate(pool.imap_unordered(self.encode, commands), 1):
            bar.tick()

        bar.tick()

        self.concatenate_video(self.args.file_path)


if __name__ == '__main__':

    # Main thread

    start = time.time()

    av1an = Av1an()
    av1an.main()

    print(f'\n Completed in {round(time.time()-start, 1)} seconds')

    # Delete temp folders
    rmtree(join(os.getcwd(), ".temp"))

    # To prevent console from hanging
    os.popen('stty sane', 'r')
