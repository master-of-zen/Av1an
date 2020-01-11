#!/usr/bin/python3
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
#import asyncio
import os
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
except:
    print('ERROR: No PyScenedetect installed, try: sudo pip install scenedetect')


DEFAULT_ENCODE = ' -h 40 -w 70  --passes=1 --tile-columns=2 --tile-rows=2  --cpu-used=8 --end-usage=q --cq-level=63 --aq-mode=0'
DEFAULT_AUDIO = '-c:a libopus -ac 1 -b:a 12k'
FFMPEG = 'ffmpeg -hide_banner -loglevel warning '


class ProgressBar:
    """
    Progress Bar for tracking encoding progress
    """

    def __init__(self,
                 prefix='',
                 suffix='',
                 decimals=1,
                 fill='█',
                 print_end="\r"):
        self.iteration: int = 0
        self.total = 0
        self.prefix = prefix
        self.suffix = suffix
        self.decimals = decimals
        self.length = 50
        self.fill = fill
        self.print_end = print_end
        self.suffix_percent_lenght = 4

    def print(self):
        terminal_size = int(os.popen('stty size', 'r').read().split()[1])
        self.length = terminal_size - 14 - 2 * len(str(self.total))

        if self.iteration == 1:
            percent = 0
            filled_length = 0
        else:
            percent = ("{0:." + str(self.decimals) + "f}").format(100 * (self.iteration / float(self.total)))
            filled_length = int(self.length * self.iteration // self.total)

        bar_size = (self.fill * filled_length) + '-' * (self.length - filled_length)
        
        print(f'\r{self.prefix}|{bar_size}| {percent}% {self.iteration}/{self.total}{self.suffix} ', end='')

    def start(self):
        self.print()
        with open(f'{os.getcwd()}/temp/.counter', 'w+') as f:
            f.write(f'0')

    def tick(self):
        with open(f'{os.getcwd()}/temp/.counter', 'r+') as f:
            f.seek(0)
            counter = int(f.read())
            self.iteration: int = int(counter) + 1
            f.seek(0)
            f.write(f'{self.iteration}')

        self.print()


# Progress Bar initialization
bar = ProgressBar()


def arg_parsing():
    """
    Command line parser
    Have default params
    """

    parser = argparse.ArgumentParser()
    parser.add_argument('--encoding_params', type=str, default=DEFAULT_ENCODE, help='AOMENC settings')
    parser.add_argument('--input_file', '-i', type=str, default='bruh.mp4', help='input video file')
    parser.add_argument('--num_worker', '-t', type=int, default=determine_resources(), help='number of encodes running at a time')
    parser.add_argument('--audio_params', '-a' , type=str, default=DEFAULT_AUDIO, help='ffmpeg audio encode settings')
    return parser.parse_args()


def determine_resources():
    """
    Returns number of workers that machine can handle
    :return: int
    """
    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2**30)
    return ceil(min(cpu, ram/2))


def setup(input_file):

    if not os.path.exists(input_file):
        print("File don't exist")
        exit()

    # Make temporal directories, and remove them if already presented
    if os.path.isdir(join(os.getcwd(), "temp")):
        rmtree(join(os.getcwd(), "temp"))

    os.makedirs(join(os.getcwd(), 'temp', 'split'))
    os.makedirs(join(os.getcwd(), 'temp', 'encode'))


def extract_audio(input_vid, audio_params):
    """
    Extracting audio from video file
    Encoding audio to opus.
    Posible to -acodec copy to .mkv container without reencoding
    """
    cmd = f'{FFMPEG} -i {join(os.getcwd(),input_vid)} -vn {audio_params} {join(os.getcwd(),"temp","audio.mkv")}'
    Popen(cmd, shell=True).wait()


def split_video(input_vid):
    cmd2 = f'scenedetect -q -i {input_vid}  --output temp/split detect-content --threshold 50 split-video -c'
    call(cmd2, shell=True)
    print(f'Video {input_vid} splitted')


def get_video_queue(source_path):
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

    cmd = f'{FFMPEG} {commands[0]}'

    Popen(cmd, shell=True).wait()
    bar.tick()

    # +1 to progress bar after encode is finished


def concat(input_video):
    """
    Using FFMPEG to concatenate all encoded videos to 1 file.
    Reading all files in A-Z order and saving it to concat.txt
    """
    with open(f'{os.getcwd()}/temp/concat.txt', 'w') as f:

        for root, firs, files in os.walk(join(os.getcwd(), 'temp', 'encode')):
            for file in sorted(files):
                f.write(f"file '{join(root, file)}'\n")

    cmd = f'{FFMPEG} -f concat -safe 0 -i {join(os.getcwd(), "temp", "concat.txt")} -i {join(os.getcwd(), "temp", "audio.mkv")} -c copy -y {input_video.split(".")[0]}_av1.webm'
    Popen(cmd, shell=True).wait()


"""
TODO: async encode instead of having pool of python instances
async def async_encode(commands, num_worker=4):
    tasks = [asyncio.create_task(encode(task)) for task in commands]
    sem = asyncio.Semaphore(num_worker)
    for task in tasks:
        await task
"""


def main(arg):

    # Check validity of request and create temp folders/files
    setup(arg.input_file)

    # Extracting audio
    extract_audio(arg.input_file, arg.audio_params)

    # Splitting video and sorting big-first
    split_video(arg.input_file)
    vid_queue = get_video_queue('temp/split')
    files = [i[0] for i in vid_queue[:-1]]

    # Making list of commands for encoding
    commands = [(f'-i {join(os.getcwd(), "temp", "split", file)} -pix_fmt yuv420p -f yuv4mpegpipe - |' +
                f' aomenc -q {arg.encoding_params} -o {join(os.getcwd(), "temp", "encode", file)} -', file) for file in files]

    # Creating threading pool to encode fixed amount of files at the same time
    print(f'Starting encoding with {arg.num_worker} workers. \nParameters:{arg.encoding_params}\nEncoding..')

    # Progress Bar
    bar.total = (len(vid_queue))
    bar.start()

    # async_encode(commands, num_worker)
    pool = Pool(arg.num_worker)
    pool.map(encode, commands)

    # Merging all encoded videos to 1
    concat(arg.input_file)


if __name__ == '__main__':

    # Main thread
    start = time.time()

    main(arg_parsing())

    bar.tick()
    print(f'\nCompleted in {round(time.time()-start, 1)} seconds\n')

    # Delete temp folders
    rmtree(join(os.getcwd(), "temp"))
