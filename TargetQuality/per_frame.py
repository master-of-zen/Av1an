#/bin/env python
from Projects import Project
from Av1an.bar import process_pipe
from Chunks.chunk import Chunk
from Av1an.commandtypes import CommandPair, Command
from Av1an.logger import log
from VMAF import call_vmaf, read_weighted_vmaf
from VMAF import read_json
from .target_quality import gen_probes_names


def per_frame_target_quality_routine(args: Project, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param args: the Project
    :param chunk: the Chunk
    :return: None
    """
    chunk.per_frame_target_quality_cq = per_frame_target_quality(chunk, args)


def gen_probes_qp(frame_list):
    return None


def get_next_probes(frame_list, chunk, args):
    return None


def per_frame_probe_cmd(chunk: Chunk, q, ffmpeg_pipe, encoder, probing_rate, qp_file) -> CommandPair:
    """
    Generate and return commands for probes at set Q values
    These are specifically not the commands that are generated
    by the user or encoder defaults, since these
    should be faster than the actual encoding commands.
    These should not be moved into encoder classes at this point.
    """
    pipe = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', '-vf', f'select=not(mod(n\\,{probing_rate}))',
            *ffmpeg_pipe]

    probe_name = gen_probes_names(chunk, q).with_suffix('.ivf').as_posix()

    if encoder == 'svt_av1':
        params = ['SvtAv1EncApp', '-i', 'stdin',
                  '--preset', '8', '--rc', '0',
                  '--use-q-file', '--qpfile', f'{qp_file}']

        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])
    else:
        print('supported only by SVT-AV1')
        exit()

    return cmd




def per_frame_target_quality(chunk, args):
    frames = chunk.frames
    # First q value to make probe at
    middle_point = (args.min_q + args.max_q) // 2
    frame_list = [{'frame_number': x, 'probes': [(middle_point, -1)]} for x in range(frames)]





    return 1
