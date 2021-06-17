import os
import re
import sys

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from av1an.encoder.encoder import Encoder
from av1an.utils import list_index_of_regex
from av1an_pyo3 import compose_ffmpeg_pipe
from av1an_pyo3 import compose_1_pass as pass1


class SvtAv1(Encoder):
    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                compose_ffmpeg_pipe(a.ffmpeg_pipe),
                pass1(a.encoder, a.video_params, output),
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                compose_ffmpeg_pipe(a.ffmpeg_pipe),
                [
                    "SvtAv1EncApp",
                    "-i",
                    "stdin",
                    "--progress",
                    "2",
                    "--irefresh-type",
                    "2",
                    *a.video_params,
                    "--pass",
                    "1",
                    "--stats",
                    f"{c.fpf}.stat",
                    "-b",
                    os.devnull,
                ],
            ),
            CommandPair(
                compose_ffmpeg_pipe(a.ffmpeg_pipe),
                [
                    "SvtAv1EncApp",
                    "-i",
                    "stdin",
                    "--progress",
                    "2",
                    "--irefresh-type",
                    "2",
                    *a.video_params,
                    "--pass",
                    "2",
                    "--stats",
                    f"{c.fpf}.stat",
                    "-b",
                    output,
                ],
            ),
        ]

    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"(--qp|-q|--crf)")
        adjusted_command[i + 1] = f"{q}"

        return adjusted_command

    def match_line(self, line: str):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :return: match object from re.search matching the number of encoded frames"""
        if "error" in line.lower():
            print("\n\nERROR IN ENCODING PROCESS\n\n", line)
            sys.exit(1)
        return re.search(r"Encoding frame\s+(\d+)", line)
