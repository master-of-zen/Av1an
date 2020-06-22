import os
import sys
import subprocess
from subprocess import PIPE, STDOUT
from utils import frame_probe, get_keyframes
from .logger import log
from pathlib import Path
from .utils import terminate
from ast import literal_eval
from .pyscenedetect import pyscene
from .aom_keyframes import aom_keyframes

def segment(video:Path, temp, frames):
    """Split video by frame numbers, or just copying video."""

    log('Split Video\n')
    cmd = [
        "ffmpeg", "-hide_banner", "-y",
        "-i", video.absolute().as_posix(),
        "-map", "0:v:0",
        "-an",
        "-c", "copy",
        "-avoid_negative_ts", "1"
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

def reduce_scenes(scenes):
    """Windows terminal can't handle more than ~500 scenes in length."""
    count = len(scenes)
    interval = int(count / 500 + (count % 500 > 0))
    scenes = scenes[::interval]
    return scenes

def extra_splits(video, frames: list, split_distance):
    log('Applying extra splits\n')
    frames.append(frame_probe(video))
    # Get all keyframes of original video
    keyframes = get_keyframes(video)

    t = frames[:]
    t.insert(0, 0)
    splits = list(zip(t, frames))
    for i in splits:
        # Getting distance between splits
        distance = (i[1] - i[0])

        if distance > split_distance:
            # Keyframes that between 2 split points
            candidates = [k for k in keyframes if i[1] > k > i[0]]

            if len(candidates) > 0:
                # Getting number of splits that need to be inserted
                to_insert = min((i[1] - i[0]) // split_distance, (len(candidates)))
                for k in range(0, to_insert):
                    # Approximation of splits position
                    aprox_to_place = (((k + 1) * distance) // (to_insert + 1)) + i[0]

                    # Getting keyframe closest to approximated
                    key = min(candidates, key=lambda x: abs(x - aprox_to_place))
                    frames.append(key)
    result = [int(x) for x in sorted(frames)]
    log(f'Split distance: {split_distance}\nNew splits:{len(len(result))}\n')
    return result

def split_routine(video, scenes, split_method, temp, min_scene_len, queue, threshold):

        if scenes == '0':
            log('Skipping scene detection\n')
            return []

        sc = []

        if scenes:
            scenes = Path(scenes)
            if scenes.exists():
                # Read stats from CSV file opened in read mode:
                with scenes.open() as stats_file:
                    stats = list(literal_eval(stats_file.read().strip()))
                    log('Using Saved Scenes\n')
                    return stats

        # Splitting using PySceneDetect
        if split_method == 'pyscene':
            log(f'Starting scene detection Threshold: {threshold}, Min_scene_length: {min_scene_len}\n')
            try:
                sc = pyscene(video, threshold, queue, min_scene_len)
            except Exception as e:
                log(f'Error in PySceneDetect: {e}\n')
                print(f'Error in PySceneDetect{e}\n')
                terminate()

        # Splitting based on aom keyframe placement
        elif split_method == 'aom_keyframes':
            try:
                stat_file = temp / 'keyframes.log'
                sc = aom_keyframes(video, stat_file, min_scene_len)
            except:
                log('Error in aom_keyframes')
                print('Error in aom_keyframes')
                terminate()
        else:
            print(f'No valid split option: {split_method}\nValid options: "pyscene", "aom_keyframes"')
            terminate()

        # Fix for windows character limit
        if sys.platform != 'linux':
            if len(sc) > 600:
                sc = reduce_scenes(sc)

        # Write scenes to file

        if scenes:
            Path(scenes).write_text(','.join([str(x) for x in sc]))

        return sc