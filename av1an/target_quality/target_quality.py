import subprocess

from math import isnan
from math import log as ln
import numpy as np
import re
import pprint
from scipy import interpolate

from av1an.vmaf import VMAF
from av1an.logger import log
from av1an.commandtypes import CommandPair, Command
from av1an.project import Project
from av1an.chunk import Chunk
from av1an.manager.Pipes import process_pipe
try:
    import matplotlib
    from matplotlib import pyplot as plt
except ImportError:
    matplotlib = None
    plt = None

# TODO: rework to class, account for dark scenes/banding

def per_shot_target_quality_routine(project: Project, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param project: the Project
    :param chunk: the Chunk
    :return: None
    """
    chunk.per_shot_target_quality_cq = per_shot_target_quality(chunk, project)


def get_scene_scores(chunk, ffmpeg_pipe):
    """
    Run ffmpeg scenedetection filter to get average amount of motion in scene
    """

    pipecmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', *ffmpeg_pipe]

    params = ['ffmpeg', '-hide_banner', '-i', '-', '-vf', 'fps=fps=5,scale=\'min(960,iw)\':-1,hqdn3d=4:4:0:0,select=\'gte(scene,0)\',metadata=print', '-f', 'null', '-']
    cmd = CommandPair(pipecmd, [*params])
    pipe = make_pipes(chunk.ffmpeg_gen_cmd, cmd)

    history = []

    while True:
        line = pipe.stdout.readline().strip()
        if len(line) == 0 and pipe.poll() is not None:
            break
        if len(line) == 0:
            continue
        if line:
            history.append(line)

    if pipe.returncode != 0 and pipe.returncode != -2:
        print(f"\n:: Encoder in getting scene score {pipe.returncode}")
        print(f"\n:: Chunk: {chunk.index}")
        print('\n'.join(history))

    pp = pprint.PrettyPrinter(indent=2).pprint

    scores = [x for x in history if 'score' in x]

    results = []
    for x in scores:
        matches = re.findall(r"=\s*([\S\s]+)", x)
        var = float(matches[-1])
        if var < 0.3:
            results.append(var)

    result = (round(np.average(results), 4))

    pp(result * 1000)


def adapt_probing_rate(rate, frames):
    """
    Change probing rate depending on amount of frames in scene.
    Ensure that low frame count scenes get decent amount of probes

    :param rate: given rate of probing
    :param frames: amount of frames in scene
    :return: new probing rate
    """

    #Todo: Make it depend on amount of motion in scene

    #For current moment 4 for everything

    if frames > 0:
        return 4

    if frames < 40:
        return 4
    elif frames < 120:
        return 8
    elif frames <= 240:
        return 10
    elif frames > 240:
        return 16


def get_target_q(scores, target_quality):
    """
    Interpolating scores to get Q closest to target
    Interpolation type for 2 probes changes to linear
    """
    x = [x[1] for x in sorted(scores)]
    y = [float(x[0]) for x in sorted(scores)]

    if len(x) > 2:
        interpolation = 'quadratic'
    else:
        interpolation = 'linear'
    f = interpolate.interp1d(x, y, kind=interpolation)
    xnew = np.linspace(min(x), max(x), max(x) - min(x))
    tl = list(zip(xnew, f(xnew)))
    q = min(tl, key=lambda l: abs(l[1] - target_quality))

    return int(q[0]), round(q[1], 3)


def weighted_search(num1, vmaf1, num2, vmaf2, target):
    """
    Returns weighted value closest to searched

    :param num1: Q of first probe
    :param vmaf1: VMAF of first probe
    :param num2: Q of second probe
    :param vmaf2: VMAF of first probe
    :param target: VMAF target
    :return: Q for new probe
    """

    dif1 = abs(VMAF.transform_vmaf(target) - VMAF.transform_vmaf(vmaf2))
    dif2 = abs(VMAF.transform_vmaf(target) - VMAF.transform_vmaf(vmaf1))

    tot = dif1 + dif2

    new_point = int(round(num1 * (dif1 / tot) + (num2 * (dif2 / tot))))
    return new_point


def probe_cmd(chunk: Chunk, q, ffmpeg_pipe, encoder, probing_rate, n_threads) -> CommandPair:
    """
    Generate and return commands for probes at set Q values
    These are specifically not the commands that are generated
    by the user or encoder defaults, since these
    should be faster than the actual encoding commands.
    These should not be moved into encoder classes at this point.
    """
    pipe = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', '-', '-vf',
            f'select=not(mod(n\\,{probing_rate}))', *ffmpeg_pipe]

    probe_name = gen_probes_names(chunk, q).with_suffix('.ivf').as_posix()

    if encoder == 'aom':
        params = ['aomenc', '--passes=1', f'--threads={n_threads}', '--tile-columns=1',
                  '--end-usage=q', '-b', '8', '--cpu-used=6', f'--cq-level={q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'x265':
        params = ['x265', '--log-level', '0', '--no-progress',
                  '--y4m', '--frame-threads', f'{n_threads}', '--preset', 'fast', '--crf', f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'rav1e':
        params = ['rav1e', '-y', '-s', '10', '--threads', f'{n_threads}', '--tiles', '32', '--quantizer', f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'vpx':
        params = ['vpxenc', '-b', '10', '--profile=2','--passes=1', '--pass=1', '--codec=vp9',
                  f'--threads={n_threads}', '--cpu-used=9', '--end-usage=q',
                  f'--cq-level={q}', '--row-mt=1']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    elif encoder == 'svt_av1':
        params = ['SvtAv1EncApp', '-i', 'stdin', '--lp', f'{n_threads}',
                  '--preset', '8', '--rc', '0', '--qp', f'{q}']
        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])

    elif encoder == 'svt_vp9':
        params = ['SvtVp9EncApp', '-i', 'stdin', '--lp', f'{n_threads}',
                  '-enc-mode', '8', '-q', f'{q}']
        # TODO: pipe needs to output rawvideo
        cmd = CommandPair(pipe, [*params, '-b', probe_name, '-'])

    elif encoder == 'x264':
        params = ['x264', '--log-level', 'error', '--demuxer', 'y4m',
                  '-', '--no-progress', '--threads', f'{n_threads}', '--preset', 'medium', '--crf',
                  f'{q}']
        cmd = CommandPair(pipe, [*params, '-o', probe_name, '-'])

    return cmd


def gen_probes_names(chunk: Chunk, q):
    """Make name of vmaf probe
    """
    return chunk.fake_input_path.with_name(f'v_{q}{chunk.name}').with_suffix('.ivf')


def make_pipes(ffmpeg_gen_cmd: Command, command: CommandPair):

    ffmpeg_gen_pipe = subprocess.Popen(ffmpeg_gen_cmd,
                                        stdout=subprocess.PIPE,
                                        stderr=subprocess.STDOUT)

    ffmpeg_pipe = subprocess.Popen(command[0],
                                    stdin=ffmpeg_gen_pipe.stdout,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT)

    pipe = subprocess.Popen(command[1],
                            stdin=ffmpeg_pipe.stdout,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            universal_newlines=True)

    return pipe


def vmaf_probe(chunk: Chunk, q,  project: Project, probing_rate):
    """
    Calculates vmaf and returns path to json file

    :param chunk: the Chunk
    :param q: Value to make probe
    :param project: the Project
    :param probing_rate: 1 out of every N frames should be encoded for analysis
    :return : path to json file with vmaf scores
    """

    n_threads = project.n_threads if project.n_threads else 12
    cmd = probe_cmd(chunk, q, project.ffmpeg_pipe, project.encoder, probing_rate, n_threads)
    pipe = make_pipes(chunk.ffmpeg_gen_cmd, cmd)
    process_pipe(pipe, chunk)
    vm = VMAF(n_threads=project.n_threads, model=project.vmaf_path, res=project.vmaf_res, vmaf_filter=project.vmaf_filter)
    file = vm.call_vmaf(chunk, gen_probes_names(chunk, q), vmaf_rate=probing_rate)
    return file


def get_closest(q_list, q, positive=True):
    """
    Returns closest value from the list, ascending or descending

    :param q_list: list of q values that been already used
    :param q:
    :param positive: search direction, positive - only values bigger than q
    :return: q value from list
    """
    if positive:
        q_list = [x for x in q_list if x > q]
    else:
        q_list = [x for x in q_list if x < q]

    return min(q_list, key=lambda x: abs(x - q))


def interpolate_data(vmaf_cq: list, target_quality):
    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    # Interpolate data
    f = interpolate.interp1d(x, y, kind='quadratic')
    xnew = np.linspace(min(x), max(x), max(x) - min(x))

    # Getting value closest to target
    tl = list(zip(xnew, f(xnew)))
    target_quality_cq = min(tl, key=lambda l: abs(l[1] - target_quality))
    return target_quality_cq, tl, f, xnew


def plot_probes(project, vmaf_cq, chunk: Chunk, frames):
    if plt is None:
        log(f'Matplotlib is not installed or could not be loaded. Unable to plot probes.')
        return
    # Saving plot of vmaf calculation

    x = [x[1] for x in sorted(vmaf_cq)]
    y = [float(x[0]) for x in sorted(vmaf_cq)]

    cq, tl, f, xnew = interpolate_data(vmaf_cq, project.target_quality)
    matplotlib.use('agg')
    plt.ioff()
    plt.plot(xnew, f(xnew), color='tab:blue', alpha=1)
    plt.plot(x, y, 'p', color='tab:green', alpha=1)
    plt.plot(cq[0], cq[1], 'o', color='red', alpha=1)
    plt.grid(True)
    plt.xlim(project.min_q, project.max_q)
    vmafs = [int(x[1]) for x in tl if isinstance(x[1], float) and not isnan(x[1])]
    plt.ylim(min(vmafs), max(vmafs) + 1)
    plt.ylabel('VMAF')
    plt.title(f'Chunk: {chunk.name}, Frames: {frames}')
    plt.xticks(np.arange(project.min_q, project.max_q + 1, 1.0))
    temp = project.temp / chunk.name
    plt.savefig(f'{temp}.png', dpi=200, format='png')
    plt.close()


def per_shot_target_quality(chunk: Chunk, project: Project):
    vmaf_cq = []
    frames = chunk.frames

    # get_scene_scores(chunk, project.ffmpeg_pipe)

    # Adapt probing rate
    if project.probing_rate in (1,2):
        probing_rate = project.probing_rate
    else:
        probing_rate = adapt_probing_rate(project.probing_rate, frames)

    q_list = []
    score = 0

    # Make middle probe
    middle_point = (project.min_q + project.max_q) // 2
    q_list.append(middle_point)
    last_q = middle_point

    score = VMAF.read_weighted_vmaf(vmaf_probe(chunk, last_q, project, probing_rate))
    vmaf_cq.append((score, last_q))

    if project.probes < 3:
        #Use Euler's method with known relation between cq and vmaf
        vmaf_cq_deriv = -0.18
        ## Formula -ln(1-score/100) = vmaf_cq_deriv*last_q + constant
        #constant = -ln(1-score/100) - vmaf_cq_deriv*last_q
        ## Formula -ln(1-project.vmaf_target/100) = vmaf_cq_deriv*cq + constant
        #cq = (-ln(1-project.vmaf_target/100) - constant)/vmaf_cq_deriv
        next_q = int(round(last_q + (VMAF.transform_vmaf(project.target_quality) - VMAF.transform_vmaf(score))/vmaf_cq_deriv))

        #Clamp
        if next_q < project.min_q:
            next_q = project.min_q
        if project.max_q < next_q:
            next_q = project.max_q

        #Single probe cq guess or exit to avoid divide by zero
        if project.probes == 1 or next_q == last_q:
            return next_q

        #Second probe at guessed value
        score_2 = VMAF.read_weighted_vmaf(vmaf_probe(chunk, next_q, project, probing_rate))

        #Calculate slope
        vmaf_cq_deriv = (VMAF.transform_vmaf(score_2) - VMAF.transform_vmaf(score)) / (next_q-last_q)

        #Same deal different slope
        next_q = int(round(next_q+(VMAF.transform_vmaf(project.target_quality)-VMAF.transform_vmaf(score_2))/vmaf_cq_deriv))

        #Clamp
        if next_q < project.min_q:
            next_q = project.min_q
        if project.max_q < next_q:
            next_q = project.max_q

        return next_q

    # Initialize search boundary
    vmaf_lower = score
    vmaf_upper = score
    vmaf_cq_lower = last_q
    vmaf_cq_upper = last_q

    # Branch
    if score < project.target_quality:
        next_q = project.min_q
        q_list.append(project.min_q)
    else:
        next_q = project.max_q
        q_list.append(project.max_q)

    # Edge case check
    score = VMAF.read_weighted_vmaf(vmaf_probe(chunk, next_q, project, probing_rate))
    vmaf_cq.append((score, next_q))

    if next_q == project.min_q and score < project.target_quality:
        log(f"Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} VMAF: {round(vmaf_cq[-1][0], 2)}\n\n")
        return next_q

    elif next_q == project.max_q and score > project.target_quality:
        log(f"Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n"
            f"Q: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n"
            f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n"
            f"Target Q: {vmaf_cq[-1][1]} VMAF: {round(vmaf_cq[-1][0], 2)}\n\n")
        return next_q

    # Set boundary
    if score < project.target_quality:
        vmaf_lower = score
        vmaf_cq_lower = next_q
    else:
        vmaf_upper = score
        vmaf_cq_upper = next_q

    # VMAF search
    for _ in range(project.probes - 2):
        new_point = weighted_search(vmaf_cq_lower, vmaf_lower, vmaf_cq_upper, vmaf_upper, project.target_quality)
        if new_point in [x[1] for x in vmaf_cq]:
            break

        q_list.append(new_point)
        score = VMAF.read_weighted_vmaf(vmaf_probe(chunk, new_point, project, probing_rate))
        vmaf_cq.append((score, new_point))

        # Update boundary
        if score < project.target_quality:
            vmaf_lower = score
            vmaf_cq_lower = new_point
        else:
            vmaf_upper = score
            vmaf_cq_upper = new_point

    q, q_vmaf = get_target_q(vmaf_cq, project.target_quality)

    log(f'Chunk: {chunk.name}, Rate: {probing_rate}, Fr: {frames}\n'
        f'Q: {sorted([x[1] for x in vmaf_cq])}\n'
        f'Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n'
        f'Target Q: {q} VMAF: {round(q_vmaf, 2)}\n\n')

    # Plot Probes
    if project.vmaf_plots and len(vmaf_cq) > 3:
        plot_probes(project, vmaf_cq, chunk, frames)

    return q


def per_frame_target_quality_routine(project: Project, chunk: Chunk):
    """
    Applies per_shot_target_quality to this chunk. Determines what the cq value should be and sets the
    per_shot_target_quality_cq for this chunk

    :param project: the Project
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
    vm = VMAF(n_threads=project.n_threads, model=project.vmaf_path, res=project.vmaf_res, vmaf_filter=project.vmaf_filter)
    fl = vm.call_vmaf(chunk, gen_probes_names(chunk, q))
    jsn = VMAF.read_json(fl)
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
