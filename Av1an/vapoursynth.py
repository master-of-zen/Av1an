import re
import subprocess
from subprocess import PIPE 
from pathlib import Path

VS_EXTENSIONS = ['.vpy', '.py']


def is_vapoursynth(path: Path):
    return path.suffix in VS_EXTENSIONS


def frame_probe_vspipe(source: Path):
    """
    Get frame count from vspipe.
    :param: source: Path to input vapoursynth (vpy/py) file
    """
    cmd = ["vspipe", "--info", "-i", source.as_posix(), "-"]
    r = subprocess.run(cmd, stdout=PIPE, stderr=PIPE)
    # TODO: This regex process is overbuilt, as the output of vspipe is very simple and only has one "Frames" result.
    matches = re.findall(r"Frames:\s*([0-9]+)\s", r.stderr.decode("utf-8") + r.stdout.decode("utf-8"))
    return int(matches[-1])


def compose_vapoursynth_pipe(source: Path, fifo: Path = None):
    return ["vspipe", "-y", source.as_posix(), fifo.as_posix() if fifo else "-"]
