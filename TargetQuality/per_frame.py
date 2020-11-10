#/bin/env python
from Av1an.arg_parse import Args
from Av1an.bar import process_pipe
from Chunks.chunk import Chunk
from Av1an.commandtypes import CommandPair, Command
from Av1an.logger import log
from VMAF import call_vmaf, read_weighted_vmaf
from VMAF import read_json


def per_frame_target_quality_routine(args: Args, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param args: the Args
    :param chunk: the Chunk
    :return: None
    """
    chunk.per_frame_target_quality_cq = per_frame_target_quality(chunk, args)


def per_frame_target_quality(chunk, args):
    return 0
