#!/bin/env python
import os
import struct
from typing import List, Dict

from .aom_keyframes import fields


def read_first_pass(log_path):
    """
    Reads libaom first pass log into a list of dictionaries.

    :param log_path: the path to the log file
    :return: A list of dictionaries. The keys are the fields from aom_keyframes.py
    """
    frame_stats = []
    with open(log_path, 'rb') as file:
        frame_buf = file.read(208)
        while len(frame_buf) > 0:
            stats = struct.unpack('d' * 26, frame_buf)
            p = dict(zip(fields, stats))
            frame_stats.append(p)
            frame_buf = file.read(208)
    return frame_stats


def write_first_pass_log(log_path, frm_lst: List[Dict]):
    with open(log_path, 'wb') as file:
        for frm in frm_lst:
            frm_bin = struct.pack('d' * 26, *frm.values())
            file.write(frm_bin)


def reindex_chunk(stats: List[Dict]):
    for i, frm_stats in enumerate(stats):
        frm_stats['frame'] = i


def compute_eos_stats(stats: List[Dict], old_eos: Dict):
    # TODO(n9Mtq4): research this EOS packet and determine what actually needs to be done here. The minimal code for aom to accept it is here
    eos = old_eos.copy()
    eos['count'] = len(stats)
    return eos


def segment_first_pass(temp, framenums):
    stat_file = temp / 'keyframes.log'  # TODO(n9Mtq4): makes this a constant for use here and w/ aom_keyframes.py
    stats = read_first_pass(stat_file)

    # special case for only 1 scene
    if len(framenums) == 0:
        write_first_pass_log(os.path.join(temp, "split", "0.log"), stats)
        return

    split_names = [str(i).zfill(5) for i in range(len(framenums) + 1)]
    frm_split = [0] + framenums + [len(stats) - 1]

    for i in range(0, len(frm_split) - 1):
        frm_start_idx = frm_split[i]
        frm_end_idx = frm_split[i + 1]
        log_name = split_names[i] + '.log'

        chunk_stats = stats[frm_start_idx:frm_end_idx]
        reindex_chunk(chunk_stats)
        chunk_stats = chunk_stats + [compute_eos_stats(chunk_stats, stats[-1])]

        write_first_pass_log(os.path.join(temp, "split", log_name), chunk_stats)
