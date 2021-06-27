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

    @staticmethod
    def compose_1_pass(a: Project, c: Chunk, output: str):
        return [
            compose_ffmpeg_pipe(a.ffmpeg_pipe),
            compose_1_1_pass(a.encoder, a.video_params, output),
        ]

    @staticmethod
    def compose_2_pass(a: Project, c: Chunk, output: str):
        return [
            (
                compose_ffmpeg_pipe(a.ffmpeg_pipe),
                compose_1_2_pass(a.encoder, a.video_params, c.fpf),
            ),
            (
                compose_ffmpeg_pipe(a.ffmpeg_pipe),
                compose_2_2_pass(a.encoder, a.video_params, c.fpf, output),
            ),
        ]

    def make_pipes(
        self,
        a: Project,
        c: Chunk,
        passes: int,
        current_pass: int,
        output: str,
        man_q: int = None,
    ):
        """
        Creates a pipe for the given chunk with the given args

        :param a: the Project
        :param c: the Chunk
        :param passes: the total number of passes (1 or 2)
        :param current_pass: the current_pass
        :param output: path posix string for encoded output
        :param man_q: use a different quality
        :return: a Pipe attached to the encoders stdout
        """
        filter_cmd, enc_cmd = (
            Encoder.compose_1_pass(a, c, output)[0]
            if passes == 1
            else self.compose_2_pass(a, c, output)[current_pass - 1]
        )
        if man_q:
            enc_cmd = man_command(a.encoder, enc_cmd, man_q)
        elif c.per_shot_target_quality_cq:
            enc_cmd = man_command(a.encoder, enc_cmd, c.per_shot_target_quality_cq)

        ffmpeg_gen_pipe = subprocess.Popen(c.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)
        ffmpeg_pipe = subprocess.Popen(
            filter_cmd, stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT
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
