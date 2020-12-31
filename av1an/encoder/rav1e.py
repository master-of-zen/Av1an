import os
import re

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from av1an.encoder.encoder import Encoder
from av1an.utils import list_index_of_regex, terminate


class Rav1e(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='rav1e',
            encoder_help='rav1e --fullhelp',
            default_args=['--tiles', '8', '--speed', '6', '--quantizer', '100'],
            default_passes=1,
            default_q_range=(50, 140),
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['rav1e', '-', '-y', *a.video_params, '--output', output]
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['rav1e', '-', '-q', '-y', '--first-pass', f'{c.fpf}.stat', *a.video_params, '--output', os.devnull]
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['rav1e', '-', '-y', '--second-pass', f'{c.fpf}.stat', *a.video_params, '--output', output]
            )
        ]

    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"--quantizer")
        adjusted_command[i + 1] = f'{q}'

        return adjusted_command

    def match_line(self, line: str):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :return: match object from re.search matching the number of encoded frames"""

        if 'error' in line.lower():
            print('\n\nERROR IN ENCODING PROCESS\n\n', line)
            terminate()
        return re.search(r"encoded.*? ([^ ]+?) ", line)
