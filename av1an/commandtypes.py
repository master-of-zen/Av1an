import os
from typing import Union, List, NamedTuple


Command = List[Union[str, os.PathLike]]
"""
A command as a list of strings or paths. Can be passed to subprocess.Popen
"""


class CommandPair(NamedTuple):
    """
    A pair of commands, the ffmpeg filter and then the encoder command
    """
    ffmpeg_cmd: Command
    encode_cmd: Command


MPCommands = List[CommandPair]
"""
Multi-pass commands type

self[pass][0] gets the ffmpeg filter command
self[pass][1] gets the encoder command
"""
