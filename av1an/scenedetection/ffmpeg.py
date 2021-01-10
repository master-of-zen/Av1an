import re
import subprocess
import sys
from subprocess import Popen

from av1an.logger import log
from av1an.vapoursynth import compose_vapoursynth_pipe

if sys.platform == "linux":
    from os import mkfifo


def ffmpeg(video, threshold, min_scene_len, total_frames, is_vs, temp):
    """
    Running FFMPEG detection on source video for segmenting.
    Usually the optimal threshold is 0.1 - 0.3 but it can vary a lot
    based on your source content.
    """

    log(f'Starting FFMPEG detection:\nThreshold: {threshold}, Is Vapoursynth input: {is_vs}\n')

    if is_vs:
        # Handling vapoursynth. Outputs vs to a file so ffmpeg can handle it.
        if sys.platform == "linux":
            vspipe_fifo = temp / 'vspipe.y4m'
            mkfifo(vspipe_fifo)
        else:
            vspipe_fifo = None

        vspipe_cmd = compose_vapoursynth_pipe(video, vspipe_fifo)
        vspipe_process = Popen(vspipe_cmd)

    finfo = "showinfo,select=gt(scene\\," + str(threshold) + "),showinfo"
    ffmpeg_cmd = ["ffmpeg", "-i", str(vspipe_fifo if is_vs else video.as_posix()), "-hide_banner", "-loglevel", "32",
                  "-filter_complex", finfo, "-an", "-f", "null", "-"]
    pipe = subprocess.Popen(ffmpeg_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    last_frame = -1
    scenes = []
    while True:
        line = pipe.stderr.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            print(pipe.poll())
            break
        if len(line) == 0:
            continue
        if line:
            cur_frame = re.search("n:\\ *[0-9]+", str(line))
            if cur_frame is not None:
                frame_num = re.search("[0-9]+", cur_frame.group(0))
                if frame_num is not None:
                    frame_num = int(frame_num.group(0))
                    if frame_num < last_frame:
                        scenes += [last_frame]
                    else:
                        last_frame = frame_num

    # If fed using a vspipe process, ensure that vspipe has finished.
    if is_vs:
        vspipe_process.wait()

    # General purpose min_scene_len implementation that works if "scenes" are sorted from smallest
    # to largest.

    # First add the first and last frame so you can test if those are too close
    scenes = [0] + scenes + [total_frames]
    index = 1

    while index < len(scenes):
        # Check if this current split is too close to the previous split
        if scenes[index] < (scenes[index - 1] + min_scene_len):
            # if so remove the current split and then recheck if index < len(scenes)
            scenes.pop(index)
        else:
            index = index + 1

    # Remove the first and last splits. the first split will always be at frame 0 which is bad
    # and the last split will either be the last frame of video, or the actual last split.
    # if it's the last frame of video it should be removed
    # and if it's the last split it means that the last frame of video was too close to that
    # last split and thus the duration of the last split was too small and should have been removed
    if len(scenes) > 2:
        scenes.pop(0)
        scenes.pop(len(scenes) - 1)
    else:
        # Will only occur if literally all possible splits were removed for the min_scene_len
        return []

    # Remove 0 from list
    if len(scenes) > 0 and scenes[0] == 0:
        scenes.remove(0)
    log(f'Found split points: {len(scenes)}\n')
    log(f'Splits: {scenes}\n')

    return scenes