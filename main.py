#!/usr/bin/python3
"""
mkvmerge required (python-pymkv)
ffmpeg required
TODO:
make encoding queue with limiting by workers,
make concatenating videos,
make passing your arguments for encoding,
make separate audio and encode it separately,
"""

import os
import subprocess
from multiprocessing import Pool
try:
    import scenedetect
except:
    print('ERROR: No PyScenedetect installed, try: sudo pip install scenedetect')


def get_cpu_count():
    return os.cpu_count()


def get_ram():
    return round((os.sysconf('SC_PAGE_SIZE') * os.sysconf('SC_PHYS_PAGES')) / (1024. ** 3), 3)


def split_video(input_vid):
    cmd2 = f'scenedetect -i {input_vid} --output temp/split detect-content split-video -c'
    subprocess.call(cmd2, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def get_video_queue(source_path):
    videos = []
    for root, dirs, files in os.walk(source_path):
        for file in files:
            f = os.path.getsize(os.path.join(root, file))
            videos.append([file, f])

    videos = sorted(videos, key=lambda x: -x[1])
    return videos


def encode(commands):
    print('encoding')
    cmd = f'ffmpeg {commands}'
    subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).wait()


def concat(directory):
    cmd = ''
    subprocess.call(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main(input_video):

    # Make temporal directories
    os.makedirs(f'{os.getcwd()}/temp/split', exist_ok=True)
    os.makedirs(f'{os.getcwd()}/temp/encoded', exist_ok=True)

    # Passing encoding parameters
    encoding_params = ' -an -c:v libaom-av1 -strict -2 -cpu-used 8 -crf 60'

    # Spliting video and sorting big-first
    split_video(input_video)
    vid_queue = get_video_queue('temp')
    files = [i[0] for i in vid_queue[:-1]]

    # Making list of commands for encoding
    commands = [f'-i {os.getcwd()}/temp/split/{file} {encoding_params} {os.getcwd()}/temp/encoded/{file}' for file in files]

    # Creating threading pool to encode fixed amount of files at the same time
    pool = Pool(4)
    pool.map(encode, commands)


if __name__ == '__main__':
    main('bruh.mp4')