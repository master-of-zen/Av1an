#!/usr/bin/python3
"""
mkvmerge required (python-pymkv)
"""

import sys
import os
import subprocess
import scenedetect


def get_cpu_count():
    return os.cpu_count()


def get_ram():
    return round((os.sysconf('SC_PAGE_SIZE') * os.sysconf('SC_PHYS_PAGES')) / (1024. ** 3), 3)


def split_video(input_vid):
    cmd2 = f'scenedetect -i {input_vid} --output output detect-content list-scenes split-video -c'
    subprocess.call(cmd2, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

