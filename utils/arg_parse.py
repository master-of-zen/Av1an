import argparse
from pathlib import Path

def arg_parsing():
        """Command line parsing"""
        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=0, help='0 - local, 1 - master, 2 - encoder')

        # Input/Output/Temp
        parser.add_argument('--input', '-i', nargs='+', type=Path, help='Input File')
        parser.add_argument('--temp', type=Path, default=Path('.temp'), help='Set temp folder path')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')

        # Splitting
        parser.add_argument('--split_method', type=str, default='pyscene', help='Specify splitting method', choices=['pyscene', 'aom_keyframes'])
        parser.add_argument('--extra_split', '-xs', type=int, default=0, help='Number of frames after which make split')
        parser.add_argument('--min_scene_len', type=int, default=None, help='Minimum number of frames in a split')

        # PySceneDetect split
        parser.add_argument('--scenes', '-s', type=str, default=None, help='File location for scenes')
        parser.add_argument('--threshold', '-tr', type=float, default=50, help='PySceneDetect Threshold')

        # Encoding
        parser.add_argument('--passes', '-p', type=int, default=2, help='Specify encoding passes')
        parser.add_argument('--video_params', '-v', type=str, default='', help='encoding settings')
        parser.add_argument('--encoder', '-enc', type=str, default='aom', help='Choosing encoder',  choices=['aom', 'svt_av1', 'rav1e', 'vpx'])
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
        parser.add_argument('--boost_range', default=15, type=int, help='Range/strength of CQ change')
        parser.add_argument('--boost_limit', default=10, type=int, help='CQ limit for boosting')

        # Vmaf
        parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        parser.add_argument('--vmaf_path', type=Path, default=None, help='Path to vmaf models')

        # Target Vmaf
        parser.add_argument('--vmaf_target', type=float, help='Value of Vmaf to target')
        parser.add_argument('--vmaf_steps', type=int, default=4, help='Steps between min and max qp for target vmaf')
        parser.add_argument('--min_cq', type=int, default=25, help='Min cq for target vmaf')
        parser.add_argument('--max_cq', type=int, default=50, help='Max cq for target vmaf')
        parser.add_argument('--vmaf_plots', help='Make plots of probes in temp folder', action='store_true')

        # Server parts
        parser.add_argument('--host', nargs='+', type=str, help='ips of encoders')

        # Store all vars in dictionary
        return vars(parser.parse_args())
