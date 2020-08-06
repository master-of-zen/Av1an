from pathlib import Path
from typing import Dict, Any

from .arg_parse import Args
import Av1an


class Chunk:
    """
    Chunk class. Stores information relating to a chunk. The command that gets the chunk and the encoding commands
    to be run on this chunk.
    """

    def __init__(self, temp: Path, index: int, ffmpeg_gen_cmd: str, output_ext: str, size: int, frames: int):
        """
        Chunk class constructor

        :param temp: The temp directory
        :param index: the index of this chunk
        :param ffmpeg_gen_cmd: the ffmpeg command that will generate this chunk
        :param output_ext: the output extension after encoding
        :param size: the size of this chunk. used for sorting
        :param frames: the number of frames in this chunk
        """
        self.index = index
        self.ffmpeg_gen_cmd = ffmpeg_gen_cmd
        self.size = size
        self.temp = temp
        self.pass_cmds = []
        self.frames = frames
        self.output_ext = output_ext

    def generate_pass_cmds(self, args: Args):
        """
        Generates and sets the encoding commands for this chunk

        :param args: the Args
        :return: None
        """
        self.pass_cmds = Av1an.gen_pass_commands(args, self)

    def remove_first_pass_from_commands(self):
        """
        Removes the first pass command from the list of commands.
        Used with first pass reuse

        :return: None
        """
        # just one pass to begin with, do nothing
        if len(self.pass_cmds) == 1:
            return

        # passes >= 2, remove the command for first pass (pass_cmds[0])
        self.pass_cmds = self.pass_cmds[1:]

    def to_dict(self) -> Dict[str, Any]:
        """
        Converts this chunk to a dictionary for easy json serialization

        :return: A dictionary
        """
        return {
            'index': self.index,
            'ffmpeg_gen_cmd': self.ffmpeg_gen_cmd,
            'size': self.size,
            'pass_cmds': self.pass_cmds,
            'frames': self.frames,
            'output_ext': self.output_ext,
        }

    @property
    def fake_input_path(self) -> Path:
        """
        Returns the mkv chunk file that would have been for this chunk in the old chunk system.
        Ex: .temp/split/00000.mkv

        :return: a path
        """
        return (self.temp / 'split') / f'{self.name}.mkv'

    @property
    def output_path(self) -> Path:
        """
        Gets the path of this chunk after being encoded with an extension
        Ex: Path('.temp/encode/00000.ivf')

        :return: the Path of this encoded chunk
        """
        return (self.temp / 'encode') / f'{self.name}.{self.output_ext}'

    @property
    def output(self) -> str:
        """
        Gets the posix string of this chunk's output_path (with extension)
        See: Chunk.output_path
        Ex: '.temp/encode/00000.ivf'

        :return: the string of this chunk's output path
        """
        return self.output_path.as_posix()

    @property
    def fpf(self) -> str:
        """
        Gets the posix string of this chunks first pass file without an extension
        Ex: '.temp/split/00000_fpf'

        :return: the string of this chunk's first pass file (no extension)
        """
        fpf_file = (self.temp / 'split') / f'{self.name}_fpf'
        return fpf_file.as_posix()

    @property
    def name(self) -> str:
        """
        Gets the name of this chunk. It is the index zero padded to length 5 as a string.
        Ex: '00000'

        :return: the name of this chunk as a string
        """
        return str(self.index).zfill(5)

    @staticmethod
    def create_from_dict(d: dict, temp):
        """
        Creates a chunk from a dictionary.
        See: Chunk.to_dict

        :param d: the dictionary
        :param temp: the temp directory
        :return: A Chunk from the dictionary
        """
        chunk = Chunk(temp, d['index'], d['ffmpeg_gen_cmd'], d['output_ext'], d['size'], d['frames'])
        chunk.pass_cmds = d['pass_cmds']
        return chunk
