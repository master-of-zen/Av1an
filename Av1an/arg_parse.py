#!/bin/env python
import argparse
from pathlib import Path

class Args(object):

    def __init__(self, initial_data):
        # Input/Output/Temp
        self.input = None
        self.temp = None
        self.output_file = None

        # Splitting
        self.scenes = None
        self.split_method = None
        self.extra_split = None
        self.min_scene_len = None

        # PySceneDetect split
        self.threshold = None

        # AOM Keyframe split
        self.reuse_first_pass = None

        # Encoding
        self.passes = None
        self.video_params = None
        self.encoder = None
        self.workers = None
        self.config = None

        self.video_params = None

        # FFmpeg params
        self.ffmpeg_pipe = None
        self.ffmpeg = None
        self.audio_params = None
        self.pix_format = None

        # Misc
        self.logging = None
        self.resume = None
        self.no_check = None
        self.keep = None

        # Boost
        self.boost = None
        self.boost_range = None
        self.boost_limit = None

        # Vmaf
        self.vmaf = None
        self.vmaf_path = None

        # Target Vmaf
        self.vmaf_target = None
        self.vmaf_steps = None
        self.min_q = None
        self.max_q = None
        self.vmaf_plots = None
        self.n_threads = None

        # VVC
        self.vvc_conf = None
        self.video_dimensions = (None, None)
        self.video_framerate = None
        for key in initial_data:
            setattr(self, key, initial_data[key])

def arg_parsing():
    """Command line parsing"""
    parser = argparse.ArgumentParser()

    # Input/Output/Temp
    parser.add_argument('--input', '-i', nargs='+', type=Path, help='Input File')
    parser.add_argument('--temp', type=Path, default=Path('.temp'), help='Set temp folder path')
    parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')

    # Splitting
    parser.add_argument('--scenes', '-s', type=str, default=None, help='File location for scenes')
    parser.add_argument('--split_method', type=str, default='pyscene', help='Specify splitting method',
                        choices=['pyscene', 'aom_keyframes'])
    parser.add_argument('--extra_split', '-xs', type=int, default=0, help='Number of frames after which make split')
    parser.add_argument('--min_scene_len', type=int, default=None, help='Minimum number of frames in a split')

    # PySceneDetect split
    parser.add_argument('--threshold', '-tr', type=float, default=50, help='PySceneDetect Threshold')

    # AOM Keyframe split
    parser.add_argument('--reuse_first_pass', help='Reuse the first pass from aom_keyframes split on the chunks', action='store_true')

    # Encoding
    parser.add_argument('--passes', '-p', type=int, default=None, help='Specify encoding passes', choices=[1, 2])
    parser.add_argument('--video_params', '-v', type=str, default=None, help='encoding settings')
    parser.add_argument('--encoder', '-enc', type=str, default='aom', help='Choosing encoder',
                        choices=['aom', 'svt_av1', 'rav1e', 'vpx','x265', 'vvc'])
    parser.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
    parser.add_argument('-cfg', '--config', type=Path, help='Parameters file. Save/Read: '
                                                            'Video, Audio, Encoder, FFmpeg parameteres')

    # FFmpeg params
    parser.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
    parser.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
    parser.add_argument('--pix_format', '-fmt', type=str, default='yuv420p', help='FFmpeg pixel format')

    # Misc
    parser.add_argument('--logging', '-log', type=str, default=None, help='Enable logging')
    parser.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
    parser.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')
    parser.add_argument('--keep', help='Keep temporally folder after encode', action='store_true')

    # Boost
    parser.add_argument('--boost', help='Experimental feature, decrease CQ of clip based on brightness.'
                                        'Darker = lower CQ', action='store_true')
    parser.add_argument('--boost_range', '-br', default=15, type=int, help='Range/strength of CQ change')
    parser.add_argument('--boost_limit', '-bl', default=10, type=int, help='CQ limit for boosting')

    # Vmaf
    parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
    parser.add_argument('--vmaf_path', type=Path, default=None, help='Path to vmaf models')

    # Target Vmaf
    parser.add_argument('--vmaf_target', type=float, help='Value of Vmaf to target')
    parser.add_argument('--vmaf_steps', type=int, default=5, help='Steps between min and max qp for target vmaf')
    parser.add_argument('--min_q', type=int, default=None, help='Min q for target vmaf')
    parser.add_argument('--max_q', type=int, default=None, help='Max q for target vmaf')
    parser.add_argument('--vmaf_plots', help='Make plots of probes in temp folder', action='store_true')
    parser.add_argument('--n_threads', type=int, default=None, help='Threads for vmaf calculation')

    # VVC
    parser.add_argument('--vvc_conf', type=Path, default=None, help='Path to VVC confing file')

    return Args(vars(parser.parse_args()))
