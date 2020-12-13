#/bin/env python
from pathlib import Path
from Projects import Project
from Av1an.bar import process_pipe
from Chunks.chunk import Chunk
from Av1an.commandtypes import CommandPair, Command
from Av1an.logger import log
from VMAF import call_vmaf, read_weighted_vmaf, read_json
from .target_quality import gen_probes_names, make_pipes, vmaf_probe, weighted_search
from scipy import interpolate
import pprint
import numpy as np

def per_frame_target_quality_routine(project: Project, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param args: the Project
    :param chunk: the Chunk
    :return: None
    """
    chunk.per_frame_target_quality_q_list = per_frame_target_quality(chunk, project)


def make_q_file(q_list, chunk):
    qfile = chunk.fake_input_path.with_name(f'probe_{chunk.name}').with_suffix('.txt')
    with open(qfile, 'w') as fl:
        text = ''

        for x in q_list:
            text += str(x) + '\n'
        fl.write(text)
    return qfile


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
                  '--preset', '8', '--rc', '0', '--passes', '1',
                  '--use-q-file','1', '--qpfile', f'{qp_file.as_posix()}']

        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])

    elif encoder == 'x265':
        params = ['x265', '--log-level', '0', '--no-progress',
                  '--y4m', '--preset', 'fast', '--crf', f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])


    else:
        print('supported only by SVT-AV1 and x265')
        exit()

    return cmd


def per_frame_probe(q_list, q, chunk, project):
    qfile = chunk.make_q_file(q_list)
    cmd = per_frame_probe_cmd(chunk, q, project.ffmpeg_pipe, project.encoder, 1, qfile)
    pipe = make_pipes(chunk.ffmpeg_gen_cmd, cmd)
    process_pipe(pipe, chunk)

    fl = call_vmaf(chunk, gen_probes_names(chunk, q), project.n_threads, project.vmaf_path, project.vmaf_res, vmaf_filter=project.vmaf_filter, vmaf_rate=1)
    jsn = read_json(fl)
    vmafs = [x['metrics']['vmaf'] for x in jsn['frames']]
    return vmafs


def add_probes_to_frame_list(frame_list, q_list, vmafs):
    frame_list = list(frame_list)
    for index, q_vmaf in enumerate(zip(q_list, vmafs)):
        frame_list[index]['probes'].append((q_vmaf[0], q_vmaf[1]))

    return frame_list



def per_frame_target_quality(chunk, project):
    frames = chunk.frames
    frame_list = [{'frame_number': x, 'probes': []} for x in range(frames)]

    for _ in range(project.probes):
        q_list = gen_next_q(frame_list, chunk, project)
        vmafs = per_frame_probe(q_list, 1, chunk, project)
        frame_list = add_probes_to_frame_list(frame_list, q_list, vmafs)
        mse = round(get_square_error([x['probes'][-1][1] for x in frame_list] ,project.target_quality), 2)
        # print(':: MSE:', mse)

        if mse < 1.0:
            return q_list

    return q_list


def get_square_error(ls, target):
    total = 0
    for i in ls:
        dif = i - target
        total += dif ** 2
    mse = total / len(ls)
    return mse


def gen_next_q(frame_list, chunk, project):
    q_list = []

    probes = len(frame_list[0]['probes'])

    if probes == 0:
        return [project.min_q] * len(frame_list)
    elif probes == 1:
        return [project.max_q] * len(frame_list)
    else:
        for probe in frame_list:

            x = [x[0] for x in probe['probes']]
            y = [x[1] for x in probe['probes']]

            if probes > 2:
                if len(x) != len(set(x)):
                    q_list.append(probe['probes'][-1][0])
                    continue

            interpolation = 'quadratic' if probes > 2 else 'linear'

            f = interpolate.interp1d(x, y, kind=interpolation)
            xnew = np.linspace(min(x), max(x), max(x) - min(x))
            tl = list(zip(xnew, f(xnew)))
            q = min(tl, key=lambda l: abs(l[1] - project.target_quality))

            q_list.append(int(round(q[0])))

        return q_list


def search(q1, v1, q2, v2, target):

    if abs(target - v2) < 0.5:
        return q2

    if v1 > target and v2 > target:
        return min(q1, q2)
    if v1 < target and v1 < target:
        return max(q1, q2)

    dif1 = abs(target - v2)
    dif2 = abs(target - v1)

    tot = dif1 + dif2

    new_point = int(round(q1 * (dif1 / tot) + (q2 * (dif2 / tot))))
    return new_point


"""
def frame_types_probe(chunk: Chunk, q, ffmpeg_pipe, encoder, probing_rate, qp_file) -> CommandPair:

    probe_name = gen_probes_names(chunk, q).with_suffix('.ivf').as_posix()

    pipe = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', '-vf', f'select=not(mod(n\\,{probing_rate}))',
            *ffmpeg_pipe]

    params = ['x265', '--log-level', '0', '--no-progress',
              '--y4m', '--preset', 'fast', '--crf', f'{q}']

    cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    return cmd
"""
