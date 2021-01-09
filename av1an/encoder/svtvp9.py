from typing import Tuple, Optional

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from av1an.encoder.encoder import Encoder
from av1an.utils import list_index_of_regex


class SvtVp9(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='SvtVp9EncApp',
            encoder_help='SvtVp9EncApp --help',
            default_args=None,
            default_passes=1,
            default_q_range=(15, 55),
            output_extension='ivf'
        )

    @staticmethod
    def compose_ffmpeg_raw_pipe(a: Project) -> Command:
        """
        Compose a rawvideo ffmpeg pipe for svt-vp9
        SVT-VP9 requires rawvideo, so we can't use arg.ffmpeg_pipe

        :param a: the Project
        :return: a command
        """
        return ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', *a.ffmpeg, *a.pix_format,'-f', 'rawvideo', '-']

    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                SvtVp9.compose_ffmpeg_raw_pipe(a),
                ['SvtVp9EncApp', '-i', 'stdin', '-n', f'{c.frames}', *a.video_params, '-b', output]
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        raise ValueError("SVT-VP9 doesn't support 2 pass")

    def is_valid(self, project: Project) -> Tuple[bool, Optional[str]]:
        if project.video_params is None:
            return False, 'SVT-VP9 requires: -w, -h, and -fps/-fps-num/-fps-denom'
        return super().is_valid(project)

    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"-q")
        adjusted_command[i + 1] = f'{q}'

        return adjusted_command

    def match_line(self, line: str):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :return: match object from re.search matching the number of encoded frames"""
        pass  # todo: SVT encoders are special in the way they output to console
