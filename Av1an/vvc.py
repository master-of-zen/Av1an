#! /bin/env python

import subprocess
from pathlib import Path
from .logger import log

def concat(inputs_list: list, output: Path):
    bitstreams = [x.as_posix() for x in inputs_list]
    bitstreams = ' '.join(bitstreams)
    cmd = f'parcatStatic  {bitstreams} {output.as_posix()}'

    output = subprocess.run(cmd, shell=True)
    er = output.stderr.strip()
    out = output.stdout.strip()

    if len(er) > 1:
        print(er)

    log(out)
    log(er)