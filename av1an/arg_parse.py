#!/bin/env python
import argparse
import sys
from pathlib import Path
from .project import Project

class Args:
    """
    Class responsible for arg parsing
    Creation of original project file
    """
    def __init__(self):
        self.parser = self.arg_parsing()
        self.defaults = self.get_defaults()
        self.parsed = None
        self.project = None

    def get_project(self):
        """
        Create and return project object with all parameters
        """
        if not self.parsed:
            self.parse()

        self.project = Project(self.parsed)

        if self.project.config:
            self.save_load_project_file()

        return self.project

    def get_defaults(self) -> dict:
        """
        Get dictionary of default values specified in arg_parsing()
        """
        return vars(self.parser.parse_args([]))

    def get_difference(self) -> dict:
        """
        Return difference of defaults and new
        """
        return dict([x for x in self.parsed.items() if x not in self.defaults.items()])

    def parse(self):
        """
        Parse command line parameters provided by user
        """
        self.parsed = vars(self.parser.parse_args())
        if not self.parsed['input']:
            self.parser.print_help()
            sys.exit()

    def save_load_project_file(self):
        """
        Saves current/Loads given project file, loads saved project first and when overwrites only unique values from current parse
        """
        cfg_path = Path(self.project.config)

        if cfg_path.exists():

            new = self.get_difference()
            self.project.load_project_from_file(self.project.config)
            self.project.load_project(new)

        else:
            self.project.save_project_to_file(self.project.config)

    def arg_parsing(self):
        """Command line parsing and setting default variables"""
        parser = argparse.ArgumentParser()

        # Input/Output/Temp
        io_group = parser.add_argument_group('Input and Output')
        io_group.add_argument('--input', '-i', nargs='+', type=Path, help='Input File')
        io_group.add_argument('--temp', type=Path, default=None, help='Set temp folder path')
        io_group.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')
        io_group.add_argument('--mkvmerge', help='Use mkvmerge instead of ffmpeg to concatenate', action='store_true')

        io_group.add_argument('--quiet', '-q', help='Disable printing tqdm and scenedetect data to terminal', action='store_true')
        io_group.add_argument('--logging', '-log', type=str, default=None, help='Enable logging')
        io_group.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
        io_group.add_argument('--keep', help='Keep temporally folder after encode', action='store_true')
        io_group.add_argument('--config', '-c', type=str, default=None, help="Path to config file, create if doesn't exists")
        io_group.add_argument('--webm', default=False, action='store_true', help="Output to webm")

        # Splitting
        split_group = parser.add_argument_group('Splitting')
        split_group.add_argument('--chunk_method', '-cm', type=str, default=None, help='Method for creating chunks',
                                 choices=['select', 'vs_ffms2', 'vs_lsmash', 'hybrid'])
        split_group.add_argument('--scenes', '-s', type=str, default=None, help='File location for scenes')
        split_group.add_argument('--split_method', type=str, default='pyscene', help='Specify splitting method',
                                 choices=['none', 'pyscene', 'aom_keyframes', 'ffmpeg'])
        split_group.add_argument('--extra_split', '-xs', type=int, default=240,
                                 help='Number of frames after which make split')

        # PySceneDetect split
        split_group.add_argument('--threshold', '-tr', type=float, default=35, help='PySceneDetect Threshold')
        split_group.add_argument('--min_scene_len', type=int, default=60, help='Minimum number of frames in a split')

        # AOM Keyframe split
        split_group.add_argument('--reuse_first_pass', help='Reuse the first pass from aom_keyframes split on the chunks',
                                 action='store_true')

        # Encoding
        encode_group = parser.add_argument_group('Encoding')
        encode_group.add_argument('--passes', '-p', type=int, default=None, help='Specify encoding passes', choices=[1, 2])
        encode_group.add_argument('--video_params', '-v', type=str, default=None, help='encoding settings')
        encode_group.add_argument('--encoder', '-enc', type=str, default='aom', help='Choosing encoder',
                                  choices=['aom', 'svt_av1', 'svt_vp9', 'rav1e', 'vpx', 'x265', 'x264', 'vvc'])
        encode_group.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
        encode_group.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')
        encode_group.add_argument('--force', help="Force encoding if input args seen as invalid", action='store_true')

        # VVC
        encode_group.add_argument('--vvc_conf', type=Path, default=None, help='Path to VVC confing file')

        # FFmpeg params
        ffmpeg_group = parser.add_argument_group('FFmpeg')
        ffmpeg_group.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
        ffmpeg_group.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
        ffmpeg_group.add_argument('--pix_format', '-fmt', type=str, default='yuv420p10le', help='FFmpeg pixel format')

        # Vmaf
        vmaf_group = parser.add_argument_group('VMAF')
        vmaf_group.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        vmaf_group.add_argument('--vmaf_path', type=Path, default=None, help='Path to vmaf models')
        vmaf_group.add_argument('--vmaf_res', type=str, default="1920x1080", help='Resolution used in vmaf calculation')
        vmaf_group.add_argument('--n_threads', type=int, default=None, help='Threads for vmaf calculation')

        # Target Quality
        tq_group = parser.add_argument_group('Target Quality')
        tq_group.add_argument('--target_quality', type=float, help='Value to target')
        tq_group.add_argument('--target_quality_method', type=str, default='per_shot',
                              help='Method selection for target quality', choices=['per_frame', 'per_shot'])
        tq_group.add_argument('--probes', type=int, default=4, help='Number of probes to make for target_quality')
        tq_group.add_argument('--min_q', type=int, default=None, help='Min q for target_quality')
        tq_group.add_argument('--max_q', type=int, default=None, help='Max q for target_quality')
        tq_group.add_argument('--vmaf_plots', help='Make plots of probes in temp folder', action='store_true')
        tq_group.add_argument('--probing_rate', type=int, default=4, help='Framerate for probes, 0 - original')
        tq_group.add_argument('--vmaf_filter', type=str, default=None,
                              help='Filter applied to source at vmaf calcualation, use if you crop source')

        # Misc
        misc_group = parser.add_argument_group('Misc')
        misc_group.add_argument('--version', action='version', version=f'Av1an version: 5.6')
        # Initialize project with initial values
        return parser
