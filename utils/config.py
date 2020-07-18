#!/usr/bin/env python3
import json
from pathlib import Path
from .utils import terminate
from .compose import get_default_params_for_encoder

def conf(args):
    """Creation and reading of config files with saved settings"""
    if args.config:
        if args.config.exists():
            with open(args.config) as f:
                c: dict = dict(json.load(f))
                args.__dict__.update(c)

        else:
            with open(args.config, 'w') as f:
                c = dict()
                c['video_params'] = args.video_params
                c['encoder'] = args.encoder
                c['ffmpeg'] = args.ffmpeg
                c['audio_params'] = args.audio_params
                json.dump(c, f)

    # Changing pixel format, bit format
    args.pix_format = f'-strict -1 -pix_fmt {args.pix_format}'
    args.ffmpeg_pipe = f' {args.ffmpeg} {args.pix_format} -f yuv4mpegpipe - |'

    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate()

    if args.reuse_first_pass and args.encoder != 'aom' and args.split_method != 'aom_keyframes':
        print('Reusing the first pass is only supported with the aom encoder and aom_keyframes split method.')
        terminate()

    if args.video_params is None:
        args.video_params = get_default_params_for_encoder(args.encoder)