import subprocess
import sys
from collections import deque
from subprocess import PIPE, STDOUT, DEVNULL, Popen
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


def process_pipe(pipe, chunk_index, utility: Iterable[Popen]):
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
        msg1 = (
            f"Encoder encountered an error: {pipe.returncode}\n"
            + f"Chunk: {chunk_index}"
            + "\n".join(encoder_history)
        )
        log(msg1)
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

    ffmpeg_gen_pipe = subprocess.Popen(c.ffmpeg_gen_cmd, stdout=PIPE, stderr=DEVNULL)
    ffmpeg_pipe = subprocess.Popen(
        compose_ffmpeg_pipe(a.ffmpeg_pipe),
        stdin=ffmpeg_gen_pipe.stdout,
        stdout=PIPE,
        stderr=STDOUT,
    )
    pipe = subprocess.Popen(
        enc_cmd,
        stdin=ffmpeg_pipe.stdout,
        stdout=PIPE,
        stderr=STDOUT,
        universal_newlines=True,
    )

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

        new = match_line(encoder, line)
        if new > frame:
            a.counter_update(new - frame)
            frame = new
    if pipe.returncode != 0:

        print("ERROR IN ENCODING PROCESS")
        print("\n".join(encoder_history))
        sys.exit(1)

    for u_pipe in (ffmpeg_gen_pipe, ffmpeg_pipe):
        if u_pipe.poll() is None:
            u_pipe.kill()

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        msg = (
            f"Encoder encountered an error: {pipe.returncode}\n"
            + f"Chunk: {c.index}\n"
            + "\n".join(encoder_history)
        )
        log(msg)
        print(msg)
        tb = sys.exc_info()[2]
        raise RuntimeError("Error in processing encoding pipe").with_traceback(tb)
