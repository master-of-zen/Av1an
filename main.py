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
    cmd2 = f'scenedetect -i {input_vid} --output temp detect-content split-video -c'
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
    subprocess.Popen(cmd, shell=True).wait()


def main(input_video):
    split_video(input_video)
    vid_queue = get_video_queue('temp')
    files = [i[0] for i in vid_queue[:-1]]
    print(files)
    commands = [f'-i temp/{str(file)} -c:v libaom-av1 -strict -2 -cpu-used 4 -crf 36 {file}' for file in files]
    print('we here')
    pool = Pool(4)
    pool.map(encode, commands)


if __name__ == '__main__':
    main('bruh.mp4')