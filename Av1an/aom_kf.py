#!/bin/env python
import os
import re
import struct
import subprocess
from collections import deque
from pathlib import Path
from subprocess import PIPE, STDOUT

import cv2
from tqdm import tqdm

from .commandtypes import CommandPair
from .logger import log
from .utils import terminate
from .ffmpeg import frame_probe

# This is a script that returns a list of keyframes that aom would likely place. Port of aom's C code.
# It requires an aom first-pass stats file as input. FFMPEG first-pass file is not OK. Default filename is stats.bin.
# Script has been tested to have ~99% accuracy vs final aom encode.

# Elements related to parsing the stats file were written by MrSmilingWolf

# All of my contributions to this script are hereby public domain.
# I retain no rights or control over distribution.


# default params for 1st pass when aom isn't the final encoder and -v won't match aom's options
AOM_KEYFRAMES_DEFAULT_PARAMS = '--threads=12 --cpu-used=0 --end-usage=q --cq-level=40'


# Fields meanings: <source root>/av1/encoder/firstpass.h
fields = ['frame', 'weight', 'intra_error', 'frame_avg_wavelet_energy', 'coded_error', 'sr_coded_error', 'tr_coded_error',
         'pcnt_inter', 'pcnt_motion', 'pcnt_second_ref', 'pcnt_third_ref', 'pcnt_neutral', 'intra_skip_pct', 'inactive_zone_rows',
         'inactive_zone_cols', 'MVr', 'mvr_abs', 'MVc', 'mvc_abs', 'MVrv', 'MVcv', 'mv_in_out_count', 'new_mv_count', 'duration', 'count', 'raw_error_stdev']


def get_second_ref_usage_thresh(frame_count_so_far):
    adapt_upto = 32
    min_second_ref_usage_thresh = 0.085
    second_ref_usage_thresh_max_delta = 0.035
    if frame_count_so_far >= adapt_upto:
        return min_second_ref_usage_thresh + second_ref_usage_thresh_max_delta
    return min_second_ref_usage_thresh + (frame_count_so_far / (adapt_upto - 1)) * second_ref_usage_thresh_max_delta


# I have no idea if the following function is necessary in the python implementation or what its purpose even is.
# noinspection PyPep8Naming
def DOUBLE_DIVIDE_CHECK(x):
    if x < 0:
        return x - 0.000001
    else:
        return x + 0.000001


# noinspection PyPep8Naming
def test_candidate_kf(dict_list, current_frame_index, frame_count_so_far):
    previous_frame_dict = dict_list[current_frame_index - 1]
    current_frame_dict = dict_list[current_frame_index]
    future_frame_dict = dict_list[current_frame_index + 1]

    p = previous_frame_dict
    c = current_frame_dict
    f = future_frame_dict

    BOOST_FACTOR = 12.5

    # For more documentation on the below, see
    # https://aomedia.googlesource.com/aom/+/8ac928be918de0d502b7b492708d57ad4d817676/av1/encoder/pass2_strategy.c#1897
    MIN_INTRA_LEVEL = 0.25
    INTRA_VS_INTER_THRESH = 2.0
    VERY_LOW_INTER_THRESH = 0.05
    KF_II_ERR_THRESHOLD = 2.5
    ERR_CHANGE_THRESHOLD = 0.4
    II_IMPROVEMENT_THRESHOLD = 3.5
    KF_II_MAX = 128.0

    qmode = True
    # TODO: allow user to set whether we're testing for constant-q mode keyframe placement or not. it's not a big difference.

    is_keyframe = False

    pcnt_intra = 1.0 - c['pcnt_inter']
    modified_pcnt_inter = c['pcnt_inter'] - c['pcnt_neutral']

    second_ref_usage_thresh = get_second_ref_usage_thresh(frame_count_so_far)

    if ((qmode == False) or (frame_count_so_far > 2)) and (c['pcnt_second_ref'] < second_ref_usage_thresh) and (f['pcnt_second_ref'] < second_ref_usage_thresh) and ((c['pcnt_inter'] < VERY_LOW_INTER_THRESH) or ((pcnt_intra > MIN_INTRA_LEVEL) and (pcnt_intra > (INTRA_VS_INTER_THRESH * modified_pcnt_inter)) and ((c['intra_error'] / DOUBLE_DIVIDE_CHECK(c['coded_error'])) < KF_II_ERR_THRESHOLD) and ((abs(p['coded_error'] - c['coded_error']) / DOUBLE_DIVIDE_CHECK(c['coded_error']) > ERR_CHANGE_THRESHOLD) or (abs(p['intra_error'] - c['intra_error']) / DOUBLE_DIVIDE_CHECK(c['intra_error']) > ERR_CHANGE_THRESHOLD) or ((f['intra_error'] / DOUBLE_DIVIDE_CHECK(f['coded_error'])) > II_IMPROVEMENT_THRESHOLD)))):
        boost_score = 0.0
        old_boost_score = 0.0
        decay_accumulator = 1.0
        for i in range(0, 16):
            lnf = dict_list[current_frame_index + 1 + i]
            next_iiratio = (BOOST_FACTOR * lnf['intra_error'] / DOUBLE_DIVIDE_CHECK(lnf['coded_error']))
            if next_iiratio > KF_II_MAX:
                next_iiratio = KF_II_MAX

            # Cumulative effect of decay in prediction quality.
            if lnf['pcnt_inter'] > 0.85:
                decay_accumulator = decay_accumulator * lnf['pcnt_inter']
            else:
                decay_accumulator = decay_accumulator * ((0.85 + lnf['pcnt_inter']) / 2.0)

            # Keep a running total.
            boost_score += (decay_accumulator * next_iiratio)

            # Test various breakout clauses.
            if (lnf['pcnt_inter'] < 0.05) or (next_iiratio < 1.5) or (((lnf['pcnt_inter'] - lnf['pcnt_neutral']) < 0.20) and (next_iiratio < 3.0)) or ((boost_score - old_boost_score) < 3.0) or (lnf['intra_error'] < 200):
                break
            old_boost_score = boost_score

        # If there is tolerable prediction for at least the next 3 frames then break out else discard this potential key frame and move on
        if boost_score > 30.0 and (i > 3):
            is_keyframe = True
    return is_keyframe


def find_aom_keyframes(stat_file, key_freq_min):
    # I don't know what data format you want as output
    keyframes_list = []

    number_of_frames = round(os.stat(stat_file).st_size / 208) - 1
    dict_list = []

    with open(stat_file, 'rb') as file:
        frame_buf = file.read(208)
        while len(frame_buf) > 0:
            stats = struct.unpack('d' * 26, frame_buf)
            p = dict(zip(fields, stats))
            dict_list.append(p)
            frame_buf = file.read(208)

    # intentionally skipping 0th frame and last 16 frames
    frame_count_so_far = 1
    for i in range(1, number_of_frames - 16):
        is_keyframe = False
        if frame_count_so_far >= key_freq_min:  # https://aomedia.googlesource.com/aom/+/ce97de2724d7ffdfdbe986a14d49366936187298/av1/encoder/pass2_strategy.c#2065
            is_keyframe = test_candidate_kf(dict_list, i, frame_count_so_far)
        if is_keyframe:
            keyframes_list.append(i)
            frame_count_so_far = 0
        frame_count_so_far += 1

    return keyframes_list


def compose_aomsplit_first_pass_command(video_path: Path, stat_file: Path, ffmpeg_pipe, video_params) -> CommandPair:
    """
    Generates the command for the first pass of the entire video used for aom keyframe split

    :param video_path: the video path
    :param stat_file: the stat_file output
    :param ffmpeg_pipe: the av1an.ffmpeg_pipe with pix_fmt and -ff option
    :param video_params: the video params for aomenc first pass
    :return: ffmpeg, encode
    """

    f = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', video_path.as_posix(), *ffmpeg_pipe]
    # removed -w -h from aomenc since ffmpeg filters can change it and it can be added into video_params
    # TODO(n9Mtq4): if an encoder other than aom is being used, video_params becomes the default so -w -h may be needed again
    e = ['aomenc', '--passes=2', '--pass=1', *video_params, f'--fpf={stat_file.as_posix()}', '-o', os.devnull, '-']

    return CommandPair(f, e)


def aom_keyframes(video_path: Path, stat_file, min_scene_len, ffmpeg_pipe, video_params):
    """[Get frame numbers for splits from aomenc 1 pass stat file]
    """

    log(f'Started aom_keyframes scenedetection\nParams: {video_params}\n')
    video = cv2.VideoCapture(video_path.as_posix())  # TODO(n9Mtq4): use a frame probe for this?
    total = int(video.get(cv2.CAP_PROP_FRAME_COUNT))
    video.release()

    if total < 1:
        total = frame_probe(video_path)

    f, e = compose_aomsplit_first_pass_command(video_path, stat_file, ffmpeg_pipe, video_params)

    tqdm_bar = tqdm(total=total, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)

    ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                            stderr=STDOUT, universal_newlines=True)

    encoder_history = deque(maxlen=20)
    frame = 0

    while True:
        line = pipe.stdout.readline()
        if len(line) == 0 and pipe.poll() is not None:
            break
        line = line.strip()

        if line:
            encoder_history.append(line)

        match = re.search(r"frame.*?/([^ ]+?) ", line)
        if match:
            new = int(match.group(1))
            if new > frame:
                tqdm_bar.update(new - frame)
            frame = new

    if pipe.returncode != 0 and pipe.returncode != -2:  # -2 is Ctrl+C for aom
        enc_hist = '\n'.join(encoder_history)
        er = f"\nAom first pass encountered an error: {pipe.returncode}\n{enc_hist}"
        log(er)
        print(er)
        if not stat_file.exists():
            terminate()
        else:
            # aom crashed, but created keyframes.log, so we will try to continue
            print("WARNING: Aom first pass crashed, but created a first pass file. Keyframe splitting may not be accurate.")

    # aom kf-min-dist defaults to 0, but hardcoded to 3 in pass2_strategy.c test_candidate_kf. 0 matches default aom behavior
    # https://aomedia.googlesource.com/aom/+/8ac928be918de0d502b7b492708d57ad4d817676/av1/av1_cx_iface.c#2816
    # https://aomedia.googlesource.com/aom/+/ce97de2724d7ffdfdbe986a14d49366936187298/av1/encoder/pass2_strategy.c#1907
    min_scene_len = 0 if min_scene_len is None else min_scene_len

    keyframes = find_aom_keyframes(stat_file, min_scene_len)

    return keyframes
