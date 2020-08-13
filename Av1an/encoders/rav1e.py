import os

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair
from Av1an.encoders.encoder import Encoder


class Rav1e(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='rav1e',
            default_args=['--tiles', '8', '--speed', '6', '--quantizer', '100'],
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(Encoder.compose_ffmpeg_pipe(a), ['rav1e', '-', *a.video_params, '--output', c.output])
        ]

    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(Encoder.compose_ffmpeg_pipe(a), ['rav1e', '-', '--first-pass', f'{c.fpf}.stat', *a.video_params, '--output', os.devnull]),
            CommandPair(Encoder.compose_ffmpeg_pipe(a), ['rav1e', '-', '--second-pass', f'{c.fpf}.stat', *a.video_params, '--output', c.output])
        ]
