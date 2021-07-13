#!/bin/env python

from pathlib import Path

import cv2
from av1an_pyo3 import ffmpeg_get_frame_count, frame_probe_vspipe, log, is_vapoursynth


def frame_probe_fast(source: Path, is_vs: bool = False):
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
    if is_vs or total < 1:
        total = frame_probe(source)

    return total


def frame_probe(source: Path):
    if is_vapoursynth(source.as_posix()):
        return frame_probe_vspipe(source.resolve().as_posix())

    return ffmpeg_get_frame_count(source.resolve().as_posix())
