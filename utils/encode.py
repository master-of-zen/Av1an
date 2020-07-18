#!/usr/bin/env python3
import time
from pathlib import Path
from .target_vmaf import target_vmaf
from .boost import boosting
from .utils import frame_probe, frame_check
from .fp_reuse import remove_first_pass_from_commands
from .utils import man_q
from .logger import log
from .bar import Manager, tqdm_bar
import sys

def encode(commands):
    """Main encoding flow, appliying all scene based features

    :param args: Arguments object
    :param commands: composed commands for encoding
    :type commands: list
    """
    commands, counter, args  = commands
    try:
        st_time = time.time()
        source, target = Path(commands[-1][0]), Path(commands[-1][1])
        frame_probe_source = frame_probe(source)

        # Target Vmaf Mode
        if args.vmaf_target:
            tg_cq = target_vmaf(source, args)
            cm1 = man_q(commands[0], tg_cq, )

            if args.passes == 2:
                cm2 = man_q(commands[1], tg_cq)
                commands = (cm1, cm2) + commands[2:]
            else:
                commands = (cm1,) + commands[1:]

        # Boost
        if args.boost:
            commands = boosting(args.boost_limit, args.boost_range, source, commands, args.passes)

        # first pass reuse
        if args.reuse_first_pass:
            commands = remove_first_pass_from_commands(commands, args.passes)

        log(f'Enc: {source.name}, {frame_probe_source} fr\n\n')

        # Queue execution
        for i in commands[:-1]:
                tqdm_bar(i, args.encoder, counter, frame_probe_source, args.passes)

        frame_check(source, target, args.temp, args.no_check)

        frame_probe_fr = frame_probe(target)

        enc_time = round(time.time() - st_time, 2)

        log(f'Done: {source.name} Fr: {frame_probe_fr}\n'
            f'Fps: {round(frame_probe_fr / enc_time, 4)} Time: {enc_time} sec.\n\n')
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')