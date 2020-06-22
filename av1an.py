#!/usr/bin/env python3

import atexit
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

if sys.version_info < (3, 6):
    print('Python 3.6+ required')
    sys.exit()

if sys.platform == 'linux':
    def restore_term():
        os.system("stty sane")
    atexit.register(restore_term)


class Av1an:

    def __init__(self):
        """Av1an - Python framework for AV1, VP9, VP8 encodes."""
        self.d = dict()



    def config(self):
        """Creation and reading of config files with saved settings"""
        cfg = self.d.get('config')
        if cfg:
            if cfg.exists():
                with open(cfg) as f:
                    c: dict = dict(json.load(f))
                    self.d.update(c)

            else:
                with open(cfg, 'w') as f:
                    c = dict()
                    c['video_params'] = self.d.get('video_params')
                    c['encoder'] = self.d.get('encoder')
                    c['ffmpeg'] = self.d.get('ffmpeg')
                    c['audio_params'] = self.d.get('audio_params')
                    json.dump(c, f)

        # Changing pixel format, bit format
        self.d['pix_format'] = f'-strict -1 -pix_fmt {self.d.get("pix_format")}'
        self.d['ffmpeg_pipe'] = f' {self.d.get("ffmpeg")} {self.d.get("pix_format")} -f yuv4mpegpipe - |'

        # Make sure that vmaf calculated after encoding
        if self.d.get('vmaf_target'):
            self.d['vmaf'] = True

        if self.d.get("vmaf_path"):
            if not Path(self.d.get("vmaf_path")).exists():
                print(f'No such model: {Path(self.d.get("vmaf_path")).as_posix()}')
                terminate()

    def target_vmaf(self, source):
        # TODO speed up for vmaf stuff
        # TODO reduce complexity

        if self.d.get('vmaf_steps') < 4:
            print('Target vmaf require more than 3 probes/steps')
            terminate()

        vmaf_target = self.d.get('vmaf_target')
        min_cq, max_cq  = self.d.get('min_cq'), self.d.get('max_cq')
        steps = self.d.get('vmaf_steps')
        frames = frame_probe(source)
        probe = source.with_suffix(".mp4")
        vmaf_plots = self.d.get('vmaf_plots')
        ffmpeg = self.d.get('ffmpeg')

        try:
            # Making 4 fps probing file
            x264_probes(source, ffmpeg)

            # Making encoding fork
            fork = encoding_fork(min_cq, max_cq, steps)

            # Making encoding commands
            cmd = vmaf_probes(probe, fork, self.d.get('ffmpeg_pipe'))

            # Encoding probe and getting vmaf
            vmaf_cq = []
            for count, i in enumerate(cmd):
                subprocess.run(i[0], shell=True)

                v = call_vmaf(i[1], i[2], model=self.d.get('vmaf_path') ,return_file=True)
                # Trying 25 percentile
                mean = read_vmaf_xml(v , 25)

                vmaf_cq.append((mean, i[3]))

                # Early Skip on big CQ
                if count == 0 and round(mean) > vmaf_target:
                    log(f"File: {source.stem}, Fr: {frames}\n" \
                        f"Probes: {sorted([x[1] for x in vmaf_cq])}, Early Skip High CQ\n" \
                        f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                        f"Target CQ: {max_cq} Vmaf: {mean}\n\n")

                    return max_cq

                # Early Skip on small CQ
                if count == 1 and round(mean) < vmaf_target:
                    log(f"File: {source.stem}, Fr: {frames}\n" \
                        f"Probes: {sorted([x[1] for x in vmaf_cq])}, Early Skip Low CQ\n" \
                        f"Vmaf: {sorted([x[0] for x in vmaf_cq], reverse=True)}\n" \
                        f"Target CQ: {max_cq} Vmaf: {mean}\\n")
                    return min_cq

            x = [x[1] for x in sorted(vmaf_cq)]
            y = [float(x[0]) for x in sorted(vmaf_cq)]

            # Interpolate data
            cq, tl, f, xnew = interpolate_data(vmaf_cq, vmaf_target)

            if vmaf_plots:
                plot_probes(x, y, f, tl, min_cq, max_cq, probe, xnew, cq, frames, self.d.get('temp'))

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
        counter = commands[1]
        commands = commands[0]

        bl, br = self.d.get('boost_limit'), self.d.get('boost_range')
        encoder = self.d.get('encoder')
        passes = self.d.get('passes')
        boost = self.d.get('boost')
        vmaf_target = self.d.get('vmaf_target')

        try:
            st_time = time.time()
            source, target = Path(commands[-1][0]), Path(commands[-1][1])
            frame_probe_source = frame_probe(source)

            lg = f'Enc: {source.name}, {frame_probe_source} fr'

            # Target Vmaf Mode
            if vmaf_target:
                tg_cq = self.target_vmaf(source)
                cm1 = man_cq(commands[0], tg_cq)

                if passes == 2:
                    cm2 = man_cq(commands[1], tg_cq)
                    commands = (cm1, cm2) + commands[2:]
                else:
                    commands = (cm1,) + commands[1:]

            # Boost
            if boost:
                try:
                    commands, cq = boosting(bl, br, source, commands, passes)
                except Exception as e:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')
                lg = lg + f'[Boost]\nAvg brightness: {self.d.get("boost_range")}\nAdjusted CQ: {cq}\n'

            # Log additional function
            log(lg + '\n')

            # Queue execution
            for i in commands[:-1]:
                try:
                    tqdm_bar(i, encoder, counter, frame_probe_source, passes)
                except Exception as e:
                    _, _, exc_tb = sys.exc_info()
                    print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')

            frame_check(source, target, self.d.get('temp'), self.d.get('no_check'))

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
            enc_path = self.d.get('temp') / 'split'
            done_path = self.d.get('temp') / 'done.json'

            if self.d.get('resume') and done_path.exists():
                log('Resuming...\n')

                with open(done_path) as f:
                    data = json.load(f)

                total = data['total']
                done = len(data['done'])
                initial = sum(data['done'].values())

                log(f'Resumed with {done} encoded clips done\n\n')
            else:
                initial = 0
                total = frame_probe_fast(self.d.get('input'))

                if total == 0:
                    total = frame_probe(self.d.get('input'))

                d = {'total': total, 'done': {}}
                with open(done_path, 'w') as f:
                    json.dump(d, f)

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            w = min(self.d.get('workers'), clips)

            print(f'\rQueue: {clips} Workers: {w} Passes: {self.d.get("passes")}\n'
                  f'Params: {self.d.get("video_params").strip()}')

            with concurrent.futures.ThreadPoolExecutor(max_workers=self.d.get('workers')) as executor:
                counter = Manager().Counter(total, initial)
                future_cmd = {executor.submit(self.encode, (cmd, counter)): cmd for cmd in commands}
                for future in concurrent.futures.as_completed(future_cmd):
                    future_cmd[future]
                    try:
                        future.result()
                    except Exception as exc:
                        print(f'Encoding error: {exc}')
                        terminate()
        except KeyboardInterrupt:
            terminate()

    def video_encoding(self):
        """Encoding video on local machine."""
        passes, pipe, params = self.d.get('passes'), self.d.get("ffmpeg_pipe"), self.d.get("video_params")
        encoder, temp = self.d.get('encoder'), self.d.get('temp')
        video = self.d.get('input')
        scenes, split_method, threshold = self.d.get('scenes'), self.d.get('split_method'), self.d.get('threshold')
        xs, resume = self.d.get('extra_split'), self.d.get('resume')
        l = self.d.get('logging')
        min_scene_len, q = self.d.get('min_scene_len'), self.d.get('queue')

        audio_params = self.d.get('audio_params')

        self.d['output_file'] = outputs_filenames(self.d.get('input'), self.d.get('output'))

        if resume and (temp / 'done.json').exists():
            set_logging(l, temp)

        else:
            setup(temp, resume)

            set_logging(l, temp)

            # Splitting video and sorting big-first

            framenums = split_routine(video, scenes, split_method, temp, min_scene_len, q, threshold)

            if xs:
                framenums = extra_splits(input, framenums, xs )

            segment(video, temp, framenums)
            extract_audio(input, temp, audio_params)

        chunk = get_video_queue(temp, resume)

        # Make encode queue
        commands, params = compose_encoding_queue(chunk, temp, encoder, params, pipe, passes)
        self.d['video_params'] = params
        log(f'Encoding Queue Composed\n'
                 f'Encoder: {encoder.upper()} Queue Size: {len(commands)} Passes: {passes}\n'
                 f'Params: {params}\n\n')

        self.d['workers'] = determine_resources(encoder, self.d.get('workers'))

        self.encoding_loop(commands)

        try:
            concatenate_video(temp, self.d.get("output_file"), keep=self.d.get('keep'))

        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, FFmpeg error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
            log(f'Concatenation failed, aborting, error: {e}\n')
            terminate()

        if self.d.get("vmaf"):
            plot_vmaf(self.d.get('input'), self.d.get('output_file'), model=self.d.get('vmaf_path'))

    def main_queue(self):
        # Video Mode. Encoding on local machine
        tm = time.time()

        self.d['queue'], self.d['input'] = process_inputs(self.d.get('input'))

        if self.d.get('queue'):
            for file in self.d.get('queue'):
                tm = time.time()
                self.d['input'] = file
                print(f'Encoding: {file}')
                self.d['output_file'] = None
                self.video_encoding()
                print(f'Finished: {round(time.time() - tm, 1)}s\n')
        else:
            self.video_encoding()
            print(f'Finished: {round(time.time() - tm, 1)}s')

    def main_thread(self):
        """Main."""
        # Arg parse to main dictionary
        self.d = arg_parsing()

        # Read/Set parameters
        self.config()

        # Check all executables
        check_executables(self.d.get('encoder'))

        self.main_queue()

if __name__ == '__main__':
    # Main thread
    try:
        Av1an().main_thread()
    except KeyboardInterrupt:
        print('Encoding stopped')
        sys.exit()
