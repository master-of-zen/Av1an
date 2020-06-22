
import os
import sys
from psutil import virtual_memory
import shutil
from pathlib import Path
from .logger import log, set_log_file, set_logging
from distutils.spawn import find_executable
from .utils import terminate


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        return round(min(cpu / 2, ram / 1.5))

    elif encoder == 'svt_av1':
        return round(min(cpu, ram)) // 5

    # fix if workers round up to 0
    if workers == 0:
        return 1


def check_executables(encoder):
    encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc'}
    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()

    # Check if encoder executable is reachable
    if encoder in encoders:
        enc = encoders.get(encoder)

        if not find_executable(enc):
            print(f'Encoder {enc} not found')
            terminate()
    else:
        print(f'Not valid encoder {encoder}\nValid encoders: "aom rav1e", "svt_av1", "vpx" ')
        terminate()


def setup(temp: Path, resume):
    """Creating temporally folders when needed."""
    # Make temporal directories, and remove them if already presented
    if not resume:
        if temp.is_dir():
            shutil.rmtree(temp)

    (temp / 'split').mkdir(parents=True, exist_ok=True)
    (temp / 'encode').mkdir(exist_ok=True)

def outputs_filenames(inp: Path, out:Path):
    if out:
        return out.with_suffix('.mkv')
    else:
        return Path(f'{inp.stem}_av1.mkv')

