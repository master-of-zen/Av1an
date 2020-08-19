import os

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair, Command
from Av1an.encoders.encoder import Encoder
from Av1an.utils import list_index_of_regex


class SvtAv1(Encoder):

    def __init__(self):
        super(SvtAv1, self).__init__(
            encoder_bin='SvtAv1EncApp',
            default_args=['--preset', '4', '--rc', '0', '--qp', '25'],
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Args, c: Chunk, output=None) -> MPCommands:
        if not output:
            output = c.output
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-b', output, '-']
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk, output=None) -> MPCommands:
        if not output:
            output = c.output
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-output-stat-file', f'{c.fpf}.stat', '-b', os.devnull,
                 '-']
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-input-stat-file', f'{c.fpf}.stat', '-b', output,
                 '-']
            )
        ]

    def man_q(self, command: Command, q: int):
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"--qp")
        adjusted_command[i + 1] = f'{q}'

        return adjusted_command
