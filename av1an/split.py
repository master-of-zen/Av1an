#!/bin/env python

from pathlib import Path
from typing import List

from av1an_pyo3 import (
    extra_splits,
    log,
    read_scenes_from_file,
    write_scenes_to_file,
    av_scenechange_detect,
    Project,
)


def split_routine(project: Project, resuming: bool) -> List[int]:
    scene_file = Path(project.temp) / "scenes.json"

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
        project.frames = frames

    else:
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
        log(f"New splits: {len(scenes)}")

    # write scenes for resuming later if needed
    return scenes


def calc_split_locations(project: Project) -> List[int]:
    sc = []
    if project.split_method == "av-scenechange":
        sc = av_scenechange_detect(
            project.input,
            project.get_frames(),
            project.min_scene_len,
            project.quiet,
            project.is_vs,
        )

    # Write scenes to file
    if project.scenes:
        write_scenes_to_file(
            sc, project.get_frames(), str(Path(project.scenes).resolve())
        )

    return sc
