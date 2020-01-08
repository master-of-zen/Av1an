#!/usr/bin/python3
"""
mkvmerge required (python-pymkv)
ffmpeg required
TODO:
DONE make encoding queue with limiting by workers
DONE make concatenating videos after encoding
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


def extract_audio(input_vid):
    cmd = f'ffmpeg -i {os.getcwd()}/{input_vid} -vn -acodec copy {os.getcwd()}/temp/audio.aac'


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
    cmd = f'ffmpeg {commands}'
    subprocess.Popen(cmd, shell=True).wait()


def concat():
    """
    Using FFMPEG to concatenate all encoded videos to 1 file.
    Reading all files in A-Z order and saving it to concat.txt
    """
    with open(f'{os.getcwd()}/temp/concat.txt', 'w') as f:

        for root, firs, files in os.walk(f'{os.getcwd()}/temp/encode'):
            for file in sorted(files):
                f.write(f"file '{os.path.join(root, file)}'\n")

    cmd = f'ffmpeg -f concat -safe 0 -i {os.getcwd()}/temp/concat.txt -c copy output.mp4'
    subprocess.Popen(cmd, shell=True).wait()


def main(input_video):

    # Make temporal directories
    os.makedirs(f'{os.getcwd()}/temp/split', exist_ok=True)
    os.makedirs(f'{os.getcwd()}/temp/encode', exist_ok=True)

    # Passing encoding parameters
    #                   no audio  av1 codec              adding tiles        speed      quality
    encoding_params = ' -an -c:v libaom-av1 -strict -2 -row-mt 1 -tiles 2x2 -cpu-used 8 -crf 60 '

    # Spliting video and sorting big-first
    split_video(input_video)
    vid_queue = get_video_queue('temp')
    files = [i[0] for i in vid_queue[:-1]]

    # Making list of commands for encoding
    commands = [f'-i {os.getcwd()}/temp/split/{file} {encoding_params} {os.getcwd()}/temp/encode/{file}' for file in files]

    # Creating threading pool to encode fixed amount of files at the same time
    pool = Pool(8)
    pool.map(encode, commands)

    # Merging all encoded videos to 1
    concat()


if __name__ == '__main__':
    main('bruh.mp4')