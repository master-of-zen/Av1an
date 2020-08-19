import os

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair
from Av1an.encoders.encoder import Encoder


class Vpx(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='vpxenc',
            default_args=['--codec=vp9', '--threads=4', '--cpu-used=0', '--end-usage=q', '--cq-level=30'],
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['vpxenc', '--passes=1', *a.video_params, '-o', c.output, '-']
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['vpxenc', '--passes=2', '--pass=1', *a.video_params, f'--fpf={c.fpf}', '-o', os.devnull, '-']
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['vpxenc', '--passes=2', '--pass=2', *a.video_params, f'--fpf={c.fpf}', '-o', c.output, '-']
            )
        ]
