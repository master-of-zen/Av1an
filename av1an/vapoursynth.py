from pathlib import Path


def compose_vapoursynth_pipe(source: Path, fifo: Path = None):
    return ["vspipe", "-y", source.as_posix(), fifo.as_posix() if fifo else "-"]
