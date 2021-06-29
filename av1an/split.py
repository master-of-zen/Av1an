#!/bin/env python

import sys
from pathlib import Path
from typing import List

from av1an_pyo3 import extra_splits, log, read_scenes_from_file, write_scenes_to_file

from .project import Project
from .scenedetection import ffmpeg, pyscene


def split_routine(project: Project, resuming: bool) -> List[int]:
    scene_file = project.temp / "scenes.json"

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
