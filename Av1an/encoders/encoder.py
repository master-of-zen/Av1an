from abc import ABC, abstractmethod
from distutils.spawn import find_executable
import subprocess
from subprocess import PIPE, STDOUT

from Av1an.arg_parse import Args
from Av1an.chunk import Chunk
from Av1an.commandtypes import Command, MPCommands


class Encoder(ABC):
    """
    An abstract class used for encoders
    """

    def __init__(self, encoder_bin: str, default_args: Command, output_extension: str):
        """
        Encoder constructor

        :param encoder_bin: the binary for the encoder
        :param default_args: the default cli args for the encoder
        :param output_extension: the output extension (no dot)
        """
        self.encoder_bin = encoder_bin
        self.default_args = default_args
        self.output_extension = output_extension

    @staticmethod
    def compose_ffmpeg_pipe(a: Args) -> Command:
        """
        Creates an ffmpeg pipe command for the args

        :param a: the Args
        :return: an ffmpeg command that will pipe into the encoder
        """
        return ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', *a.ffmpeg_pipe]

    @abstractmethod
    def compose_1_pass(self, a: Args, c: Chunk) -> MPCommands:
        """
        Composes the commands needed for a 1 pass encode

        :param a: the Args
        :param c: the Chunk
        :return: a MPCommands object (a list of CommandPairs)
        """
        pass

    @abstractmethod
    def compose_2_pass(self, a: Args, c: Chunk) -> MPCommands:
        """
        Composes the commands needed for a 2 pass encode

        :param a: the Args
        :param c: the Chunk
        :return: a MPCommands object (a list of CommandPairs)
        """
        pass

    @abstractmethod
    def man_q(self, command: Command, q: int):
        """Return command with new cq value

        :param command: old command
        :param q: new cq value
        :return: command with new cq value"""
        pass

    def make_pipes(self, a: Args, c: Chunk, passes, current_pass, man_q=None, output=None):
        """
        reates a pipe for the given chunk with the given args

        :param a: the Args
        :param c: the Chunk
        :param passes: the total number of passes (1 or 2)
        :param current_pass: the current_pass
        :param man_q: use a diffrent quality
        :return: a Pipe attached to the encoders stdout
        """
        filter_cmd, enc_cmd = self.compose_1_pass(a, c, output)[0] if passes == 1 else \
                              self.compose_2_pass(a, c, output)[current_pass - 1]

        if man_q:
            enc_cmd = self.man_q(enc_cmd, man_q)
        elif c.vmaf_target_cq:
            enc_cmd = self.man_q(enc_cmd, c.vmaf_target_cq)

        ffmpeg_gen_pipe = subprocess.Popen(c.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT)
        ffmpeg_pipe = subprocess.Popen(filter_cmd, stdin=ffmpeg_gen_pipe.stdout, stdout=PIPE, stderr=STDOUT)
        pipe = subprocess.Popen(enc_cmd, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                                stderr=STDOUT,
                                universal_newlines=True)

        return pipe

    def check_exists(self) -> bool:
        """
        Verifies that this encoder exists in the system path and is ok to use

        :return: True if the encoder bin exists
        """
        return find_executable(self.encoder_bin) is not None

    def __eq__(self, o: object) -> bool:
        """
        Supports equality of encoders based on encoder_bin

        :param o: other object
        :return: True iff o is an EncoderBC and self.encoder_bin == o.encoder_bin
        """
        if not isinstance(o, Encoder):
            return False
        return self.encoder_bin == o.encoder_bin
