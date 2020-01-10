#!/usr/bin/python3
"""
mkvmerge required (python-pymkv)
ffmpeg required
TODO:
DONE make encoding queue with limiting by workers
DONE make concatenating videos after encoding
DONE make passing your arguments for encoding,
make arguments help description more understandable
make separate audio and encode it separately,
"""

import os
from os.path import join
from psutil import virtual_memory
import subprocess
import argparse
import time
import shutil
from math import ceil
from multiprocessing import Pool
try:
    import scenedetect
except:
    print('ERROR: No PyScenedetect installed, try: sudo pip install scenedetect')


def arg_parsing():
    """
    Command line parser
    Have default params
    """

    parser = argparse.ArgumentParser()
    parser.add_argument('--encoding_params', type=str,
                        default=' aomenc -q --passes=1   --tile-columns=2 --tile-rows=2  --cpu-used=3 --end-usage=q --cq-level=25 --aq-mode=1  -o',
                        help='FFmpeg settings')
    parser.add_argument('--input_file', '-i', type=str, default='bruh.mp4', help='input video file')
    parser.add_argument('--num_worker', '-t', type=int, default=determine_resources(), help='number of encodes running at a time')
    parser.add_argument('--segment_length', '-L', type=str, default='60', help='Length of each segment, then using segment spliting mode')
    parser.add_argument('--spliting_method', type=str, default='scenedetect', help='method for spliting video [scenedetect/segment]')
    return parser.parse_args()


def determine_resources():
    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2**30)
    return ceil(min(cpu, ram/2))


def extract_audio(input_vid):
    """
    Extracting audio from video file
    Encoding audio to opus.
    Posible to -acodec copy to .mkv container without reencoding
    """
    cmd = f'ffmpeg -i {join(os.getcwd(),input_vid)} -vn -acodec libopus {join(os.getcwd(),"temp","audio.opus")}'
    subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).wait()


def split_video(input_vid, method='scenedetect', segment_length='60'):
    if method == 'scenedetect':
        cmd2 = f'scenedetect -i {input_vid}  --output temp/split detect-content --threshold 50 split-video -c'
    elif method == 'segment':
        cmd2 = f'ffmpeg -i \"{input_vid}\" -c:v copy -segment_time {segment_length} -f segment -reset_timestamps 1 temp/split/segment%03d.mkv'
    else:
        raise SystemExit('Invalid video spliting method')
    subprocess.call(cmd2, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    print(f'Video {input_vid} splitted')


def get_video_queue(source_path):
    videos = []
    for root, dirs, files in os.walk(source_path):
        for file in files:
            f = os.path.getsize(os.path.join(root, file))
            videos.append([file, f])

    videos = sorted(videos, key=lambda x: -x[1])
    return videos


def encode(commands):
    """
    Passing encoding params to ffmpeg for encoding
    TODO:
    Replace ffmpeg with aomenc because ffmpeg libaom doen't work with parameters properly
    """
    cmd = f'ffmpeg {commands}'
    subprocess.Popen(cmd, shell=True).wait()


def concat(input_video):
    """
    Using FFMPEG to concatenate all encoded videos to 1 file.
    Reading all files in A-Z order and saving it to concat.txt
    """
    with open(f'{os.getcwd()}/temp/concat.txt', 'w') as f:

        for root, firs, files in os.walk(join(os.getcwd(), 'temp', 'encode')):
            for file in sorted(files):
                f.write(f"file '{join(root, file)}'\n")

    cmd = f'ffmpeg -f concat -safe 0 -i {join(os.getcwd(), "temp", "concat.txt")} -i {join(os.getcwd(), "temp", "audio.opus")} -c copy {input_video.split(".")[:-1]}-av1.webm'
    subprocess.Popen(cmd, shell=True).wait()


def main(args):

    # Make temporal directories, and remove them if already presented
    if os.path.isdir(join(os.getcwd(), "temp")):
        shutil.rmtree(join(os.getcwd(), "temp"))

    os.makedirs(join(os.getcwd(), 'temp', 'split'))
    os.makedirs(join(os.getcwd(), 'temp', 'encode'))

    # Extracting audio
    extract_audio(args.input_file)

    # Spliting video and sorting big-first
    split_video(args.input_file, args.spliting_method, args.segment_length)
    vid_queue = get_video_queue('temp')
    files = [i[0] for i in vid_queue[:-1]]

    # Making list of commands for encoding
    commands = [f'-i {join(os.getcwd(), "temp", "split", file)} -pix_fmt yuv420p -f yuv4mpegpipe - | {args.encoding_params} {join(os.getcwd(), "temp", "encode", file)} -' for file in files]

    # Creating threading pool to encode fixed amount of files at the same time
    pool = Pool(args.num_worker)
    pool.map(encode, commands)

    # Merging all encoded videos to 1
    concat(args.input_file)


if __name__ == '__main__':

    parse_args = arg_parsing()

    # Main thread
    start = time.time()
    main(parse_args)
    print(f'Encoding completed in {round(time.time()-start)} seconds')

    # Delete temp folders
    shutil.rmtree(join(os.getcwd(), "temp"))
