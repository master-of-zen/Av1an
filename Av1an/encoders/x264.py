import os

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair
from Av1an.encoders.encoder import Encoder


class X264(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='x264',
            default_args=['--preset', 'slow', '--crf', '23'],
            output_extension='mkv'
        )

    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--demuxer', 'y4m', *a.video_params, '-', '-o',
                 c.output]
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--pass', '1', '--demuxer', 'y4m', *a.video_params,
                 '-', '--stats', f'{c.fpf}.log', '-', '-o', os.devnull]
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['x264', '--stitchable', '--log-level', 'error', '--pass', '2', '--demuxer', 'y4m', *a.video_params,
                 '-', '--stats', f'{c.fpf}.log', '-', '-o', c.output]
            )
        ]
