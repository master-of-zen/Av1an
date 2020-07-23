#!/bin/env python

import re
import subprocess
import sys
from collections import deque
from multiprocessing.managers import BaseManager
from subprocess import PIPE, STDOUT
from tqdm import tqdm

from .utils import terminate


def Manager():
    m = BaseManager()
    m.start()
    return m


class Counter():
    def __init__(self, total, initial):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.tqdm_bar = tqdm(total=self.left, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)

    def update(self, value):
        if self.first_update:
            self.tqdm_bar.reset(self.left)
            self.first_update = False
        self.tqdm_bar.update(value)


BaseManager.register('Counter', Counter)

def make_pipes(command):

    f, e = command.split('|')
    f, e = f.split(), e.split()
    ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                            stderr=STDOUT,
                            universal_newlines=True)

    return pipe


def make_vvc_pipe(command):
    pipe = subprocess.Popen(command.split(), stdout=PIPE,
                            stderr=STDOUT,
                            universal_newlines=True)
    return pipe

def match_aom_vpx(line):
    if 'fatal' in line.lower():
        print('\n\nERROR IN ENCODING PROCESS\n\n', line)
        terminate()
    if 'Pass 2/2' in line or 'Pass 1/1' in line:
        return re.search(r"frame.*?\/([^ ]+?) ", line)


def match_rav1e(line):
    if 'error' in line.lower():
        print('\n\nERROR IN ENCODING PROCESS\n\n', line)
        terminate()
    return re.search(r"encoded.*? ([^ ]+?) ", line)


def match_vvc(line):
    return re.search(r"POC.*? ([^ ]+?)", line)


def process_encoding_pipe(pipe, encoder, counter):
    encoder_history = deque(maxlen=20)
    frame = 0
    pass_1_check = True
    skip_1_pass = False
    while True:
        line = pipe.stdout.readline().strip()

        if len(line) == 0 and pipe.poll() is not None:
            break

        if len(line) == 0:
            continue

        if encoder in ('aom', 'vpx'):
            match = match_aom_vpx(line)

        elif encoder == 'rav1e':
            match = match_rav1e(line)

        if encoder in  ('x265'):
            if not skip_1_pass and pass_1_check:
                if 'output file' in line:
                    if 'nul' in line.lower():
                        skip_1_pass = True
                    else:
                        pass_1_check = False
            if not skip_1_pass:
                match = re.search(r"^(\d+)", line)

        if encoder in ('vvc'):
            match = match_vvc(line)
            if match:
                counter.update(1)
                continue

        if match:
            new = int(match.group(1))
            if new > frame:
                counter.update(new - frame)
                frame = new

        if line:
            encoder_history.append(line)

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        print(f"\nEncoder encountered an error: {pipe.returncode}")
        print('\n'.join(encoder_history))


def tqdm_bar(i, encoder, counter, frame_probe_source, passes):
    try:

        if encoder in 'vvc':
            pipe = make_vvc_pipe(i)
        else:
            pipe = make_pipes(i)

        if encoder in ('aom', 'vpx', 'rav1e','x265', 'vvc'):
            process_encoding_pipe(pipe, encoder, counter)

        if encoder == 'svt_av1':
            # SVT-AV1 developer: SVT-AV1 is special in the way it outputs to console
            pipe.wait()
            counter.update(frame_probe_source // passes)
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')