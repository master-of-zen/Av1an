#!/bin/env python

import atexit
import os
import shlex
import shutil
import sys

from pathlib import Path

from av1an.startup.validate_commands import validate_inputs
from av1an.encoder import ENCODERS
from av1an.project import Project
from av1an.utils import terminate, hash_path
from av1an.logger import log
from av1an.vapoursynth import is_vapoursynth


def set_target_quality(project):
    """
    Av1an setup for target_quality

    :param project: the Project
    """
    if project.vmaf_path:
        if not Path(project.vmaf_path).exists():
            print(f"No model with this path: {Path(project.vmaf_path).as_posix()}")
            terminate()

    if project.probes < 4:
        print('Target quality with less than 4 probes is experimental and not recommended')
        terminate()

    encoder = ENCODERS[project.encoder]

    if project.encoder not in ('x265', 'svt_av1') and project.target_quality_method == 'per_frame':
        print(f":: Per frame Target Quality is not supported for selected encoder\n:: Supported encoders: x265, svt_av1")
        exit()

    # setting range for q values
    if project.min_q is None:
        project.min_q, _ = encoder.default_q_range
        assert project.min_q > 1

    if project.max_q is None:
        _, project.max_q = encoder.default_q_range


def setup_encoder(project: Project):
    """
    Setup encoder params and passes
    :param project: the Project
    """
    encoder = ENCODERS[project.encoder]

    # validate encoder settings
    settings_valid, error_msg = encoder.is_valid(project)
    if not settings_valid:
        print(error_msg)
        terminate()

    if project.passes is None:
        project.passes = encoder.default_passes

    project.video_params = encoder.default_args if project.video_params is None \
        else shlex.split(project.video_params)

    validate_inputs(project)


def startup_check(project: Project):
    """
    Performing essential checks at startup_check
    Set constant values
    """
    if sys.version_info < (3, 6):
        print('Python 3.6+ required')
        sys.exit()
    if sys.platform == 'linux':
        def restore_term():
            os.system("stty sane")

        atexit.register(restore_term)

    if not project.chunk_method:
        project.select_best_chunking_method()

    # project.is_vs = is_vapoursynth(project.input)

    if project.is_vs:
        project.chunk_method = 'vs_ffms2'

    project.check_exes()

    set_target_quality(project)

    if project.reuse_first_pass and project.encoder != 'aom' and project.split_method != 'aom_keyframes':
        print('Reusing the first pass is only supported with \
              the aom encoder and aom_keyframes split method.')
        terminate()

    setup_encoder(project)

    # No check because vvc
    if project.encoder == 'vvc':
        project.no_check = True

    if project.encoder == 'svt_vp9' and project.passes == 2:
        print("Implicitly changing 2 pass svt-vp9 to 1 pass\n2 pass svt-vp9 isn't supported")
        project.passes = 1

    project.audio_params = shlex.split(project.audio_params)
    project.ffmpeg = shlex.split(project.ffmpeg)

    project.pix_format = ['-strict', '-1', '-pix_fmt', project.pix_format]
    project.ffmpeg_pipe = [*project.ffmpeg, *project.pix_format,'-color_range', '0', '-f', 'yuv4mpegpipe', '-']
