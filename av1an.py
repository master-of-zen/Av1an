#!/usr/bin/env python3

import concurrent
import concurrent.futures
import json
import subprocess
import sys
import time
from pathlib import Path

from utils import *


class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encoding"""
    def __init__(self):

        # Input/Output/Temp
        self.input = None
        self.temp = None
        self.output_file = None

        # Splitting
        self.scenes = None
        self.split_method = None
        self.extra_split = None
        self.min_scene_len = None

        # PySceneDetect split
        self.threshold = None

        # AOM Keyframe split
        self.reuse_first_pass = None

        # Encoding
        self.passes = None
        self.video_params = None
        self.encoder = None
        self.workers = None
        self.config = None

        self.video_params = None

        # FFmpeg params
        self.ffmpeg = None
        self.audio_params = None
        self.pix_format = None

        # Misc
        self.logging = None
        self.resume = None
        self.no_check = None
        self.keep = None

        # Boost
        self.boost = None
        self.boost_range = None
        self.boost_limit = None

        # Vmaf
        self.vmaf = None
        self.vmaf_path = None

        # Target Vmaf
        self.vmaf_target = None
        self.vmaf_steps = None
        self.min_cq = None
        self.max_cq = None
        self.vmaf_plots = None
        self.n_threads = None

        # get all values from argparse
        self.__dict__.update(arg_parsing())

    def conf(self):
        """Creation and reading of config files with saved settings"""
        if self.config:
            if self.config.exists():
                with open(self.config) as f:
                    c: dict = dict(json.load(f))
                    self.__dict__.update(c)

            else:
                with open(self.config, 'w') as f:
                    c = dict()
                    c['video_params'] = self.video_params
                    c['encoder'] = self.encoder
                    c['ffmpeg'] = self.ffmpeg
                    c['audio_params'] = self.audio_params
                    json.dump(c, f)

        # Changing pixel format, bit format
        self.pix_format = f'-strict -1 -pix_fmt {self.pix_format}'
        self.ffmpeg_pipe = f' {self.ffmpeg} {self.pix_format} -f yuv4mpegpipe - |'

        if self.vmaf_path:
            if not Path(self.vmaf_path).exists():
                print(f'No such model: {Path(self.vmaf_path).as_posix()}')
                terminate()

        if self.reuse_first_pass and self.encoder != 'aom' and self.split_method != 'aom_keyframes':
            print('Reusing the first pass is only supported with the aom encoder and aom_keyframes split method.')
            terminate()

        if self.video_params is None:
            self.video_params = get_default_params_for_encoder(self.encoder)

    def encode(self, commands):
        """Single encoder command queue and logging output."""
        commands, counter  = commands[0], commands[1]
        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = frame_probe(source)

            # Target Vmaf Mode
            if self.vmaf_target:
                tg_cq = target_vmaf(source, self.temp, self.vmaf_path, self.vmaf_steps, self.min_cq, self.max_cq, self.ffmpeg, self.ffmpeg_pipe, self.n_threads, self.vmaf_target, self.vmaf_plots)
                cm1 = man_cq(commands[0], tg_cq)

                if self.passes == 2:
                    cm2 = man_cq(commands[1], tg_cq)
                    commands = (cm1, cm2) + commands[2:]
                else:
                    commands = (cm1,) + commands[1:]

            # Boost
            if self.boost:
                commands = boosting(self.boost_limit, self.boost_range, source, commands, self.passes)

            # first pass reuse
            if self.reuse_first_pass:
                commands = remove_first_pass_from_commands(commands, self.passes)

            log(f'Enc: {source.name}, {frame_probe_source} fr\n\n')

            # Queue execution
            for i in commands[:-1]:
                    tqdm_bar(i, self.encoder, counter, frame_probe_source, self.passes)

            frame_check(source, target, self.temp, self.no_check)

            frame_probe_fr = frame_probe(target)

            enc_time = round(time.time() - st_time, 2)

            log(f'Done: {source.name} Fr: {frame_probe_fr}\n'
                f'Fps: {round(frame_probe_fr / enc_time, 4)} Time: {enc_time} sec.\n\n')
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        try:
            enc_path = self.temp / 'split'
            done_path = self.temp / 'done.json'

            if self.resume and done_path.exists():
                log('Resuming...\n')

                with open(done_path) as f:
                    data = json.load(f)

                total = data['total']
                done = len(data['done'])
                initial = sum(data['done'].values())

                log(f'Resumed with {done} encoded clips done\n\n')
            else:
                initial = 0
                total = frame_probe_fast(self.input)

                if total < 1:
                    total = frame_probe(self.input)

                d = {'total': total, 'done': {}}
                with open(done_path, 'w') as f:
                    json.dump(d, f)

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            self.workers = min(self.workers, clips)

            print(f'\rQueue: {clips} Workers: {self.workers} Passes: {self.passes}\n'
                  f'Params: {self.video_params.strip()}')

            with concurrent.futures.ThreadPoolExecutor(max_workers=self.workers) as executor:
                counter = Manager().Counter(total, initial)
                future_cmd = {executor.submit(self.encode, (cmd, counter)): cmd for cmd in commands}
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

        self.output_file = outputs_filenames(self.input, self.output_file, self.encoder )

        if self.resume and (self.temp / 'done.json').exists():
            set_log(self.logging, self.temp)
        else:
            setup(self.temp, self.resume)
            set_log(self.logging, self.temp)

            # inherit video params from aom encode unless we are using a different encoder, then use defaults
            aom_keyframes_params = self.video_params if (self.encoder == 'aom') else AOM_KEYFRAMES_DEFAULT_PARAMS
            framenums = split_routine(self.input, self.scenes, self.split_method, self.temp, self.min_scene_len, self.threshold, self.ffmpeg_pipe, aom_keyframes_params)

            if self.extra_split:
                framenums = extra_splits(self.input, framenums, self.extra_split)

            if self.reuse_first_pass:
                segment_first_pass(self.temp, framenums)

            segment(self.input, self.temp, framenums)
            extract_audio(self.input, self.temp,  self.audio_params)

        chunk = get_video_queue(self.temp,  self.resume)

        # Make encode queue
        commands = compose_encoding_queue(chunk, self.temp, self.encoder, self.video_params, self.ffmpeg_pipe, self.passes)
        log(f'Encoding Queue Composed\n'
            f'Encoder: {self.encoder.upper()} Queue Size: {len(commands)} Passes: {self.passes}\n'
            f'Params: {self.video_params}\n\n')

        self.workers = determine_resources(self.encoder, self.workers)

        self.encoding_loop(commands)

        try:
            concatenate_video(self.temp, self.output_file, keep=self.keep)

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, FFmpeg error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
            log(f'Concatenation failed, aborting, error: {e}\n')
            terminate()

        if self.vmaf or self.vmaf_plots:
            plot_vmaf(self.input, self.output_file, model=self.vmaf_path)

    def main_queue(self):
        # Todo: Redo Queue
        tm = time.time()

        self.queue = process_inputs(self.input)

        for file in self.queue:
            tm = time.time()
            self.input = file

            if len(self.queue) > 1:
                print(f'Encoding: {file}')
                self.output_file = None

            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s\n')

    def main_thread(self):
        """Main."""
        startup_check()
        self.conf()
        check_executables(self.encoder)
        self.main_queue()


def main():
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()


if __name__ == '__main__':
    main()
