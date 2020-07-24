#!/usr/bin/env python3
import time
import json
import os
from pathlib import Path
from .target_vmaf import target_vmaf
from .boost import boosting
from .utils import  frame_probe_cv2, terminate, process_inputs
from .fp_reuse import remove_first_pass_from_commands
from .utils import man_q
from .logger import log
from .bar import Manager, tqdm_bar
from .setup import (startup_check, determine_resources, outputs_filenames, setup)
from .aom_kf import AOM_KEYFRAMES_DEFAULT_PARAMS
import sys
import concurrent
import concurrent.futures
from .logger import log, set_log
from .config  import conf
from .compose import compose_encoding_queue, get_video_queue
from .ffmpeg import concatenate_video, extract_audio, frame_probe, frame_check
from .fp_reuse import segment_first_pass
from .split import extra_splits, segment, split_routine
from .vmaf import plot_vmaf
import shutil
from .vvc import to_yuv, vvc_concat


def video_encoding(args):
        """Encoding video on local machine."""

        args.output_file = outputs_filenames(args.input, args.output_file, args.encoder )

        if args.resume and (args.temp / 'done.json').exists():
            set_log(args.logging, args.temp)
        else:
            setup(args.temp, args.resume)
            set_log(args.logging, args.temp)

            # inherit video params from aom encode unless we are using a different encoder, then use defaults
            aom_keyframes_params = args.video_params if (args.encoder == 'aom') else AOM_KEYFRAMES_DEFAULT_PARAMS
            framenums = split_routine(args, aom_keyframes_params)

            if args.extra_split:
                framenums = extra_splits(args.input, framenums, args.extra_split)

            if args.reuse_first_pass:
                segment_first_pass(args.temp, framenums)

            segment(args.input, args.temp, framenums)
            extract_audio(args.input, args.temp,  args.audio_params)

        chunk = get_video_queue(args.temp,  args.resume)

        # Make encode queue
        commands = compose_encoding_queue(chunk, args)

        args.workers = determine_resources(args.encoder, args.workers)

        encoding_loop(args, commands)

        try:
            if args.encoder == 'vvc':
                vvc_concat(args.temp, args.output_file.with_suffix('.h266'))
            else:
                concatenate_video(args.temp, args.output_file, args.encoder )

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, FFmpeg error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
            log(f'Concatenation failed, aborting, error: {e}\n')
            terminate()

        if args.vmaf or args.vmaf_plots:
            plot_vmaf(args.input, args.output_file, args.vmaf_path)
        # Delete temp folders
        if not args.keep:
            shutil.rmtree(args.temp)


def main_queue(args):
    # Todo: Redo Queue
    try:
        conf(args)
        tm = time.time()

        args.queue = process_inputs(args.input)

        for file in args.queue:
            tm = time.time()
            args.input = file

            if len(args.queue) > 1:
                print(f'Encoding: {file}')
                args.output_file = None

            video_encoding(args)
            print(f'Finished: {round(time.time() - tm, 1)}s\n')
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


def encoding_loop(args, commands):
        """Creating process pool for encoders, creating progress bar."""
        try:
            enc_path = args.temp / 'split'
            done_path =args.temp / 'done.json'

            if args.resume and done_path.exists():
                log('Resuming...\n')

                with open(done_path) as f:
                    data = json.load(f)

                total = data['total']
                done = len(data['done'])
                initial = sum(data['done'].values())

                log(f'Resumed with {done} encoded clips done\n\n')
            else:
                initial = 0
                total = frame_probe_cv2(args.input)

                if total < 1:
                    total = frame_probe(args.input)

                d = {'total': total, 'done': {}}
                with open(done_path, 'w') as f:
                    json.dump(d, f)

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            args.workers = min(args.workers, clips)

            print(f'\rQueue: {clips} Workers: {args.workers} Passes: {args.passes}\n'
                  f'Params: {args.video_params.strip()}')

            with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as executor:
                counter = Manager().Counter(total, initial)
                future_cmd = {executor.submit(encode, (cmd, counter, args)): cmd for cmd in commands}
                for future in concurrent.futures.as_completed(future_cmd):
                    future_cmd[future]
                    try:
                        future.result()
                    except Exception as exc:
                        _, _, exc_tb = sys.exc_info()
                        print(f'Encoding error {exc}\nAt line {exc_tb.tb_lineno}')
                        terminate()
        except KeyboardInterrupt:
            terminate()


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
                if args.encoder == 'vvc':
                    log(f'Creating yuv for file {commands[1][0]}\n')
                    fl = to_yuv(commands[1][0])
                    log(f'Created yuv for file {commands[1][0]}\n')
                tqdm_bar(i, args.encoder, counter, frame_probe_source, args.passes)

                if args.encoder == 'vvc':
                    os.remove(fl)

        frame_check(source, target, args.temp, args.no_check)

        if args.encoder == 'vvc':
            frame_probe_fr = frame_probe(source)
        else:
             frame_probe_fr = frame_probe(target)

        enc_time = round(time.time() - st_time, 2)

        log(f'Done: {source.name} Fr: {frame_probe_fr}\n'
            f'Fps: {round(frame_probe_fr / enc_time, 4)} Time: {enc_time} sec.\n\n')

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')
