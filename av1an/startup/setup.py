#!/bin/env python

import atexit
import os
import shlex
import sys
from distutils.spawn import find_executable
from pathlib import Path

from av1an.project import Project
from av1an.startup.validate_commands import validate_inputs
from av1an.vapoursynth import is_vapoursynth
from av1an_pyo3 import (
    encoder_bin,
    get_default_arguments,
    get_default_cq_range,
    get_default_pass,
    get_ffmpeg_info,
    log,
)


def startup_check(project: Project):
    if sys.version_info < (3, 6):
        print("Python 3.6+ required")
        sys.exit()
    if sys.platform == "linux":

        def restore_term():
            os.system("stty sane")

        atexit.register(restore_term)

    if project.encoder not in ["rav1e", "aom", "svt_av1", "vpx"] and project.output_ivf:
        print(".ivf only supports VP8, VP9, and AV1")
        sys.exit(1)

    if not project.chunk_method:
        project.select_best_chunking_method()

    project.is_vs = is_vapoursynth(project.input[0])

    if project.is_vs:
        project.chunk_method = "vs_ffms2"

    if not find_executable("ffmpeg"):
        print("No ffmpeg")
        sys.exit(1)
    else:
        log(get_ffmpeg_info())

    if project.chunk_method in ["vs_ffms2", "vs_lsmash"]:
        if not find_executable("vspipe"):
            print("vspipe executable not found")
            sys.exit(1)

        try:
            import vapoursynth

            plugins = vapoursynth.get_core().get_plugins()

            if (
                project.chunk_method == "vs_lsmash"
                and "systems.innocent.lsmas" not in plugins
            ):
                print("lsmas is not installed")
                sys.exit(1)

            if (
                project.chunk_method == "vs_ffms2"
                and "com.vapoursynth.ffms2" not in plugins
            ):
                print("ffms2 is not installed")
                sys.exit(1)
        except ModuleNotFoundError:
            print("Vapoursynth is not installed")
            sys.exit(1)

    if project.vmaf_path:
        if not Path(project.vmaf_path).exists():
            print(f"No model with this path: {Path(project.vmaf_path).as_posix()}")
            sys.exit(1)

    if project.probes < 4:
        print(
            "Target quality with less than 4 probes is experimental and not recommended"
        )

    # setting range for q values
    if project.min_q is None:
        project.min_q, _ = get_default_cq_range(project.encoder)
        assert project.min_q > 1

    if project.max_q is None:
        _, project.max_q = get_default_cq_range(project.encoder)

    settings_valid = find_executable(encoder_bin(project.encoder))

    if not settings_valid:
        print(
            f"Encoder {encoder_bin(project.encoder)} not found. Is it installed in the system path?"
        )
        sys.exit(1)

    if project.passes is None:
        project.passes = get_default_pass(project.encoder)

    project.video_params = (
        get_default_arguments(project.encoder)
        if project.video_params is None
        else shlex.split(project.video_params)
    )

    validate_inputs(project)
    project.audio_params = shlex.split(project.audio_params)
    project.ffmpeg = shlex.split(project.ffmpeg)

    project.pix_format = ["-strict", "-1", "-pix_fmt", project.pix_format]
    project.ffmpeg_pipe = [
        *project.ffmpeg,
        *project.pix_format,
        "-f",
        "yuv4mpegpipe",
        "-",
    ]
