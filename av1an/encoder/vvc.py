import os
import subprocess
from distutils.spawn import find_executable
from pathlib import Path
from subprocess import PIPE, STDOUT
from typing import Tuple, Optional
import re

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.commandtypes import MPCommands, CommandPair, Command
from .encoder import Encoder
from av1an.logger import log
from av1an.utils import list_index_of_regex


class Vvc(Encoder):
    """
    Redo after VVenC default and expert app have concatenation
    """


    def __init__(self):
        super(Vvc, self).__init__(
            encoder_bin='vvc_encoder',
            encoder_help='vvc_encoder --help',
            default_args=None,
            default_passes=1,
            default_q_range=(15, 50),
            output_extension='h266'
        )

    def compose_1_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        yuv_file: str = Vvc.get_yuv_file_path(c).as_posix()
        return [
            CommandPair(
                [],
                ['vvc_encoder', '-c', a.vvc_conf, '-i', yuv_file, *a.video_params, '-f', str(c.frames),
                 '--InputBitDepth=10', '--OutputBitDepth=10', '-b', output]
            )
        ]

    def compose_2_pass(self, a: Project, c: Chunk, output: str) -> MPCommands:
        raise ValueError('VVC does not support 2 pass encoding')

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

        return re.search(r"POC.*? ([^ ]+?)", line)

    def make_pipes(self, a: Project, c: Chunk, passes: int, current_pass: int, output: str, man_q: int = None):
        """
        Creates a pipe for the given chunk with the given project

        :param a: the Project
        :param c: the Chunk
        :param passes: the total number of passes (1 or 2)
        :param current_pass: the current_pass
        :param man_q: use a diffrent quality
        :return: a Pipe attached to the encoders stdout
        """
        # Filter cmd not used?
        filter_cmd, enc_cmd = self.compose_1_pass(a, c, output)[0] if passes == 1 else \
                              self.compose_2_pass(a, c, output)[current_pass - 1]

        if man_q:
            enc_cmd = self.man_q(enc_cmd, man_q)
        elif c.vmaf_target_cq:
            enc_cmd = self.man_q(enc_cmd, c.vmaf_target_cq)

        pipe = subprocess.Popen(enc_cmd, stdout=PIPE,
                                stderr=STDOUT,
                                universal_newlines=True)
        return pipe

    def is_valid(self, project: Project) -> Tuple[bool, Optional[str]]:
        # vvc requires a special concat executable
        if not find_executable('vvc_concat'):
            return False, 'vvc concatenation executable "vvc_concat" not found'

        # make sure there's a vvc config file
        if project.vvc_conf is None:
            return False, 'Conf file for vvc required'

        # vvc requires video information that av1an can't provide
        if project.video_params is None:
            return False, 'VVC requires:\n' \
                          ' -wdt X - video width\n' \
                          ' -hgt X - video height\n' \
                          ' -fr X  - framerate\n' \
                          ' -q X   - quantizer\n' \
                          'Example: -wdt 640 -hgt 360 -fr 23.98 -q 30'

        return super().is_valid(project)

    def on_before_chunk(self, project: Project, chunk: Chunk) -> None:
        # vvc requires a yuv files as input, make it here
        log(f'Creating yuv for chunk {chunk.name}\n')
        Vvc.to_yuv(chunk)
        log(f'Created yuv for chunk {chunk.name}\n')
        super().on_before_chunk(project, chunk)

    def on_after_chunk(self, project: Project, chunk: Chunk) -> None:
        # delete the yuv file for this chunk
        yuv_path = Vvc.get_yuv_file_path(chunk)
        os.remove(yuv_path)
        super().on_after_chunk(project, chunk)

    @staticmethod
    def get_yuv_file_path(chunk: Chunk) -> Path:
        """
        Gets the yuv path to be used for a given chunk

        :param chunk: the Chunk
        :return: a yuv file path for the chunk
        """
        return (chunk.temp / 'split') / f'{chunk.name}.yuv'

    @staticmethod
    def to_yuv(chunk: Chunk) -> None:
        """
        Generates a yuv file for a given chunk

        :param chunk: the Chunk
        :return: None
        """
        yuv_path = Vvc.get_yuv_file_path(chunk)

        ffmpeg_gen_pipe = subprocess.Popen(chunk.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)

        # TODO: apply ffmpeg filter to the yuv file
        cmd = ['ffmpeg', '-y', '-loglevel', 'error', '-i', '-', '-f', 'rawvideo', '-vf', 'format=yuv420p10le',
               yuv_path.as_posix()]
        pipe = subprocess.Popen(cmd, stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT, universal_newlines=True)
        pipe.wait()
