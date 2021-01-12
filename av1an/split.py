#!/bin/env python

import os
import subprocess
import json
from pathlib import Path
from subprocess import PIPE, STDOUT
from typing import List
from numpy import linspace

from .project import Project
from .scenedetection import aom_keyframes, AOM_KEYFRAMES_DEFAULT_PARAMS, pyscene, ffmpeg
from .logger import log
from .utils import terminate, frame_probe

# TODO: organize to single segmenting/splitting module

def split_routine(project: Project, resuming: bool) -> List[int]:
    """
    Performs the split routine. Runs pyscenedetect/aom keyframes and adds in extra splits if needed

    :param project: the Project
    :param resuming: if the encode is being resumed
    :return: A list of frames to split on
    """
    scene_file = project.temp / 'scenes.txt'

    # if resuming, we already have the split file, so just read that and return
    if resuming:
        scenes, frames =  read_scenes_from_file(scene_file)
        project.set_frames(frames)
        return scenes

    # Run scenedetection or skip
    if project.split_method == 'none':
        log('Skipping scene detection\n')
        scenes = []

    # Read saved scenes:
    if project.scenes and Path(project.scenes).exists():
        log('Using Saved Scenes\n')
        scenes, frames = read_scenes_from_file(Path(project.scenes))
        project.set_frames(frames)

    else:
        # determines split frames with pyscenedetect or aom keyframes
        scenes = calc_split_locations(project)
        if project.scenes and Path(project.scenes).exists():
            write_scenes_to_file(scenes, project.get_frames(), Path(project.scenes))

    # Write internal scenes
    write_scenes_to_file(scenes, project.get_frames(), scene_file)

    # Applying extra splits
    if project.extra_split:
        scenes = extra_splits(project, scenes)

    # write scenes for resuming later if needed
    return scenes


def write_scenes_to_file(scenes: List[int], frames: int, scene_path: Path):
    """
    Writes a list of scenes to the a file

    :param scenes: the scenes to write
    :param scene_path: the file to write to
    :return: None
    """
    with open(scene_path, 'w') as scene_file:
        data = {'scenes': scenes, 'frames': frames }
        json.dump(data, scene_file)


def read_scenes_from_file(scene_path: Path) -> List[int]:
    """
    Reads a list of split locations from a file

    :param scene_path: the file to read from
    :return: a list of frames to split on
    """
    with open(scene_path, 'r') as scene_file:
        data = json.load(scene_file)
        return data['scenes'], data['frames']


def segment(video: Path, temp: Path, frames: List[int]):
    """
    Uses ffmpeg to segment the video into separate files.
    Splits the video by frame numbers or copies the video if no splits are needed

    :param video: the source video
    :param temp: the temp directory
    :param frames: the split locations
    :return: None
    """

    log('Split Video\n')
    cmd = [
        "ffmpeg", "-hide_banner", "-y",
        "-i", video.absolute().as_posix(),
        "-map", "0:v:0",
        "-an",
        "-c", "copy",
        "-avoid_negative_ts", "1",
        "-vsync", "0"
    ]

    if len(frames) > 0:
        cmd.extend([
            "-f", "segment",
            "-segment_frames", ','.join([str(x) for x in frames])
        ])
        cmd.append(os.path.join(temp, "split", "%05d.mkv"))
    else:
        cmd.append(os.path.join(temp, "split", "0.mkv"))
    pipe = subprocess.Popen(cmd, stdout=PIPE, stderr=STDOUT)
    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break

    log('Split Done\n')


def extra_splits(project: Project, split_locations: list):
    log('Applying extra splits\n')

    split_locs_with_start = split_locations[:]
    split_locs_with_start.insert(0, 0)

    split_locs_with_end = split_locations[:]
    split_locs_with_end.append(project.get_frames())

    splits = list(zip(split_locs_with_start, split_locs_with_end))
    for i in splits:
        distance = (i[1] - i[0])
        if distance > project.extra_split:
            to_add = distance // project.extra_split
            new_scenes = list(linspace(i[0],i[1], to_add + 1, dtype=int, endpoint=False)[1:])
            split_locations.extend(new_scenes)

    result = [int(x) for x in sorted(split_locations)]
    log(f'Split distance: {project.extra_split}\nNew splits:{len(result)}\n')
    return result


def calc_split_locations(project: Project) -> List[int]:
    """
    Determines a list of frame numbers to split on with pyscenedetect or aom keyframes

    :param project: the Project
    :return: A list of frame numbers
    """
    # inherit video params from aom encode unless we are using a different encoder, then use defaults
    aom_keyframes_params = project.video_params if (project.encoder == 'aom') else AOM_KEYFRAMES_DEFAULT_PARAMS

    sc = []

    # Splitting using PySceneDetect
    if project.split_method == 'pyscene':
        log(f'Starting scene detection Threshold: {project.threshold}, Min_scene_length: {project.min_scene_len}\n')
        try:
            sc = pyscene(project.input, project.threshold, project.min_scene_len, project.is_vs, project.temp, project.quiet)
        except Exception as e:
            log(f'Error in PySceneDetect: {e}\n')
            print(f'Error in PySceneDetect{e}\n')
            terminate()

    # Splitting based on aom keyframe placement
    elif project.split_method == 'aom_keyframes':
        stat_file = project.temp / 'keyframes.log'
        sc = aom_keyframes(project.input, stat_file, project.min_scene_len, project.ffmpeg_pipe, aom_keyframes_params, project.is_vs, project.quiet)

    elif project.split_method == 'ffmpeg':
        sc = ffmpeg(project.input, project.threshold, project.min_scene_len, project.get_frames(), project.is_vs, project.temp)

    # Write scenes to file
    if project.scenes:
        write_scenes_to_file(sc, project.get_frames(), project.scenes)

    return sc
