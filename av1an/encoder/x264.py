import os
import re

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from .encoder import Encoder
from av1an.utils import list_index_of_regex


class X264(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='x264',
            encoder_help='x264 --fullhelp',
            default_args=['--preset', 'slow', '--crf', '25'],
            default_passes=1,
            default_q_range=(15, 35),
            output_extension='mkv'
        )

    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--demuxer', 'y4m', *a.video_params, '-', '-o',
                 output]
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--pass', '1', '--demuxer', 'y4m', *a.video_params,
                 '-', '--stats', f'{c.fpf}.log', '-', '-o', os.devnull]
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--pass', '2', '--demuxer', 'y4m', *a.video_params,
                 '-', '--stats', f'{c.fpf}.log', '-', '-o', output]
            )
        ]

    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"--crf")
        adjusted_command[i + 1] = f'{q}'

        return adjusted_command

    def match_line(self, line: str):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :return: match object from re.search matching the number of encoded frames"""

        return re.search(r"^[^\d]*(\d+)", line)
