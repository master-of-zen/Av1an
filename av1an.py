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
        self.iteration: int = 0
        self.tasks = tasks

        # Print on empty bar on initialization
        self.print()

    def print(self):
        terminal_size, _ = shutil.get_terminal_size((80, 20))
        bar_length = terminal_size - (2 * len(str(self.tasks))) - 13

        if self.iteration == 0:
            percent = 0
            fill_size = 0
        else:
            percent = round(100 * (self.iteration / self.tasks), 1)
            fill_size = int(bar_length * self.iteration // self.tasks)

        end = f'{percent}% {self.iteration}/{self.tasks}'
        in_bar = ('█' * fill_size) + '-' * (bar_length - fill_size)

        print(f'\r|{in_bar}| {end} ', end='')

    def tick(self):
        self.iteration += 1
        self.print()


def arg_parsing():
    """
    Command line parser
    Have default params
    """
    DEFAULT_ENCODE = '--threads=2 --cpu-used=4 --end-usage=q --cq-level=35 --aq-mode=1'
    DEFAULT_AUDIO = '-c:a copy'

    parser = argparse.ArgumentParser()
    parser.add_argument('--encoding_params', '-e', type=str, default=DEFAULT_ENCODE, help='encoding settings')
    parser.add_argument('--file_path', '-i', type=str, default='bruh.mp4', help='Input File', required=True)
    parser.add_argument('--encoder', '-enc', type=str, default='aomenc', help='Choosing encoder')
    parser.add_argument('--workers', '-t', type=int, default=determine_resources(), help='Number of workers')
    parser.add_argument('--audio_params', '-a', type=str, default=DEFAULT_AUDIO, help='ffmpeg audio encode settings')
    args = parser.parse_args()
    return args


def determine_resources():
    """
    Returns number of workers that machine can handle
    :return: int
    """
    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2**30)
    return ceil(min(cpu, ram/1.5))


def setup(input_file):

    if not os.path.exists(input_file):
        print("File don't exist")
        exit()

    # Make temporal directories, and remove them if already presented
    if os.path.isdir(join(os.getcwd(), ".temp")):
        rmtree(join(os.getcwd(), ".temp"))

    os.makedirs(join(os.getcwd(), '.temp', 'split'))
    os.makedirs(join(os.getcwd(), '.temp', 'encode'))


def extract_audio(input_vid, audio_params):
    """
    Extracting audio from video file
    Encoding audio if needed
    """
    cmd = f'{FFMPEG} -i {join(os.getcwd(),input_vid)} -vn {audio_params} {join(os.getcwd(),".temp","audio.mkv")}'
    Popen(cmd, shell=True).wait()


def split_video(input_vid):
    """
    PySceneDetect used split video by scenes and pass it to encoder
    Optimal threshold settings 15-50
    """
    cmd2 = f'scenedetect -q -i {input_vid}  --output .temp/split detect-content --threshold 20 split-video -c'
    call(cmd2, shell=True)
    print(f'Video {input_vid} splitted')


def get_video_queue(source_path):
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


def encode(commands):
    """
    Passing encoding params to ffmpeg for encoding
    TODO:
    Replace ffmpeg with aomenc because ffmpeg libaom doen't work with parameters properly
    """
    for i in commands[:-1]:
        cmd = f'{FFMPEG} -an {i}'
        Popen(cmd, shell=True).wait()


def concatenate_video(input_video):
    """
    Using FFMPEG to concatenate all encoded videos to 1 file.
    Reading all files in A-Z order and saving it to concat.txt
    """
    with open(f'{os.getcwd()}/.temp/concat.txt', 'w') as f:

        for root, firs, files in os.walk(join(os.getcwd(), '.temp', 'encode')):
            for file in sorted(files):
                f.write(f"file '{join(root, file)}'\n")

    concat = join(os.getcwd(), ".temp", "concat.txt")
    audio = join(os.getcwd(), ".temp", "audio.mkv")
    output = f'{input_video.split(".")[0]}_av1.mkv'
    cmd = f'{FFMPEG} -f concat -safe 0 -i {concat} -i {audio} -c copy -y {output}'
    Popen(cmd, shell=True).wait()


def compose_encoding_queue(encoding_params, files, encoder):
    """
    Composing encoding commands
    Examples:
    1_pass:
    ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
    aomenc -q   --passes=1 --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 -o output_file

    2_pass:
    ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
    aomenc -q --passes=2 --pass=1  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o /dev/null -

    ffmpeg -i input_file -pix_fmt yuv420p -f yuv4mpegpipe - |
    aomenc -q --passes=2 --pass=2  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0 --log_file -o output_file -
    """
    file_paths = [(f'{join(os.getcwd(), ".temp", "split", file_name)}',
                   f'{join(os.getcwd(), ".temp", "encode", file_name)}',
                   file_name) for file_name in files]

    print(file_paths[0])

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
                            f' rav1e -  {encoding_params}  --output {file[1]}.ivf', file[2])
                           for file in file_paths]
        return pass_1_commands


def main(arg):

    # Check validity of request and create temp folders/files
    setup(arg.file_path)

    # Extracting audio
    extract_audio(arg.file_path, arg.audio_params)

    # Splitting video and sorting big-first
    split_video(arg.file_path)
    vid_queue = get_video_queue('.temp/split')
    files = [i[0] for i in vid_queue[:-1]]

    # Make encode queue
    commands = compose_encoding_queue(arg.encoding_params, files, arg.encoder)

    # Creating threading pool to encode bunch of files at the same time
    print(f'Starting encoding with {arg.workers} workers. \nParameters:{arg.encoding_params}\nEncoding..')

    # Progress Bar
    bar = ProgressBar(len(vid_queue))

    # async_encode(commands, num_worker)
    pool = Pool(arg.workers)
    for i, _ in enumerate(pool.imap_unordered(encode, commands), 1):
        bar.tick()

    bar.tick()

    # Merging all encoded videos to 1
    concatenate_video(arg.file_path)


if __name__ == '__main__':

    # Main thread
    start = time.time()

    main(arg_parsing())

    print(f'\n Completed in {round(time.time()-start, 1)} seconds')

    # Delete temp folders
    rmtree(join(os.getcwd(), ".temp"))

    # To prevent console from hanging
    os.popen('stty sane', 'r')
