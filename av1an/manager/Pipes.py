import subprocess
import sys
from collections import deque
from subprocess import PIPE, STDOUT, Popen
from typing import Iterable

from av1an.chunk import Chunk
from av1an.project import Project
from av1an_pyo3 import (
    compose_1_1_pass,
    compose_1_2_pass,
    compose_2_2_pass,
    compose_ffmpeg_pipe,
    log,
    man_command,
    match_line,
)


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
        log(msg1)
        log(msg2)
        tb = sys.exc_info()[2]
        raise RuntimeError("Error in processing encoding pipe").with_traceback(tb)


def process_encoding_pipe(
    pipe, encoder, counter, chunk: Chunk, utility: Iterable[Popen]
):
    encoder_history = deque(maxlen=20)
    frame = 0
    while True:
        line = pipe.stdout.readline().strip()

        if len(line) == 0 and pipe.poll() is not None:
            break

        if len(line) == 0:
            continue

        if len(line) > 1:
            encoder_history.append(line)

        if "fatal" in line.lower() or "error" in line.lower():
            print("ERROR IN ENCODING PROCESS")
            print("\n".join(encoder_history))
            sys.exit(1)

        new = match_line(encoder, line)
        if new > frame:
            counter.update(new - frame)
            frame = new

    for u_pipe in utility:
        if u_pipe.poll() is None:
            u_pipe.kill()

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        msg1 = f"Encoder encountered an error: {pipe.returncode}"
        msg2 = f"Chunk: {chunk.index}"
        msg3 = "\n".join(encoder_history)
        log(msg1)
        log(msg2)
        log(msg3)
        print(f"::{msg1}\n::{msg2}\n::{msg3}")
        tb = sys.exc_info()[2]
        raise RuntimeError("Error in processing encoding pipe").with_traceback(tb)


def create_pipes(
    a: Project, c: Chunk, encoder, counter, frame_probe_source, passes, current_pass
):

    fpf_file = str(((c.temp / "split") / f"{c.name}_fpf").as_posix())

    if passes == 1:
        enc_cmd = compose_1_1_pass(a.encoder, a.video_params, c.output)
    if passes == 2:
        if current_pass == 1:
            enc_cmd = compose_1_2_pass(a.encoder, a.video_params, fpf_file)
        if current_pass == 2:
            enc_cmd = compose_2_2_pass(a.encoder, a.video_params, fpf_file, c.output)

    if c.per_shot_target_quality_cq:
        enc_cmd = man_command(a.encoder, enc_cmd, c.per_shot_target_quality_cq)

    ffmpeg_gen_pipe = subprocess.Popen(c.ffmpeg_gen_cmd, stdout=PIPE)
    ffmpeg_pipe = subprocess.Popen(
        compose_ffmpeg_pipe(a.ffmpeg_pipe),
        stdin=ffmpeg_gen_pipe.stdout,
        stdout=PIPE,
        stderr=STDOUT
    )
    pipe = subprocess.Popen(
        enc_cmd,
        stdin=ffmpeg_pipe.stdout,
        stdout=PIPE,
        stderr=STDOUT,
        universal_newlines=True
    )

    utility = (ffmpeg_gen_pipe, ffmpeg_pipe)
    process_encoding_pipe(pipe, encoder, counter, c, utility)
