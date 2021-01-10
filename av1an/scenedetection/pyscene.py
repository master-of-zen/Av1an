#!/bin/env python

import sys
from subprocess import Popen

try:
    from scenedetect.detectors import ContentDetector
    from scenedetect.scene_manager import SceneManager
    from scenedetect.video_manager import VideoManager
    from scenedetect.frame_timecode import FrameTimecode
except ImportError:
    ContentDetector = None

from av1an.logger import log
from av1an.utils import frame_probe
from av1an.vapoursynth import compose_vapoursynth_pipe

if sys.platform == "linux":
    from os import mkfifo


def pyscene(video, threshold, min_scene_len, is_vs, temp, quiet):
    """
    Running PySceneDetect detection on source video for segmenting.
    Optimal threshold settings 15-50
    """
    if not min_scene_len:
        min_scene_len = 15

    if ContentDetector is None:
        log(f'Unable to start PySceneDetect because it was not found. Please install scenedetect[opencv] to use')
        return []

    log(f'Starting PySceneDetect:\nThreshold: {threshold}, Min scene length: {min_scene_len}\n Is Vapoursynth input: {is_vs}\n')

    if is_vs:
        # Handling vapoursynth, so we need to create a named pipe to feed to VideoManager.
        # TODO: Do we clean this up after pyscenedetect has run, or leave it as part of the temp dir, where it will be cleaned up later?
        if sys.platform == "linux":
            vspipe_fifo = temp / 'vspipe.y4m'
            mkfifo(vspipe_fifo)
        else:
            vspipe_fifo = None

        vspipe_cmd = compose_vapoursynth_pipe(video, vspipe_fifo)
        vspipe_process = Popen(vspipe_cmd)

        # Get number of frames from Vapoursynth script to pass as duration to VideoManager.
        # We need to pass the number of frames to the manager, otherwise it won't close the
        # receiving end of the pipe, and will simply sit waiting after vspipe has finished sending
        # the last frame.
        frames = frame_probe(video)

    video_manager = VideoManager([str(vspipe_fifo if is_vs else video)])
    scene_manager = SceneManager()
    scene_manager.add_detector(ContentDetector(threshold=threshold, min_scene_len=min_scene_len))
    base_timecode = video_manager.get_base_timecode()

    video_manager.set_duration(duration=FrameTimecode(frames, video_manager.get_framerate()) if is_vs else None)

    # Set downscale factor to improve processing speed.
    video_manager.set_downscale_factor()

    # Start video_manager.
    video_manager.start()

    scene_manager.detect_scenes(frame_source=video_manager, show_progress=(not quiet))

    # If fed using a vspipe process, ensure that vspipe has finished.
    if is_vs:
        vspipe_process.wait()

    # Obtain list of detected scenes.
    scene_list = scene_manager.get_scene_list(base_timecode)

    scenes = [int(scene[0].get_frames()) for scene in scene_list]

    # Remove 0 from list
    if scenes[0] == 0:
        scenes.remove(0)
    log(f'Found scenes: {len(scenes)}\n')

    return scenes
