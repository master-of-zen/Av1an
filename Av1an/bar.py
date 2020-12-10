#!/bin/env python

import sys
from collections import deque
from multiprocessing.managers import BaseManager
from subprocess import STDOUT
from tqdm import tqdm

from .commandtypes import CommandPair

from Encoders import ENCODERS
from Projects import Project
from Chunks.chunk import Chunk


def Manager():
    """
    Thread save manager for frame counter
    """
    m = BaseManager()
    m.start()
    return m


class Counter:
    """
    Frame Counter based on TQDM
    """
    def __init__(self, total, initial):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.tqdm_bar = tqdm(total=self.left, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.01)

    def update(self, value):
        if self.first_update:
            self.tqdm_bar.reset(self.left)
            self.first_update = False
        self.tqdm_bar.update(value)

    def close(self):
        self.tqdm_bar.close()


BaseManager.register('Counter', Counter)


def process_pipe(pipe, chunk: Chunk):
    encoder_history = deque(maxlen=20)
    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break
        if len(line) == 0:
            continue
        if line:
            encoder_history.append(line)

    if pipe.returncode != 0 and pipe.returncode != -2:
        print(f"\n:: Encoder encountered an error: {pipe.returncode}")
        print(f"\n:: Chunk: {chunk.index}")
        print('\n'.join(encoder_history))


def process_encoding_pipe(pipe, encoder, counter, chunk: Chunk):
    encoder_history = deque(maxlen=20)
    frame = 0
    enc = ENCODERS[encoder]
    while True:
        line = pipe.stdout.readline().strip()

        if len(line) == 0 and pipe.poll() is not None:
            break

        if len(line) == 0:
            continue

        match = enc.match_line(line)

        if match:
            new = int(match.group(1))
            if new > frame:
                counter.update(new - frame)
                frame = new

        if line:
            encoder_history.append(line)

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom

        print(f"\n:: Encoder encountered an error: {pipe.returncode}")
        print(f"\n:: Chunk: {chunk.index}")
        print('\n'.join(encoder_history))


def tqdm_bar(a: Project, c: Chunk, encoder, counter, frame_probe_source, passes, current_pass):
    try:

        enc = ENCODERS[encoder]
        pipe = enc.make_pipes(a, c, passes, current_pass, c.output)

        if encoder in ('aom', 'vpx', 'rav1e', 'x265', 'x264', 'vvc', 'svt_av1'):
            process_encoding_pipe(pipe, encoder, counter, c)

        if encoder in ('svt_vp9'):
            # SVT-VP9 is special
            process_pipe(pipe, c)
            counter.update(frame_probe_source // passes)

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'\n:: Error at encode {e}\nAt line {exc_tb.tb_lineno}')
