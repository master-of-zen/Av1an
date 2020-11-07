#!/bin/env python

import sys
from collections import deque

from libAv1an.Encoders import ENCODERS
from libAv1an.LibAv1an.callbacks import Callbacks


def process_pipe(pipe):
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
        print(f"\nEncoder encountered an error: {pipe.returncode}")
        print('\n'.join(encoder_history))


def process_encoding_pipe(pipe, encoder, cb: Callbacks):
    encoder_history = deque(maxlen=20)
    frame = 0
    enc = ENCODERS[encoder]
    while True:
        line = pipe.stdout.readline().strip()

        if len(line) == 0 and pipe.poll() is not None:
            break

        if len(line) == 0:
            continue

        match = enc.match_line(line, cb)

        if match:
            new = int(match.group(1))
            if new > frame:
                cb.run_callback("newframes", new - frame)
                # counter.update(new - frame)
                frame = new

        if line:
            encoder_history.append(line)

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        print(f"\nEncoder encountered an error: {pipe.returncode}")
        print('\n'.join(encoder_history))
