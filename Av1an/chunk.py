from pathlib import Path
from typing import Tuple

from .arg_parse import Args
import Av1an


class Chunk:
    """
    Chunk class. Stores information relating to a chunk. The command that gets the chunk and the encoding commands
    to be run on this chunk.
    """

    def __init__(self, index: int, ffmpeg_gen_cmd: str, size: int, temp: Path, frames: int, output_ext: str):
        self.index = index
        self.ffmpeg_gen_cmd = ffmpeg_gen_cmd
        self.size = size
        self.temp = temp
        self.pass_cmds = []
        self.frames = frames
        self.output_ext = output_ext

    def generate_pass_cmds(self, args: Args):
        self.pass_cmds = Av1an.gen_pass_commands(args, self)

    def remove_first_pass_from_commands(self):
        """
        Removes the first pass command from the list of commands since we generated the first pass file ourselves.

        :return: None
        """
        # just one pass to begin with, do nothing
        if len(self.pass_cmds) == 1:
            return

        # passes >= 2, remove the command for first pass (pass_cmds[0])
        self.pass_cmds = self.pass_cmds[1:]

    def to_dict(self):
        return {
            'index': self.index,
            'ffmpeg_gen_cmd': self.ffmpeg_gen_cmd,
            'size': self.size,
            'pass_cmds': self.pass_cmds,
            'frames': self.frames,
            'output_ext': self.output_ext,
        }

    @property
    def output_path(self) -> Path:
        return (self.temp / 'encode') / f'{self.name}.{self.output_ext}'

    @property
    def output(self) -> str:
        return self.output_path.as_posix()

    @property
    def fpf(self) -> str:
        fpf_file = (self.temp / 'split') / f'{self.name}_fpf'
        return fpf_file.as_posix()

    @property
    def name(self) -> str:
        return str(self.index).zfill(5)

    @staticmethod
    def create_from_dict(d: dict, temp):
        chunk = Chunk(d['index'], d['ffmpeg_gen_cmd'], d['size'], temp, d['frames'], d['output_ext'])
        chunk.pass_cmds = d['pass_cmds']
        return chunk
