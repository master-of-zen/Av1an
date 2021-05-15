import re
from subprocess import PIPE
from pathlib import Path
from shlex import split
from subprocess import run, Popen

VS_EXTENSIONS = [".vpy", ".py"]


def is_vapoursynth(path: Path):
    return path.suffix in VS_EXTENSIONS


def frame_probe_vspipe(source: Path):
    """
    Get frame count from vspipe.
    :param: source: Path to input vapoursynth (vpy/py) file
    """
    cmd = ["vspipe", "-i", source.as_posix(), "-"]
    r = run(cmd, capture_output=True)
    matches = re.findall(
        r"Frames:\s*([0-9]+)\s", r.stderr.decode("utf-8") + r.stdout.decode("utf-8")
    )
    frames = int(matches[-1])
    return frames


def create_vs_file(temp: Path, source, chunk_method):
    """
    Creates vs pipe file or returns file if it exists
    """

    load_script = temp / "split" / "loadscript.vpy"

    if load_script.exists():
        return load_script

    if chunk_method == "vs_ffms2":
        cache_file = (temp / "split" / "cache.ffindex").resolve()
        script = (
            "from vapoursynth import core\n"
            'core.ffms2.Source(r"{}", cachefile="{}").set_output()'
        )
    else:
        cache_file = (temp / "split" / "cache.lwi").resolve().as_posix()
        script = (
            "from vapoursynth import core\n"
            'core.lsmas.LWLibavSource(r"{}", cachefile="{}").set_output()'
        )

    with open(load_script, "w+", encoding='utf-8') as file:
        file.write(script.format(Path(source).resolve(), cache_file))

    cache_generation = f"vspipe -i {load_script.as_posix()} -i -"
    d = Popen(split(cache_generation), stdout=PIPE, stderr=PIPE).wait()

    return load_script


def compose_vapoursynth_pipe(source: Path, fifo: Path = None):
    return ["vspipe", "-y", source.as_posix(), fifo.as_posix() if fifo else "-"]
