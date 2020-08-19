from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import MPCommands, CommandPair, Command
from Av1an.encoders.encoder import Encoder


class SvtVp9(Encoder):

    def __init__(self):
        super(SvtVp9, self).__init__(
            encoder_bin='SvtVp9EncApp',
            default_args=None,
            output_extension='ivf'
        )

    @staticmethod
    def compose_ffmpeg_raw_pipe(a: Args) -> Command:
        """
        Compose a rawvideo ffmpeg pipe for svt-vp9
        SVT-VP9 requires rawvideo, so we can't use arg.ffmpeg_pipe

        :param a: the Args
        :return: a command
        """
        return ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', *a.ffmpeg, *a.pix_format, '-bufsize',
                '50000K', '-f', 'rawvideo', '-']

    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        return [
            CommandPair(
                SvtVp9.compose_ffmpeg_raw_pipe(a),
                ['SvtVp9EncApp', '-i', 'stdin', '-n', f'{c.frames}', *a.video_params, '-b', c.output]
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        raise ValueError("SVT-VP9 doesn't support 2 pass")
