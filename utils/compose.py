#!/bin/env python

from utils.utils import terminate
import os
import sys
import json
from pathlib import Path
from .logger import log, set_log_file

def svt_av1_encode(inputs, passes, pipe, params):
        """SVT-AV1 encoding command composition."""
        encoder = 'SvtAv1EncApp'
        commands = []
        if not params:
            print('-w -h -fps is required parameters for svt_av1 encoder')
            terminate()

        if passes == 1:
            commands = [
                (f'-i {file[0]} {pipe} ' +
                 f'  {encoder} -i stdin {params} -b {file[1].with_suffix(".ivf")} -',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        if passes == 2:
            p2i = '-input-stat-file '
            p2o = '-output-stat-file '
            commands = [
                (f'-i {file[0]} {pipe} {encoder} -i stdin {params} {p2o} '
                 f'{file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {pipe} '
                 f'{encoder} -i stdin {params} {p2i} {file[0].with_suffix(".stat")} -b '
                 f'{file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in inputs]

        return commands

def aom_vpx_encode(inputs, enc, passes, pipe, params):
    """AOM encoding command composition."""
    single_p = f'{enc} --passes=1 '
    two_p_1 = f'{enc} --passes=2 --pass=1'
    two_p_2 = f'{enc} --passes=2 --pass=2'
    commands = []

    if passes == 1:
        commands = [
            (f'-i {file[0]} {pipe} {single_p} {params} -o {file[1].with_suffix(".ivf")} - ',
             (file[0], file[1].with_suffix('.ivf')))
             for file in inputs]

    if passes == 2:
        commands = [
            (f'-i {file[0]} {pipe} {two_p_1} {params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
             f'-i {file[0]} {pipe} {two_p_2} {params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
             (file[0], file[1].with_suffix('.ivf')))
             for file in inputs]

    return commands

def rav1e_encode(inputs, passes, pipe, params):
    """Rav1e encoding command composition."""
    commands = []

    if passes == 2:
        print("Implicitly changing passes to 1\nCurrently 2 pass Rav1e doesn't work")

    if passes:
        commands = [
            (f'-i {file[0]} {pipe} '
             f' rav1e -  {params}  '
             f'--output {file[1].with_suffix(".ivf")}',
             (file[0], file[1].with_suffix('.ivf')))
             for file in inputs]

    # 2 encode pass not working with FFmpeg pipes :(
    """
    if passes == 2:
        commands = [
        (f'-i {file[0]} {pipe} '
         f' rav1e - --first-pass {file[0].with_suffix(".stat")} {params} '
         f'--output {file[1].with_suffix(".ivf")}',
         f'-i {file[0]} {pipe} '
         f' rav1e - --second-pass {file[0].with_suffix(".stat")} {params} '
         f'--output {file[1].with_suffix(".ivf")}',
         (file[0], file[1].with_suffix('.ivf')))
         for file in inputs]
    """
    return commands

def compose_encoding_queue(files, temp, encoder, params, pipe, passes):
        """Composing encoding queue with split videos."""
        encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc'}
        enc_exe = encoders.get(encoder)
        inputs = [(temp / "split" / file.name,
                   temp / "encode" / file.name,
                   file) for file in files]

        if encoder in ('aom', 'vpx'):
            if not params:
                if enc_exe == 'vpxenc':
                    params = '--codec=vp9 --threads=4 --cpu-used=0 --end-usage=q --cq-level=30'

                if enc_exe == 'aomenc':
                    params = '--threads=4 --cpu-used=6 --end-usage=q --cq-level=30'

            queue = aom_vpx_encode(inputs, enc_exe, passes, pipe, params)

        elif encoder == 'rav1e':
            if not params:
                params = ' --tiles 8 --speed 6 --quantizer 100'
            queue = rav1e_encode(inputs, passes, pipe, params)

        elif encoder == 'svt_av1':
            if not params:
                print('-w -h -fps is required parameters for svt_av1 encoder')
                terminate()
            queue = svt_av1_encode(inputs, passes, pipe, params)

        # Catch Error
        if len(queue) == 0:
            print('Error in making command queue')
            terminate()
        return queue, params


def get_video_queue(temp: Path, resume):
    """Returns sorted list of all videos that need to be encoded. Big first."""
    source_path = temp / 'split'
    queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']

    done_file = temp / 'done.json'
    if resume and done_file.exists():
        try:
            with open(done_file) as f:
                data = json.load(f)
            data = data['done'].keys()
            queue = [x for x in queue if x.name not in data]
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error at resuming {e}\nAt line {exc_tb.tb_lineno}')

    queue = sorted(queue, key=lambda x: -x.stat().st_size)

    if len(queue) == 0:
        print('Error: No files found in .temp/split, probably splitting not working')
        terminate()

    return queue