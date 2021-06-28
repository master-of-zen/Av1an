import sys
import re

from abc import ABC, abstractmethod
from distutils.spawn import find_executable
from typing import Tuple, Optional
import subprocess
from subprocess import PIPE, STDOUT
from av1an.project import Project
from chunk import Chunk
from av1an_pyo3 import (
    encoder_bin,
    compose_ffmpeg_pipe,
    compose_1_1_pass,
    compose_1_2_pass,
    compose_2_2_pass,
    man_command,
)


class Encoder:
    """
    class used for encoders
    """

    def make_pipes(
        self,
        a: Project,
        c: Chunk,
        passes: int,
        current_pass: int,
        output: str,
        man_q: int = None,
    ):

        fpf_file = str(((c.temp / "split") / f"{c.name}_fpf").as_posix())

        if passes == 1:
            enc_cmd = compose_1_1_pass(a.encoder, a.video_params, output)
        if passes == 2:
            if current_pass == 1:
                enc_cmd = compose_1_2_pass(a.encoder, a.video_params, fpf_file)
            if current_pass == 2:
                enc_cmd = compose_2_2_pass(a.encoder, a.video_params, fpf_file, output)

        if man_q:
            enc_cmd = man_command(a.encoder, enc_cmd, man_q)
        elif c.per_shot_target_quality_cq:
            enc_cmd = man_command(a.encoder, enc_cmd, c.per_shot_target_quality_cq)

        ffmpeg_gen_pipe = subprocess.Popen(c.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)
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

        utility = (ffmpeg_gen_pipe, ffmpeg_pipe)
        return pipe, utility
