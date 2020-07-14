#!/bin/env python

import json
import os
import sys
from pathlib import Path

from utils.utils import terminate

from .logger import log


def compose_aomsplit_first_pass_command(video_path: Path, stat_file, ffmpeg_pipe, video_params):
    """
    Generates the command for the first pass of the entire video used for aom keyframe split

    :param video_path: the video path
    :param stat_file: the stat_file output
    :param ffmpeg_pipe: the av1an.ffmpeg_pipe with pix_fmt and -ff option
    :param video_params: the video params for aomenc first pass
    :return: ffmpeg, encode
    """

    ffmpeg_pipe = ffmpeg_pipe[:-2]  # remove the ' |' at the end

    f = f'ffmpeg -y -hide_banner -loglevel error -i {video_path.as_posix()} {ffmpeg_pipe}'
    # removed -w -h from aomenc since ffmpeg filters can change it and it can be added into video_params
    # TODO(n9Mtq4): if an encoder other than aom is being used, video_params becomes the default so -w -h may be needed again
    e = f'aomenc --passes=2 --pass=1 {video_params} --fpf={stat_file.as_posix()} -o {os.devnull} -'

    return f, e


def get_default_params_for_encoder(enc):
    """
    Gets the default params for an encoder or terminates the program if the encoder is svt_av1 as
    svt_av1 needs -w -h -fps args to function.

    :param enc: The encoder choice from arg_parse
    :return: The default params for the encoder. Terminates if enc is svt_av1
    """

    DEFAULT_ENC_PARAMS = {
    'vpx': '--codec=vp9 --threads=4 --cpu-used=0 --end-usage=q --cq-level=30',
    'aom': '--threads=4 --cpu-used=6 --end-usage=q --cq-level=30',
    'rav1e': ' --tiles 8 --speed 6 --quantizer 100',
    'svt_av1': ' --preset 4 --rc 0 --qp 25 ',
    'x265': ' -p slow --crf 23 --vbv-maxrate 10000 --vbv-bufsize 8000 ',
    }

    return DEFAULT_ENC_PARAMS[enc]


def svt_av1_encode(inputs, passes, pipe, params):
    """
    Generates commands for SVT-AV1

    :param inputs: Files that need to be enocoded
    :param passes: Encoding passes
    :param pipe: FFmpeg piping settings
    :param params: Encoding parameters
    :return: Composed commands for execution
    """
    encoder = 'SvtAv1EncApp'
    commands = []

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
    """
    Generates commands for AOM, VPX encoders

    :param inputs: Files that need to be enocoded
    :param passes: Encoding passes
    :param pipe: FFmpeg piping settings
    :param params: Encoding parameters
    :return: Composed commands for execution
    """
    single_p = f'{enc} --passes=1 '
    two_p_1 = f'{enc} --passes=2 --pass=1'
    two_p_2 = f'{enc} --passes=2 --pass=2'

    if passes == 1:
        return [
            (f'-i {file[0]} {pipe} {single_p} {params} -o {file[1].with_suffix(".ivf")} - ',
             (file[0], file[1].with_suffix('.ivf')))
            for file in inputs]

    if passes == 2:
        return [
            (f'-i {file[0]} {pipe} {two_p_1} {params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
             f'-i {file[0]} {pipe} {two_p_2} {params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
             (file[0], file[1].with_suffix('.ivf')))
            for file in inputs]

    return []


def rav1e_encode(inputs, passes, pipe, params):
    """
    Generates commands for Rav1e encoder
    Currently 2 pass rav1e piping doesn't work

    :param inputs: Files that need to be enocoded
    :param passes: Encoding passes
    :param pipe: FFmpeg piping settings
    :param params: Encoding parameters
    :return: Composed commands for execution
    """
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
         f' rav1e - --second-pass {file[0].with_suffix(".stat")} {pa65 [info]: Residual QT: max TU size, max depth : 32 / 1 inter / 1 intra
265
rams} '
         f'--output {file[1].with_suffix(".ivf")}',
         (file[0], file[1].with_suffix('.ivf')))
         for file in inputs]
    """
    return commands


def x265_encode(inputs, passes, pipe, params):
    """
    Generates commands for AOM, VPX encoders

    :param inputs: Files that need to be enocoded
    :param passes: Encoding passes
    :param pipe: FFmpeg piping settings
    :param params: Encoding parameters
    :return: Composed commands for execution
    """
    commands = []
    single_p = 'x265 --y4m'
    two_p_1 = 'x265 --pass 1 --y4m'
    two_p_2 = 'x265 --pass 2 --y4m'

    if passes == 1:
        commands = [
                (f' -i {file[0]} {pipe} {single_p} {params} - -o {file[1].with_suffix(".ivf")}',
                (file[0], file[1].with_suffix('.ivf')))
                for file in inputs
                ]

    if passes == 2:
        commands = [
            (f' -i {file[0]} {pipe} {two_p_1} {params} --stats {file[0].with_suffix(".log")} - -o {os.devnull}',
             f' -i {file[0]} {pipe} {two_p_2} {params} --stats {file[0].with_suffix(".log")} - -o {file[1].with_suffix(".ivf")}',
             (file[0], file[1].with_suffix('.ivf')))
             for file in inputs
        ]

    return commands

def compose_encoding_queue(files, temp, encoder, params, pipe, passes):
    """
    Composing encoding queue with split videos.
    :param files: List of files that need to be encoded
    :param temp: Path of temp folder
    :param encoder: Name of encoder to compose for
    :param params: Encoding parameters
    :param pipe: FFmpeg pipe
    :passes: Number of passes
    """

    assert params is not None  # params needs to be set with at least get_default_params_for_encoder before this func

    encoders = {'svt_av1': 'SvtAv1EncApp', 'rav1e': 'rav1e', 'aom': 'aomenc', 'vpx': 'vpxenc', 'x265': 'x265'}
    enc_exe = encoders.get(encoder)
    inputs = [(temp / "split" / file.name,
               temp / "encode" / file.name,
               file) for file in files]

    if encoder in ('aom', 'vpx'):
        queue = aom_vpx_encode(inputs, enc_exe, passes, pipe, params)

    elif encoder == 'rav1e':
        queue = rav1e_encode(inputs, passes, pipe, params)

    elif encoder == 'svt_av1':
        queue = svt_av1_encode(inputs, passes, pipe, params)

    elif encoder == 'x265':
        queue = x265_encode(inputs, passes, pipe, params)

    # Catch Error
    if len(queue) == 0:
        er = 'Error in making command queue'
        log(er)
        terminate()
    return queue


def get_video_queue(temp: Path, resume):
    """
    Compose and returns sorted list of all files that need to be encoded. Big first.

    :param temp: Path to temp folder
    :param resume: Flag on should already encoded chunks be discarded from queue
    :return: Sorted big first list of chunks to encode
    """
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
        er = 'Error: No files found in .temp/split, probably splitting not working'
        print(er)
        log(er)
        terminate()

    return queue
