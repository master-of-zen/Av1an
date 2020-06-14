#!/bin/env python

from utils.utils import terminate
import os

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