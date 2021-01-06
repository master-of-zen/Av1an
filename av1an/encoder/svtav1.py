import os
import re

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from av1an.encoder.encoder import Encoder
from av1an.utils import list_index_of_regex, terminate


class SvtAv1(Encoder):

    def __init__(self):
        super(SvtAv1, self).__init__(
            encoder_bin='SvtAv1EncApp',
            encoder_help='SvtAv1EncApp --help',
            default_args=['--preset', '4', '--rc', '0', '--qp', '25'],
            default_passes=1,
            default_q_range=(15, 50),
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', '--progress', '2', *a.video_params, '-b', output, '-']
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', '--progress', '2', '--irefresh-type', '2', *a.video_params,
                 '--pass', '1', '--stats', f'{c.fpf}.stat', '-b', os.devnull, '-']
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', '--progress', '2', '--irefresh-type', '2', *a.video_params,
                 '--pass', '2', '--stats', f'{c.fpf}.stat', '-b', output, '-']
            )
        ]


    def mod_command(self, command, chunk) -> Command:
        """Return new command with q_file

        :param command: old command
        :param q: q list
        :return: command with new cq value"""
        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"(--qp|-q)")

        qp_file = chunk.make_q_file(chunk.per_frame_target_quality_q_list)

        new = ['--use-q-file','1', '--qpfile', f'{qp_file.as_posix()}']
        new_cmd = adjusted_command[:i] + new + adjusted_command[i+2:]

        return new_cmd


    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"(--qp|-q)")
        adjusted_command[i + 1] = f'{q}'

        return adjusted_command

    def match_line(self, line: str):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :return: match object from re.search matching the number of encoded frames"""
        if 'error' in line.lower():
            print('\n\nERROR IN ENCODING PROCESS\n\n', line)
            terminate()
        return re.search(r"Encoding frame\s+(\d+)", line)
