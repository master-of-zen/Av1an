import os
import re

from libAv1an.LibAv1an.args import Args
from libAv1an.Chunks.chunk import Chunk
from libAv1an.LibAv1an.commandtypes import MPCommands, CommandPair, Command
from libAv1an.Encoders.encoder import Encoder
from libAv1an.LibAv1an.utils import list_index_of_regex
from libAv1an.LibAv1an.callbacks import Callbacks


class X264(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='x264',
            encoder_help='x264 --fullhelp',
            default_args=['--preset', 'slow', '--crf', '23'],
            default_passes=1,
            default_q_range=(20, 35),
            output_extension='mkv'
        )

    def compose_1_pass(self, a: Args, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--demuxer', 'y4m', *a.video_params, '-', '-o',
                 output]
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk, output: str) -> MPCommands:
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

    def match_line(self, line: str, cb: Callbacks):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :param cb: Callbacks reference in case error (not implemented)
        :return: match object from re.search matching the number of encoded frames"""

        return re.search(r"^[^\d]*(\d+)", line)
