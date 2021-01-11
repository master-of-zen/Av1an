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
    if is_vs:
        vspipe_process.wait()

    scenes = [0] + scenes + [total_frames]
    index = 1

    while index < len(scenes):
        if scenes[index] < (scenes[index - 1] + min_scene_len):
            scenes.pop(index)
        else:
            index = index + 1

    if len(scenes) > 2:
        scenes.pop(0)
        scenes.pop(len(scenes) - 1)
    else:
        return []

    if len(scenes) > 0 and scenes[0] == 0:
        scenes.remove(0)
    log(f'Found split points: {len(scenes)}\n')
    log(f'Splits: {scenes}\n')

    return scenes