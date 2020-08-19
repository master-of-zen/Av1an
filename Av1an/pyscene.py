#!/bin/env python

from scenedetect.detectors import ContentDetector
from scenedetect.scene_manager import SceneManager
from scenedetect.video_manager import VideoManager

from .logger import log


def pyscene(video, threshold, min_scene_len):
    """
    Running PySceneDetect detection on source video for segmenting.
    Optimal threshold settings 15-50
    """
    if not min_scene_len:
        min_scene_len = 15

    log(f'Starting PySceneDetect:\nThreshold: {threshold}, Min scene lenght: {min_scene_len}\n')
    video_manager = VideoManager([str(video)])
    scene_manager = SceneManager()
    scene_manager.add_detector(ContentDetector(threshold=threshold, min_scene_len=min_scene_len))
    base_timecode = video_manager.get_base_timecode()

    # Work on whole video
    video_manager.set_duration()

    # Set downscale factor to improve processing speed.
    video_manager.set_downscale_factor()

    # Start video_manager.
    video_manager.start()

    scene_manager.detect_scenes(frame_source=video_manager, show_progress=True)

    # Obtain list of detected scenes.
    scene_list = scene_manager.get_scene_list(base_timecode)

    scenes = [int(scene[0].get_frames()) for scene in scene_list]

    # Remove 0 from list
    if scenes[0] == 0:
        scenes.remove(0)
    log(f'Found scenes: {len(scenes)}\n')

    return scenes
