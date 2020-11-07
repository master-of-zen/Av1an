import atexit
import shlex
import sys
import os

from Av1an.handle_callbacks import terminate
from pathlib import Path
from libAv1an.Encoders import ENCODERS
from distutils.spawn import find_executable
from libAv1an.LibAv1an.args import Args


def set_vmaf(args):
    """
    Av1an setup for VMAF

    :param args: the Args
    """
    if args.vmaf_path:
        if not Path(args.vmaf_path).exists():
            print(f'No such model: {Path(args.vmaf_path).as_posix()}')
            terminate(1)

    if args.vmaf_steps < 4:
        print('Target vmaf require more than 3 probes/steps')
        terminate(1)

    encoder = ENCODERS[args.encoder]

    if args.min_q is None:
        args.min_q, _ = encoder.default_q_range
    if args.max_q is None:
        _, args.max_q = encoder.default_q_range


def check_exes(args: Args):
    """
    Checking required executables

    :param args: the Args
    """

    if not find_executable('ffmpeg'):
        print('No ffmpeg')
        terminate(1)

    if args.chunk_method in ['vs_ffms2', 'vs_lsmash']:
        if not find_executable('vspipe'):
            print('vspipe executable not found')
            terminate(1)

        try:
            import vapoursynth
            plugins = vapoursynth.get_core().get_plugins()
        except ModuleNotFoundError:
            print('Vapoursynth is not installed')
            terminate(1)

        if args.chunk_method == 'vs_lsmash' and "systems.innocent.lsmas" not in plugins:
            print('lsmas is not installed')
            terminate(1)

        if args.chunk_method == 'vs_ffms2' and "com.vapoursynth.ffms2" not in plugins:
            print('ffms2 is not installed')
            terminate(1)


def setup_encoder(args: Args):
    """
    Settup encoder params and passes

    :param args: the Args
    """
    encoder = ENCODERS[args.encoder]

    # validate encoder settings
    settings_valid, error_msg = encoder.is_valid(args)
    if not settings_valid:
        print(error_msg)
        terminate(1)

    if args.passes is None:
        args.passes = encoder.default_passes

    args.video_params = encoder.default_args if args.video_params is None \
        else shlex.split(args.video_params)


def startup_check(args: Args):
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

    check_exes(args)

    set_vmaf(args)

    if args.reuse_first_pass and args.encoder != 'aom' and args.split_method != 'aom_keyframes':
        print('Reusing the first pass is only supported with \
              the aom encoder and aom_keyframes split method.')
        terminate(1)

    setup_encoder(args)

    # No check because vvc
    if args.encoder == 'vvc':
        args.no_check = True

    if args.encoder == 'svt_vp9' and args.passes == 2:
        print("Implicitly changing 2 pass svt-vp9 to 1 pass\n2 pass svt-vp9 isn't supported")
        args.passes = 1

    args.audio_params = shlex.split(args.audio_params)
    args.ffmpeg = shlex.split(args.ffmpeg)

    args.pix_format = ['-strict', '-1', '-pix_fmt', args.pix_format]
    args.ffmpeg_pipe = [*args.ffmpeg, *args.pix_format,
                        '-bufsize', '50000K', '-f', 'yuv4mpegpipe', '-']
