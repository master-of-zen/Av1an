import re
from pathlib import Path
from subprocess import run

VS_EXTENSIONS = [".vpy", ".py"]


def is_vapoursynth(path: Path):
    return path.suffix in VS_EXTENSIONS


def compose_vapoursynth_pipe(source: Path, fifo: Path = None):
    return ["vspipe", "-y", source.as_posix(), fifo.as_posix() if fifo else "-"]
