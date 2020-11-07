import os
import re
from libAv1an.Chunks.chunk import Chunk
from libAv1an.Encoders.encoder import Encoder

from libAv1an.LibAv1an.args import Args
from libAv1an.LibAv1an.commandtypes import MPCommands, CommandPair, Command
from libAv1an.LibAv1an.utils import list_index_of_regex
from libAv1an.LibAv1an.callbacks import Callbacks


class Aom(Encoder):

    def __init__(self):
        super().__init__(
            encoder_bin='aomenc',
            encoder_help='aomenc --help',
            default_args=['--threads=4', '--cpu-used=6', '--end-usage=q', '--cq-level=30'],
            default_passes=2,
            default_q_range=(25, 50),
            output_extension='ivf'
        )

    def compose_1_pass(self, a: Args, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['aomenc', '--passes=1', *a.video_params, '-o', output, '-']
            )
        ]

    def compose_2_pass(self, a: Args, c: Chunk, output: str) -> MPCommands:
        return [
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['aomenc', '--passes=2', '--pass=1', *a.video_params, f'--fpf={c.fpf}.log', '-o', os.devnull, '-']
            ),
            CommandPair(
                Encoder.compose_ffmpeg_pipe(a),
                ['aomenc', '--passes=2', '--pass=2', *a.video_params, f'--fpf={c.fpf}.log', '-o', output, '-']
            )
        ]

    def man_q(self, command: Command, q: int) -> Command:
        """Return command with new cq value"""

        adjusted_command = command.copy()

        i = list_index_of_regex(adjusted_command, r"--cq-level=.+")
        adjusted_command[i] = f'--cq-level={q}'

        return adjusted_command

    def match_line(self, line: str, cb: Callbacks):
        """Extract number of encoded frames from line.

        :param line: one line of text output from the encoder
        :param cb: Callbacks reference in case error
        :return: match object from re.search matching the number of encoded frames"""

        if 'fatal' in line.lower():
            print('\n\nERROR IN ENCODING PROCESS\n\n', line)
            cb.run_callback("terminate", 1)
        if 'Pass 2/2' in line or 'Pass 1/1' in line:
            return re.search(r"frame.*?/([^ ]+?) ", line)
