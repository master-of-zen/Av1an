#!/bin/env python
import os
import struct
from typing import Dict, List

from .aom_kf import fields


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
    """
    Writes a libaom compatible first pass log from a list of dictionaries containing frame stats.

    :param log_path: the path of the ouput file
    :param frm_lst: the list of dictionaries of the frame stats + eos stat
    :return: None
    """
    with open(log_path, 'wb') as file:
        for frm in frm_lst:
            frm_bin = struct.pack('d' * 26, *frm.values())
            file.write(frm_bin)


def reindex_chunk(chunk_stats: List[Dict]):
    """
    The stats for each frame includes its frame number. This will reindex them to start at 0 in place.

    :param chunk_stats: the list of stats for just this chunk
    :return: None
    """
    for i, frm_stats in enumerate(chunk_stats):
        frm_stats['frame'] = i


def compute_eos_stats(chunk_stats: List[Dict], old_eos: Dict):
    """
    The end of sequence stat is a final packet at the end of the log. It contains the sum of all the previous
    frame packets. When we split the log file, we need to sum up just the included frames as a new EOS packet.

    :param chunk_stats: the list of stats for just this chunk
    :param old_eos: the old eos stat packet
    :return: A dict for the new eos packet
    """
    eos = old_eos.copy()
    for key in eos.keys():
        eos[key] = sum([d[key] for d in chunk_stats])
        # TODO(n9Mtq4): I think this will work well for VBR encodes
        # eos[key] = (old_eos[key] / old_eos['count']) * len(chunk_stats)
    return eos


def segment_first_pass(temp, framenums):
    """
    Segments the first pass file in temp/keyframes.log into individual log files for each chunk.
    Looks at the len of framenums to determine file names for the chunks.

    :param temp: the temp directory Path
    :param framenums: a list of frame numbers along the split boundaries
    :return: None
    """
    stat_file = temp / 'keyframes.log'  # TODO(n9Mtq4): makes this a constant for use here and w/ aom_keyframes.py
    stats = read_first_pass(stat_file)

    # special case for only 1 scene
    # we don't need to do anything with the log
    if len(framenums) == 0:
        write_first_pass_log(os.path.join(temp, "split", "0_fpf.log"), stats)
        return

    eos_stats = stats[-1]  # EOS stats is the last one
    split_names = [str(i).zfill(5) for i in range(len(framenums) + 1)]
    frm_split = [0] + framenums + [len(stats) - 1]

    for i in range(0, len(frm_split) - 1):
        frm_start_idx = frm_split[i]
        frm_end_idx = frm_split[i + 1]
        log_name = split_names[i] + '_fpf.log'

        chunk_stats = stats[frm_start_idx:frm_end_idx]
        reindex_chunk(chunk_stats)
        chunk_stats = chunk_stats + [compute_eos_stats(chunk_stats, eos_stats)]

        write_first_pass_log(os.path.join(temp, "split", log_name), chunk_stats)
