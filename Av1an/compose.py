#!/bin/env python

import os
from pathlib import Path
from typing import List

from .arg_parse import Args
from .chunk import Chunk
from .vvc import get_yuv_file_path


def gen_pass_commands(args: Args, chunk: Chunk) -> List[str]:
    """
    Generates commands for ffmpeg and the encoder specified in args for the given chunk

    :param args: the Args
    :param chunk: the Chunk
    :return: A list of commands
    """
    # params needs to be set with at least get_default_params_for_encoder before this func
    assert args.video_params is not None

    # TODO: rav1e 2 pass is broken
    if args.encoder == 'rav1e' and args.passes == 2:
        print("Implicitly changing 2 pass rav1e to 1 pass\n2 pass Rav1e doesn't work")
        args.passes = 1

    encoder_funcs = {
        'svt_av1': compose_svt_av1,
        'rav1e': compose_rav1e,
        'aom': compose_aom,
        'vpx': compose_vpx,
        'x265': compose_x265,
        'x264': compose_x264,
        'vvc': compose_vvc,
    }

    return encoder_funcs[args.encoder](args, chunk)


def get_default_params_for_encoder(enc):
    """
    Gets the default params for an encoder or terminates the program if the encoder is svt_av1 as
    svt_av1 needs -w -h -fps args to function.

    :param enc: The encoder choice from arg_parse
    :return: The default params for the encoder. Terminates if enc is svt_av1
    """

    default_enc_params = {
        'vpx': '--codec=vp9 --threads=4 --cpu-used=0 --end-usage=q --cq-level=30',
        'aom': '--threads=4 --cpu-used=6 --end-usage=q --cq-level=30',
        'rav1e': ' --tiles 8 --speed 6 --quantizer 100 ',
        'svt_av1': ' --preset 4 --rc 0 --qp 25 ',
        'x265': ' -p slow --crf 23 ',
        'x264': ' --preset slow --crf 23 ',
        'vvc': ' -wdt 640 -hgt 360 -fr 23.98 -q 30 ',
    }

    return default_enc_params[enc]


def get_file_extension_for_encoder(enc):
    """
    Gets the file extension for the output of an encoder

    :param enc: The encoder name
    :return: The extension. ex: 'ivf' or 'mkv'
    """

    enc_file_extensions = {
        'vpx': 'ivf',
        'aom': 'ivf',
        'rav1e': 'ivf',
        'svt_av1': 'ivf',
        'x265': 'mkv',
        'x264': 'mkv',
        'vvc': 'h266',
    }

    return enc_file_extensions[enc]


def compose_ffmpeg_pipe(a: Args) -> str:
    """
    Gets the ffmpeg command with filters that pipes into the encoder

    :param a: the Args
    :return: the ffmpeg command as a string
    """
    return f'ffmpeg -y -hide_banner -loglevel error -i - {a.ffmpeg} {a.pix_format} -bufsize 50000K -f yuv4mpegpipe -'


def compose_svt_av1(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and svt-av1 command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | SvtAv1EncApp -i stdin {a.video_params} -b {c.output} -',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | SvtAv1EncApp -i stdin {a.video_params} -output-stat-file {c.fpf}.stat -b {os.devnull} -',
            f'{compose_ffmpeg_pipe(a)} | SvtAv1EncApp -i stdin {a.video_params} -input-stat-file {c.fpf}.stat -b {c.output} -',
        ]


def compose_rav1e(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and rav1e command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    assert a.passes == 1  # TODO: rav1e 2 pass is broken
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | rav1e - {a.video_params} --output {c.output}',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | rav1e - --first-pass {c.fpf}.stat {a.video_params} --output {os.devnull}',
            f'{compose_ffmpeg_pipe(a)} | rav1e - --second-pass {c.fpf}.stat {a.video_params} --output {c.output}',
        ]


def compose_aom(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and libaom command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | aomenc --passes=1 {a.video_params} -o {c.output} -',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | aomenc --passes=2 --pass=1 {a.video_params} --fpf={c.fpf}.log -o {os.devnull} -',
            f'{compose_ffmpeg_pipe(a)} | aomenc --passes=2 --pass=2 {a.video_params} --fpf={c.fpf}.log -o {c.output} -',
        ]


def compose_vpx(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and vpx command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | vpxenc --passes=1 {a.video_params} -o {c.output} -',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | vpxenc --passes=2 --pass=1 {a.video_params} --fpf={c.fpf} -o {os.devnull} -',
            f'{compose_ffmpeg_pipe(a)} | vpxenc --passes=2 --pass=2 {a.video_params} --fpf={c.fpf} -o {c.output} -',
        ]


def compose_x265(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and x265 command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | x265 --y4m {a.video_params} - -o {c.output}',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | x265 --log-level error --pass 1 --y4m {a.video_params} --stats {c.fpf}.log - -o {os.devnull}',
            f'{compose_ffmpeg_pipe(a)} | x265 --log-level error --pass 2 --y4m {a.video_params} --stats {c.fpf}.log - -o {c.output}',
        ]


def compose_x264(a: Args, c: Chunk) -> List[str]:
    """
    Composes the ffmpeg and x264 command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    if a.passes == 1:
        return [
            f'{compose_ffmpeg_pipe(a)} | x264 --stitchable --log-level error --demuxer y4m {a.video_params} - -o {c.output}',
        ]
    elif a.passes == 2:
        return [
            f'{compose_ffmpeg_pipe(a)} | x264 --stitchable --log-level error --pass 1 --demuxer y4m {a.video_params} - --stats {c.fpf}.log - -o {os.devnull}',
            f'{compose_ffmpeg_pipe(a)} | x264 --stitchable --log-level error --pass 2 --demuxer y4m {a.video_params} - --stats {c.fpf}.log - -o {c.output}',
        ]


def compose_vvc(a: Args, c: Chunk) -> List[str]:
    """
    Composes the vvc command(s) for the chunk with respect to args

    :param a: the Args
    :param c: the Chunk
    :return: A list of commands
    """
    yuv_file = get_yuv_file_path(c).as_posix()
    return [
        f'vvc_encoder -c {a.vvc_conf} -i {yuv_file} {a.video_params} -f {c.frames} --InputBitDepth=10 --OutputBitDepth=10 -b {c.output}',
    ]


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
