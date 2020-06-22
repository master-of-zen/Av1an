#!/usr/bin/env python3

import concurrent
import concurrent.futures
import json
import numpy as np
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from subprocess import PIPE, STDOUT
from utils import *

class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encodes."""
    def __init__(self):
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

        # Make sure that vmaf calculated after encoding
        if self.vmaf_target:
            self.vmaf = True

        if self.vmaf_path:
            if not Path(self.vmaf_path).exists():
                print(f'No such model: {Path(self.vmaf_path).as_posix()}')
                terminate()

    def target_vmaf(self, source):
        # TODO speed up for vmaf stuff
        # TODO reduce complexity

        if self.vmaf_steps < 4:
            print('Target vmaf require more than 3 probes/steps')
            terminate()
        frames = frame_probe(source)
        probe = source.with_suffix(".mp4")

        try:
            # Making 4 fps probing file
            x264_probes(source, self.ffmpeg)

            # Making encoding fork
            fork = encoding_fork(self.min_cq, self.max_cq, self.vmaf_steps)

            # Making encoding commands
            cmd = vmaf_probes(probe, fork, self.ffmpeg_pipe)

            # Encoding probe and getting vmaf
            vmaf_cq = []
            for count, i in enumerate(cmd):
                subprocess.run(i[0], shell=True)

                v = call_vmaf(i[1], i[2], model=self.vmaf_path ,return_file=True)
                # Trying 25 percentile
                mean = read_vmaf_xml(v , 25)

                vmaf_cq.append((mean, i[3]))

                # Early Skip on big CQ
                if count == 0 and round(mean) > self.vmaf_target:
                    log(f"File: {source.stem}, Fr: {frames}\n" \
                        f"Probes: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n" \
                        f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                        f"Target CQ: {self.max_cq} Vmaf: {mean}\n\n")

                    return self.max_cq

                # Early Skip on small CQ
                if count == 1 and round(mean) < self.vmaf_target:
                    log(f"File: {source.stem}, Fr: {frames}\n" \
                        f"Probes: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n" \
                        f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                        f"Target CQ: {self.max_cq} Vmaf: {mean}\\n")
                    return self.min_cq

            x = [x[1] for x in sorted(vmaf_cq)]
            y = [float(x[0]) for x in sorted(vmaf_cq)]

            # Interpolate data
            cq, tl, f, xnew = interpolate_data(vmaf_cq, self.vmaf_target)

            if self.vmaf_plots:
                plot_probes(x, y, f, tl, self.min_cq, self.max_cq, probe, xnew, cq, frames, self.temp)

            log(f'File: {source.stem}, Fr: {frames}\n' \
                f'Probes: {sorted([x[1] for x in vmaf_cq])}\n' \
                f'Vmaf: {sorted([x[0] for x in vmaf_cq])}\n' \
                f'Target CQ: {int(cq[0])} Vmaf: {round(float(cq[1]), 2)}\n\n')

            return int(cq[0])

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Error in vmaf_target {e} \nAt line {exc_tb.tb_lineno}')
            terminate()

    def encode(self, commands):
        """Single encoder command queue and logging output."""
        commands, counter  = commands[0], commands[1]
        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = frame_probe(source)

            # Target Vmaf Mode
            if self.vmaf_target:
                tg_cq = self.target_vmaf(source)
                cm1 = man_cq(commands[0], tg_cq)

                if self.passes == 2:
                    cm2 = man_cq(commands[1], tg_cq)
                    commands = (cm1, cm2) + commands[2:]
                else:
                    commands = (cm1,) + commands[1:]

            # Boost
            if self.boost:
                commands, cq = boosting(self.boost_limit, self.boost_range, source, commands, self.passes)

            log(f'Enc: {source.name}, {frame_probe_source} fr\n')

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

                if total == 0:
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
                        print(f'Encoding error {e}\nAt line {exc_tb.tb_lineno}')
                        terminate()
        except KeyboardInterrupt:
            terminate()

    def video_encoding(self):
        """Encoding video on local machine."""
        self.output_file = outputs_filenames(self.input, self.output_file)

        if self.resume and (self.temp / 'done.json').exists():
            set_logging(self.logging, self.temp)
        else:
            setup(self.temp, self.resume)
            set_logging(self.logging, self.temp)

            framenums = split_routine(self.input, self.scenes, self.split_method, self.temp, self.min_scene_len, self.queue, self.threshold)

            if self.extra_split:
                framenums = extra_splits(input, framenums, self.extra_split)

            segment(self.input, self.temp, framenums)
            extract_audio(input, self.temp,  self.audio_params)

        chunk = get_video_queue(self.temp,  self.resume)

        # Make encode queue
        commands, self.video_params = compose_encoding_queue(chunk,  self.temp,  self.encoder,  self.video_params, self.ffmpeg_pipe,  self.passes)
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

        if self.vmaf:
            plot_vmaf(self.input, self.output_file, model=self.vmaf_path)

    def main_queue(self):
        tm = time.time()

        self.queue, self.input = process_inputs(self.input)

        if self.queue:
            for file in self.queue:
                tm = time.time()
                self.input = file
                print(f'Encoding: {file}')
                self.output_file = None
                self.video_encoding()
                print(f'Finished: {round(time.time() - tm, 1)}s\n')
        else:
            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s')

    def main_thread(self):
        """Main."""
        startup_check()
        self.conf()
        check_executables(self.encoder)
        self.main_queue()

if __name__ == '__main__':
    # Main thread
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()
