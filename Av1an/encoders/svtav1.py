import os

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair
from Av1an.encoders.encoder import Encoder


class SvtAv1(Encoder):

    def __init__(self):
        super(SvtAv1, self).__init__(
            encoder_bin='SvtAv1EncApp',
            default_args=['--preset', '4', '--rc', '0', '--qp', '25'],
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-b', c.output, '-']
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-output-stat-file', f'{c.fpf}.stat', '-b', os.devnull,
                 '-']
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['SvtAv1EncApp', '-i', 'stdin', *a.video_params, '-input-stat-file', f'{c.fpf}.stat', '-b', c.output,
                 '-']
            )
        ]
