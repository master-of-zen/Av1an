import sys

from collections import deque
from typing import Iterable
from subprocess import Popen

from av1an.chunk import Chunk
from av1an.encoder import ENCODERS

from av1an.project import Project
from av1an.logger import log


def process_pipe(pipe, chunk: Chunk, utility: Iterable[Popen]):
    encoder_history = deque(maxlen=20)
    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break
        if len(line) == 0:
            continue
        if line:
            encoder_history.append(line)

    for u_pipe in utility:
        if u_pipe.poll() is None:
            u_pipe.kill()

    if pipe.returncode != 0 and pipe.returncode != -2:
        msg1 = f"Encoder encountered an error: {pipe.returncode}"
        msg2 = f"Chunk: {chunk.index}" + "\n".join(encoder_history)
        log(msg1, msg2)
        tb = sys.exc_info()[2]
        raise RuntimeError("Error in processing encoding pipe").with_traceback(tb)


def process_encoding_pipe(
    pipe, encoder, counter, chunk: Chunk, utility: Iterable[Popen]
):
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

    for u_pipe in utility:
        if u_pipe.poll() is None:
            u_pipe.kill()

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        msg1 = f"Encoder encountered an error: {pipe.returncode}"
        msg2 = f"Chunk: {chunk.index}"
        msg3 = "\n".join(encoder_history)
        log(msg1, msg2, msg3)
        print(f"::{msg1}\n::{msg2}\n::{msg3}")
        tb = sys.exc_info()[2]
        raise RuntimeError("Error in processing encoding pipe").with_traceback(tb)


def tqdm_bar(
    a: Project, c: Chunk, encoder, counter, frame_probe_source, passes, current_pass
):
    enc = ENCODERS[encoder]
    pipe, utility = enc.make_pipes(a, c, passes, current_pass, c.output)

    if encoder in ("aom", "vpx", "rav1e", "x265", "x264", "vvc", "svt_av1"):
        process_encoding_pipe(pipe, encoder, counter, c, utility)

    if encoder in ("svt_vp9"):
        # SVT-VP9 is special
        process_pipe(pipe, c, utility)
        counter.update(frame_probe_source // passes)
