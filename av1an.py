#!/usr/bin/env python3

import concurrent
import concurrent.futures
import shutil
import subprocess
import sys
import time
from pathlib import Path
import json

from utils import *


class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encoding"""
    def __init__(self):
        self.args = arg_parsing()

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        try:
            enc_path = self.args.temp / 'split'
            done_path = self.args.temp / 'done.json'

            if self.args.resume and done_path.exists():
                log('Resuming...\n')

                with open(done_path) as f:
                    data = json.load(f)

                total = data['total']
                done = len(data['done'])
                initial = sum(data['done'].values())

                log(f'Resumed with {done} encoded clips done\n\n')
            else:
                initial = 0
                total = frame_probe_fast(self.args.input)

                if total < 1:
                    total = frame_probe(self.args.input)

                d = {'total': total, 'done': {}}
                with open(done_path, 'w') as f:
                    json.dump(d, f)

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            self.args.workers = min(self.args.workers, clips)

            print(f'\rQueue: {clips} Workers: {self.args.workers} Passes: {self.args.passes}\n'
                  f'Params: {self.args.video_params.strip()}')

            with concurrent.futures.ThreadPoolExecutor(max_workers=self.args.workers) as executor:
                counter = Manager().Counter(total, initial)
                future_cmd = {executor.submit(encode, (cmd, counter, self.args)): cmd for cmd in commands}
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

    def video_encoding(self):
        """Encoding video on local machine."""

        self.args.output_file = outputs_filenames(self.args.input, self.args.output_file, self.args.encoder )

        if self.args.resume and (self.args.temp / 'done.json').exists():
            set_log(self.args.logging, self.args.temp)
        else:
            setup(self.args.temp, self.args.resume)
            set_log(self.args.logging, self.args.temp)

            # inherit video params from aom encode unless we are using a different encoder, then use defaults
            aom_keyframes_params = self.args.video_params if (self.args.encoder == 'aom') else AOM_KEYFRAMES_DEFAULT_PARAMS
            framenums = split_routine(self.args, aom_keyframes_params)

            if self.args.extra_split:
                framenums = extra_splits(self.args.input, framenums, self.args.extra_split)

            if self.args.reuse_first_pass:
                segment_first_pass(self.args.temp, framenums)

            segment(self.args.input, self.args.temp, framenums)
            extract_audio(self.args.input, self.args.temp,  self.args.audio_params)

        chunk = get_video_queue(self.args.temp,  self.args.resume)

        # Make encode queue
        commands = compose_encoding_queue(chunk, self.args.temp, self.args.encoder, self.args.video_params, self.args.ffmpeg_pipe, self.args.passes)

        self.args.workers = determine_resources(self.args.encoder, self.args.workers)

        self.encoding_loop(commands)

        try:
            concatenate_video(self.args.temp, self.args.output_file, self.args.encoder )

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, FFmpeg error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
            log(f'Concatenation failed, aborting, error: {e}\n')
            terminate()

        if self.args.vmaf or self.args.vmaf_plots:
            plot_vmaf(self.args.input, self.args.output_file, model=self.args.vmaf_path)
        # Delete temp folders
        if not self.args.keep:
            shutil.rmtree(self.args.temp)

    def main_queue(self):
        # Todo: Redo Queue
        tm = time.time()

        self.args.queue = process_inputs(self.args.input)

        for file in self.args.queue:
            tm = time.time()
            self.args.input = file

            if len(self.args.queue) > 1:
                print(f'Encoding: {file}')
                self.args.output_file = None

            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s\n')

    def main_thread(self):
        """Main."""
        startup_check()
        conf(self.args)
        check_executables(self.args.encoder)
        self.main_queue()


def main():
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


if __name__ == '__main__':
    main()
