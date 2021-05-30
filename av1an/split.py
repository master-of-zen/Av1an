#!/bin/env python

import os
import subprocess
import json
from pathlib import Path
from subprocess import PIPE, STDOUT
from typing import List, Tuple
from numpy import linspace
import sys

from .project import Project
from .scenedetection import aom_keyframes, AOM_KEYFRAMES_DEFAULT_PARAMS, pyscene, ffmpeg
from .logger import log

from av1an_pyo3 import extra_splits, read_scenes_from_file, write_scenes_to_file

# TODO: organize to single segmenting/splitting module


def split_routine(project: Project, resuming: bool) -> List[int]:
    """
    Performs the split routine. Runs pyscenedetect/aom keyframes and adds in extra splits if needed

    :param project: the Project
    :param resuming: if the encode is being resumed
    :return: A list of frames to split on
    """
    scene_file = project.temp / "scenes.json"

    # if resuming, we already have the split file, so just read that and return
    if resuming:
        scenes, frames = read_scenes_from_file(str(scene_file.resolve()))
        project.set_frames(frames)
        return scenes

    # Run scenedetection or skip
    if project.split_method == "none":
        log("Skipping scene detection")
        scenes = []

    # Read saved scenes:
    if project.scenes and Path(project.scenes).exists():
        log("Using Saved Scenes")
        scenes, frames = read_scenes_from_file(str(Path(project.scenes).resolve()))
        project.set_frames(frames)

    else:
        # determines split frames with pyscenedetect or aom keyframes
        scenes = calc_split_locations(project)
        if project.scenes and Path(project.scenes).exists():
            write_scenes_to_file(
                scenes, project.get_frames(), str(Path(project.scenes).resolve())
            )

    # Write internal scenes
    write_scenes_to_file(scenes, project.get_frames(), str(scene_file.resolve()))

    # Applying extra splits
    if project.extra_split:
        log("Applying extra splits")
        log(f"Split distance: {project.extra_split}")
        scenes = extra_splits(scenes, project.get_frames(), project.extra_split)
        log(f"New splits:{len(scenes)}")

    # write scenes for resuming later if needed
    return scenes


def calc_split_locations(project: Project) -> List[int]:
    """
    Determines a list of frame numbers to split on with pyscenedetect or aom keyframes

    :param project: the Project
    :return: A list of frame numbers
    """
    # inherit video params from aom encode unless we are using a different encoder, then use defaults
    aom_keyframes_params = (
        project.video_params
        if (project.encoder == "aom")
        else AOM_KEYFRAMES_DEFAULT_PARAMS
    )

    sc = []

    # Splitting using PySceneDetect
    if project.split_method == "pyscene":
        log(
            f"Starting scene detection Threshold: {project.threshold}, Min_scene_length: {project.min_scene_len}"
        )
        try:
            sc = pyscene(
                project.input,
                project.threshold,
                project.min_scene_len,
                project.is_vs,
                project.temp,
                project.quiet,
            )
        except Exception as e:
            log(f"Error in PySceneDetect: {e}")
            print(f"Error in PySceneDetect: {e}")
            sys.exit(1)

    # Splitting based on aom keyframe placement
    elif project.split_method == "aom_keyframes":
        stat_file = project.temp / "keyframes.log"
        sc = aom_keyframes(
            project.input,
            stat_file,
            project.min_scene_len,
            project.ffmpeg_pipe,
            aom_keyframes_params,
            project.is_vs,
            project.quiet,
        )

    elif project.split_method == "ffmpeg":
        sc = ffmpeg(
            project.input,
            project.threshold,
            project.min_scene_len,
            project.get_frames(),
            project.is_vs,
            project.temp,
        )

    # Write scenes to file
    if project.scenes:
        write_scenes_to_file(
            sc, project.get_frames(), str(Path(project.scenes).resolve())
        )

    return sc
