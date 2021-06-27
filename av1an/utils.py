#!/bin/env python

import re
from typing import List
from pathlib import Path
import cv2

from av1an.vapoursynth import is_vapoursynth
from av1an_pyo3 import frame_probe_vspipe, ffmpeg_get_frame_count, log


def frame_probe_fast(source: Path, is_vs: bool = False):
    """
    Consolidated function to retrieve the number of frames from the input quickly,
    falls back on a slower (but accurate) frame count if a quick count cannot be found.

    Handles vapoursynth input as well.
    """
    total = 0
    if not is_vs:
        try:
            import vapoursynth
            from vapoursynth import core

            plugins = vapoursynth.get_core().get_plugins()
            if "systems.innocent.lsmas" in plugins:
                total = core.lsmas.LWLibavSource(
                    source.as_posix(), cache=False
                ).num_frames
                log("Get frame count with lsmash")
                log(f"Frame count: {total}")
                return total
        except:
            video = cv2.VideoCapture(source.as_posix())
            total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
            video.release()
            log("Can't open input with Pyscenedetect OpenCV")
    if is_vs or total < 1:
        total = frame_probe(source)

    return total


def frame_probe(source: Path):
    """
    Determines the total number of frames in a given input.

    Differentiates between a Vapoursynth script and standard video
    and delegates to vspipe or ffmpeg respectively.
    """
    if is_vapoursynth(source):
        return frame_probe_vspipe(source)

    return ffmpeg_get_frame_count(str(source.resolve()))
