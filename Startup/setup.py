#!/bin/env python

import atexit
import os
import shlex
import shutil
import sys
from distutils.spawn import find_executable
from pathlib import Path

from psutil import virtual_memory
from Startup.validate_commands import validate_inputs
from Encoders import ENCODERS
from Projects import Project
from Av1an.utils import terminate, hash_path
from Av1an.logger import log
from Av1an.vapoursynth import is_vapoursynth


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


def select_best_chunking_method(project: Project):

    if not find_executable('vspipe'):
        project.chunk_method = 'hybrid'
        log('Set Chunking Method: Hybrid')
    else:
        try:
            import vapoursynth
            plugins = vapoursynth.get_core().get_plugins()

            if 'com.vapoursynth.ffms2' in plugins:
                log('Set Chunking Method: FFMS2\n')
                project.chunk_method = 'vs_ffms2'

            elif 'systems.innocent.lsmas' in plugins:
                log('Set Chunking Method: L-SMASH\n')
                project.chunk_method = 'vs_lsmash'

        except:
            log('Vapoursynth not installed but vspipe reachable\n' +
                'Fallback to Hybrid\n')
            project.chunk_method = 'hybrid'


def check_exes(project: Project):
    """
    Checking required executables

    :param project: the Project
    """

    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate()

    if project.chunk_method in ['vs_ffms2', 'vs_lsmash']:
        if not find_executable('vspipe'):
            print('vspipe executable not found')
            terminate()

        try:
            import vapoursynth
            plugins = vapoursynth.get_core().get_plugins()

            if project.chunk_method == 'vs_lsmash' and "systems.innocent.lsmas" not in plugins:
                print('lsmas is not installed')
                terminate()

            if project.chunk_method == 'vs_ffms2' and "com.vapoursynth.ffms2" not in plugins:
                print('ffms2 is not installed')
                terminate()
        except ModuleNotFoundError:
            print('Vapoursynth is not installed')
            terminate()


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
        select_best_chunking_method(project)

    # project.is_vs = is_vapoursynth(project.input)

    if project.is_vs:
        project.chunk_method = 'vs_ffms2'

    check_exes(project)

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
    project.ffmpeg_pipe = [*project.ffmpeg, *project.pix_format,
                        '-bufsize', '50000K', '-f', 'yuv4mpegpipe', '-']


def determine_resources(encoder, workers):
    """Returns number of workers that machine can handle with selected encoder."""

    # If set by user, skip
    if workers != 0:
        return workers

    cpu = os.cpu_count()
    ram = round(virtual_memory().total / 2 ** 30)

    if encoder in ('aom', 'rav1e', 'vpx'):
        workers = round(min(cpu / 3, ram / 1.5))

    elif encoder in ('svt_av1', 'svt_vp9', 'x265', 'x264'):
        workers = round(min(cpu, ram)) // 8

    elif encoder in 'vvc':
        workers = round(min(cpu, ram)) // 4

    # fix if workers round up to 0
    if workers == 0:
        workers = 1

    return workers


def setup(project):
    """Creating temporally folders when needed."""

    if project.temp:
        project.temp = Path(str(project.temp))
    else:
        project.temp = Path('.' + str(hash_path(str(project.input))))

    # Checking is resume possible
    done_path = project.temp / 'done.json'
    project.resume = project.resume and done_path.exists()

    if not project.resume:
        if project.temp.is_dir():
            shutil.rmtree(project.temp)

    (project.temp / 'split').mkdir(parents=True, exist_ok=True)
    (project.temp / 'encode').mkdir(exist_ok=True)



