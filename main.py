import os
import subprocess


def get_cpu_count():
    return os.cpu_count()


def get_ram():
    return round((os.sysconf('SC_PAGE_SIZE') * os.sysconf('SC_PHYS_PAGES')) / (1024. ** 3), 3)


cmd = 'scenedetect --input my_video.mp4 --output my_video_scenes --stats my_video.stats.csv detect-content list-scenes'

d = subprocess.call(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

