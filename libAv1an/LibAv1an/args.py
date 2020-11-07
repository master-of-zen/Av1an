#!/bin/env python
from pathlib import Path

from .commandtypes import Command


class Args(object):

    # noinspection PyTypeChecker
    def __init__(self, initial_data):
        # Input/Output/Temp
        self.input: Path = None
        self.temp: Path = None
        self.output_file: Path = None
        self.mkvmerge: bool = None

        # Splitting
        self.chunk_method: str = None
        self.scenes: Path = None
        self.split_method: str = None
        self.extra_split: int = None
        self.min_scene_len: int = None

        # PySceneDetect split
        self.threshold: float = None

        # AOM Keyframe split
        self.reuse_first_pass: bool = None

        # Encoding
        self.passes = None
        self.video_params: Command = None
        self.encoder: str = None
        self.workers: int = None
        self.config = None

        # FFmpeg params
        self.ffmpeg_pipe: Command = None
        self.ffmpeg: str = None
        self.audio_params = None
        self.pix_format: Command = None

        # Misc
        self.logging = None
        self.resume: bool = None
        self.no_check: bool = None
        self.keep: bool = None
        self.force: bool = None

        # Vmaf
        self.vmaf: bool = None
        self.vmaf_path: str = None
        self.vmaf_res: str = None

        # Target Vmaf
        self.vmaf_target: int = None
        self.vmaf_steps: int = None
        self.min_q: int = None
        self.max_q: int = None
        self.vmaf_plots: bool = None
        self.vmaf_rate: int = None
        self.n_threads: int = None
        self.vmaf_filter: str = None

        # VVC
        self.vvc_conf: Path = None
        self.video_dimensions = (None, None)
        self.video_framerate = None

        # Inner
        self.counter = None

        # Vapoursynth
        self.is_vs: bool = None

        for key in initial_data:
            setattr(self, key, initial_data[key])
