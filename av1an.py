#!/usr/bin/env python3

import time
from tqdm import tqdm
import sys
import os
import shutil
from distutils.spawn import find_executable
from ast import literal_eval
from psutil import virtual_memory
import argparse
from multiprocessing import Pool
import multiprocessing
import subprocess
from pathlib import Path
from typing import Optional

import cv2
import statistics

from scenedetect.video_manager import VideoManager
from scenedetect.scene_manager import SceneManager
from scenedetect.detectors import ContentDetector


if sys.version_info < (3, 7):
    print('Av1an requires at least Python 3.7 to run.')
    sys.exit()


class Av1an:

    def __init__(self):
        """Av1an - Python wrapper for AV1 encoders."""
        self.temp_dir = Path('.temp')
        self.FFMPEG = 'ffmpeg -y -hide_banner -loglevel error'
        self.pix_format = 'yuv420p'
        self.encoder = 'aom'
        self.passes = 2
        self.threshold = 30
        self.workers = 0
        self.mode = 0
        self.ffmpeg_pipe = None
        self.ffmpeg = None
        self.logging = None
        self.args = None
        self.video_params = ''
        self.output_file: Optional[Path] = None
        self.pyscene = ''
        self.scenes: Optional[Path] = None
        self.skip_scenes = False

    def log(self, info):
        """Default logging function, write to file."""
        with open(self.logging, 'a') as log:
            log.write(time.strftime('%X') + ' ' + info)

    def call_cmd(self, cmd, capture_output=False):
        """Calling system shell, if capture_output=True output string will be returned."""
        if capture_output:
            return subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).stdout

        with open(self.logging, 'a') as log:
            subprocess.run(cmd, shell=True, stdout=log, stderr=log)

    def arg_parsing(self):
        """Command line parse and assigning defined and user defined params."""
        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=self.mode, help='Mode 0 - video, Mode 1 - image')
        parser.add_argument('--video_params', '-v', type=str, default=self.video_params, help='encoding settings')
        parser.add_argument('--file_path', '-i', type=Path, help='Input File', required=True)
        parser.add_argument('--encoder', '-enc', type=str, default=self.encoder, help='Choosing encoder')
        parser.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=float, default=self.threshold, help='PySceneDetect Threshold')
        parser.add_argument('--temp', type=Path, default=self.temp_dir, help='Set temp folder path')
        parser.add_argument('--logging', '-log', type=str, default=self.logging, help='Enable logging')
        parser.add_argument('--passes', '-p', type=int, default=self.passes, help='Specify encoding passes')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')
        parser.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
        parser.add_argument('--pix_format', '-fmt', type=str, default=self.pix_format, help='FFmpeg pixel format')
        parser.add_argument('--scenes', '-s', type=str, default=self.scenes, help='File location for scenes')
        parser.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
        parser.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')
        parser.add_argument('--keep', help='Keep temporally folder after encode', action='store_true')
        parser.add_argument('--boost', help='Experimental feature', action='store_true')
        parser.add_argument('-br', default=15, type=int, help='Range/strenght of CQ change')
        parser.add_argument('-bl', default=10, type=int, help='CQ limit for boosting')
        parser.add_argument('--vmaf', help='Calculating vmaf after encode', action='store_true')
        parser.add_argument('--vmaf_path', type=str, default='', help='Path to vmaf models')

        # Pass command line args that were passed
        self.args = parser.parse_args()

        # Set temp dir
        self.temp_dir = self.args.temp

        # Set scenes if provided
        if self.args.scenes:
            scenes = self.args.scenes.strip()
            if scenes == '0':
                self.skip_scenes = True
            else:
                self.scenes = Path(scenes)

        self.threshold = self.args.threshold

        if not find_executable('ffmpeg'):
            print('No ffmpeg')
            sys.exit()

        # Set encoder if provided
        self.encoder = self.args.encoder.strip()

        # Check if encoder executable is reachable
        if self.encoder in ('svt_av1', 'rav1e', 'aom'):
            if self.encoder == 'rav1e':
                enc = 'rav1e'
            elif self.encoder == 'aom':
                enc = 'aomenc'
            elif self.encoder == 'svt_av1':
                enc = 'SvtAv1EncApp'
            if not find_executable(enc):
                print(f'Encoder {enc} not found')
                sys.exit()
        else:
            print(f'Not valid encoder {self.encoder}')
            sys.exit()

        # Set mode
        self.mode = self.args.mode

        # Number of encoder passes
        self.passes = self.args.passes

        # Set output file
        if self.args.output_file is None:
            self.output_file = Path(f'{self.args.file_path.stem}_av1.mkv')
        else:
            self.output_file = self.args.output_file.with_suffix('.mkv')

        # Forcing FPS option
        if self.args.ffmpeg == 0:
            self.ffmpeg = ''
        else:
            self.ffmpeg = self.args.ffmpeg

        # Changing pixel format, bit format
        if self.args.pix_format != self.pix_format:
            self.pix_format = f' -strict -1 -pix_fmt {self.args.pix_format}'
        else:
            self.pix_format = f'-pix_fmt {self.args.pix_format}'

        self.ffmpeg_pipe = f' {self.ffmpeg} {self.pix_format} -f yuv4mpegpipe - |'

    def determine_resources(self):
        """Returns number of workers that machine can handle with selected encoder."""
        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.encoder == 'aom' or self.encoder == 'rav1e':
            self.workers = round(min(cpu / 2, ram / 1.5))

        elif self.encoder == 'svt_av1':
            self.workers = round(min(cpu, ram)) // 5

        # fix if workers round up to 0
        if self.workers == 0:
            self.workers += 1

    def set_logging(self):
        """Setting logging file."""
        if self.args.logging:
            self.logging = f"{self.args.logging}.log"
        else:
            self.logging = self.temp_dir / 'log.log'

    def setup(self, input_file: Path):
        """Creating temporally folders when needed."""
        if not input_file.exists():
            prnt = f'No file: {input_file}\nCheck paths'
            print(prnt)
            sys.exit()

        # Make temporal directories, and remove them if already presented
        if self.temp_dir.exists() and self.args.resume:
            pass
        else:
            if self.temp_dir.is_dir():
                shutil.rmtree(self.temp_dir)
            (self.temp_dir / 'split').mkdir(parents=True)
            (self.temp_dir / 'encode').mkdir()

        if self.logging is os.devnull:
            self.logging = self.temp_dir / 'log.log'

    def extract_audio(self, input_vid: Path):
        """Extracting audio from source, transcoding if needed."""
        audio_file = self.temp_dir / 'audio.mkv'
        if audio_file.exists():
            self.log('Reusing Audio File\n')
            return

        # Capture output to check if audio is present

        check = fr'{self.FFMPEG} -ss 0 -i "{input_vid}" -t 0 -vn -c:a copy -f null -'
        is_audio_here = len(self.call_cmd(check, capture_output=True)) == 0

        if is_audio_here:
            self.log(f'Audio processing\n'
                     f'Params: {self.args.audio_params}\n')
            cmd = f'{self.FFMPEG} -i "{input_vid}" -vn ' \
                  f'{self.args.audio_params} {audio_file}'
            self.call_cmd(cmd)

    def get_vmaf(self, source: Path, encoded: Path, ):
        if self.args.vmaf_path != '':
            model = f'=model_path={self.args.vmaf_path}'
        else:
            model = ''

        cmd = f'{self.FFMPEG} -i {source.as_posix()} -i {encoded.as_posix()}  ' \
              f'-filter_complex "[0:v][1:v]libvmaf{model}" ' \
              f'-max_muxing_queue_size 1024 -f null - '

        result = (self.call_cmd(cmd, capture_output=True)).decode().strip().split()

        if 'monotonically' in result:
            return 'Nan. Non monotonically increasing dts to muxer. Check your source'
        try:

            res = float(result[-1])
            return res
        except:
            return 'Nan'

    def reduce_scenes(self, scenes):
        """Windows terminal can't handle more than ~600 scenes in length."""
        if len(scenes) > 600:
            scenes = scenes[::2]
            self.reduce_scenes(scenes)
        return scenes

    def scene_detect(self, video: Path):
        """Running scene detection on source video for segmenting."""
        # Skip scene detection if the user choosed to
        if self.skip_scenes:
            self.log('Skipping scene detection\n')
            return ''

        try:

            # PySceneDetect used split video by scenes and pass it to encoder
            # Optimal threshold settings 15-50

            video_manager = VideoManager([str(video)])
            scene_manager = SceneManager()
            scene_manager.add_detector(ContentDetector(threshold=self.threshold))
            base_timecode = video_manager.get_base_timecode()

            # If stats file exists, load it.
            if self.scenes and self.scenes.exists():
                # Read stats from CSV file opened in read mode:
                with self.scenes.open() as stats_file:
                    stats = stats_file.read()
                    self.log('Using Saved Scenes\n')
                    return stats

            # Work on whole video
            video_manager.set_duration()

            # Set downscale factor to improve processing speed.
            video_manager.set_downscale_factor()

            # Start video_manager.
            video_manager.start()

            # Perform scene detection on video_manager.
            self.log(f'Starting scene detection Threshold: {self.threshold}\n')
            scene_manager.detect_scenes(frame_source=video_manager, show_progress=True)

            # Obtain list of detected scenes.
            scene_list = scene_manager.get_scene_list(base_timecode)
            # Like FrameTimecodes, each scene in the scene_list can be sorted if the
            # list of scenes becomes unsorted.

            self.log(f'Found scenes: {len(scene_list)}\n')

            scenes = [scene[0].get_timecode() for scene in scene_list]

            # Fix for windows character limit
            if sys.platform != 'linux':
                scenes = self.reduce_scenes(scenes)

            scenes = ','.join(scenes[1:])

            # We only write to the stats file if a save is required:
            if self.scenes:
                self.scenes.write_text(scenes)
            return scenes

        except Exception as e:
            self.log(f'Error in PySceneDetect: {e}\n')
            print(f'Error in PySceneDetect{e}\n')
            sys.exit()

    def split(self, video, timecodes):
        """Spliting video by timecodes, or just copying video."""
        if len(timecodes) == 0:
            self.log('Copying video for encode\n')
            cmd = f'{self.FFMPEG} -i "{video}" -map_metadata -1 -an -c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "0.mkv"}'
        else:
            self.log('Splitting video\n')
            cmd = f'{self.FFMPEG} -i "{video}" -map_metadata -1 -an -f segment -segment_times {timecodes} ' \
                  f'-c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "%04d.mkv"}'

        self.call_cmd(cmd)

    def frame_probe(self, source: Path):
        """Get frame count."""
        cmd = f'ffmpeg -hide_banner  -i "{source.absolute()}" -an  -map 0:v:0 -c:v copy -f null - '
        frames = (self.call_cmd(cmd, capture_output=True)).decode("utf-8")
        frames = int(frames[frames.rfind('frame=') + 6:frames.rfind('fps=')])
        return frames

    def frame_check(self, source: Path, encoded: Path):
        """Checking is source and encoded video framecounts match."""
        status_file = Path(self.temp_dir / 'done.txt')

        if self.args.no_check:
            with status_file.open('a') as done:
                done.write('"' + source.name + '", ')
                return

        s1, s2 = [self.frame_probe(i) for i in (source, encoded)]

        if s1 == s2:
            with status_file.open('a') as done:
                done.write(f'({s1}, "{source.name}"), ')
        else:
            print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')

    def get_video_queue(self, source_path: Path):
        """Returns sorted list of all videos that need to be encoded. Big first."""
        queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']

        if self.args.resume:
            done_file = self.temp_dir / 'done.txt'
            if done_file.exists():
                with open(done_file, 'r') as f:
                    data = [line for line in f]
                    data = literal_eval(data[-1])
                    queue = [x for x in queue if x.name not in [x[1] for x in data]]

        queue = sorted(queue, key=lambda x: -x.stat().st_size)

        if len(queue) == 0:
            print('Error: No files found in .temp/split, probably splitting not working')
            sys.exit()

        return queue

    def svt_av1_encode(self, file_paths):
        """SVT-AV1 encoding command composition."""
        if self.args.video_params == '':
            print('-w -h -fps is required parameters for svt_av1 encoder')
            sys.exit()
        else:
            self.video_params = self.args.video_params
        encoder = 'SvtAv1EncApp '
        if self.passes == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.video_params} -b {file[1].with_suffix(".ivf")} -',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_1_commands

        if self.passes == 2:
            p2i = '-input-stat-file '
            p2o = '-output-stat-file '
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.video_params} {p2o} '
                 f'{file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {self.ffmpeg_pipe} '
                 +
                 f'{encoder} -i stdin {self.video_params} {p2i} '
                 f'{file[0].with_suffix(".stat")} -b {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_2_commands

    def aom_encode(self, file_paths):
        """AOM encoding command composition."""
        if self.args.video_params == '':
            self.video_params = '--threads=4 --cpu-used=5 --end-usage=q --cq-level=40'
        else:
            self.video_params = self.args.video_params

        single_p = 'aomenc  -q --passes=1 '
        two_p_1_aom = 'aomenc -q --passes=2 --pass=1'
        two_p_2_aom = 'aomenc  -q --passes=2 --pass=2'

        if self.passes == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {single_p} {self.video_params} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_1_commands

        if self.passes == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_p_1_aom} {self.video_params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_p_2_aom} {self.video_params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_2_commands

    def rav1e_encode(self, file_paths):
        """Rav1e encoding command composition."""
        if self.args.video_params == '':
            self.video_params = ' --tiles=4 --speed=10'
        else:
            self.video_params = self.args.video_params
        if self.passes == 1 or self.passes == 2:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} '
                 f' rav1e -  {self.video_params}  '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_1_commands
        if self.passes == 2:

            # 2 encode pass not working with FFmpeg pipes :(
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} '
                 f' rav1e - --first-pass {file[0].with_suffix(".stat")} {self.video_params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 f'-i {file[0]} {self.ffmpeg_pipe} '
                 f' rav1e - --second-pass {file[0].with_suffix(".stat")} {self.video_params} '
                 f'--output {file[1].with_suffix(".ivf")}',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]

            return pass_2_commands

    def compose_encoding_queue(self, files):
        """Composing encoding queue with splitted videos."""
        file_paths = [(self.temp_dir / "split" / file.name,
                       self.temp_dir / "encode" / file.name,
                       file) for file in files]

        if self.encoder == 'aom':
            queue = self.aom_encode(file_paths)

        elif self.encoder == 'rav1e':
            queue = self.rav1e_encode(file_paths)

        elif self.encoder == 'svt_av1':
            queue = self.svt_av1_encode(file_paths)

        else:
            print(self.encoder)
            print(f'No valid encoder : "{self.encoder}"')
            sys.exit()

        self.log(f'Encoding Queue Composed\n'
                 f'Encoder: {self.encoder.upper()} Queue Size: {len(queue)} Passes: {self.passes}\n'
                 f'Params: {self.video_params}\n')

        return queue

    def get_brightness(self, video):
        """Getting average brightness value for single video."""
        brightness = []
        cap = cv2.VideoCapture(video)
        try:
            while True:
                # Capture frame-by-frame
                _, frame = cap.read()

                # Our operations on the frame come here
                gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)

                # Display the resulting frame
                mean = cv2.mean(gray)
                brightness.append(mean[0])
                if cv2.waitKey(1) & 0xFF == ord('q'):
                    break
        except cv2.error:
            pass

        # When everything done, release the capture
        cap.release()
        brig_geom = round(statistics.geometric_mean([x+1 for x in brightness]), 1)

        return brig_geom

    def boost(self, command: str, br_geom, new_cq=0):
        """Based on average brightness of video decrease(boost) Quantizer value for encoding."""
        mt = '--cq-level='
        cq = int(command[command.find(mt) + 11:command.find(mt) + 13])
        if not new_cq:
            if br_geom < 128:
                new_cq = cq - round((128 - br_geom) / 128 * self.args.br)

                # Cap on boosting
                if new_cq < self.args.bl:
                    new_cq = self.args.bl
            else:
                new_cq = cq
        cmd = command[:command.find(mt) + 11] + \
              str(new_cq) + command[command.find(mt) + 13:]

        return cmd, new_cq

    def encode(self, commands):
        """Single encoder command queue and logging output."""
        # Passing encoding params to ffmpeg for encoding.
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly.

        st_time = time.time()
        source, target = Path(commands[-1][0]), Path(commands[-1][1])
        frame_probe_source = self.frame_probe(source)

        if self.args.boost:
            br = self.get_brightness(source.absolute().as_posix())

            com0, cq = self.boost(commands[0], br)

            if self.passes == 2:
                com1, _ = self.boost(commands[1], br, cq)
                commands = (com0, com1) + commands[2:]
            else:
                commands = com0 + commands[1:]

            self.log(f'Enc:  {source.name}, {frame_probe_source} fr\n'
                     f'Avg brightness: {br}\n'
                     f'Adjusted CQ: {cq}\n\n')

        else:
            self.log(f'Enc:  {source.name}, {frame_probe_source} fr\n\n')

        # Queue execution
        for i in commands[:-1]:
            cmd = rf'{self.FFMPEG} {i}'
            self.call_cmd(cmd)

        self.frame_check(source, target)

        frame_probe = self.frame_probe(target)

        enc_time = round(time.time() - st_time, 2)

        if self.args.vmaf:
            vmaf = f'Vmaf: {self.get_vmaf(source, target)}\n'
        else:
            vmaf = ''

        self.log(f'Done: {source.name} Fr: {frame_probe}\n'
                 f'Fps: {round(frame_probe / enc_time, 4)} Time: {enc_time} sec.\n{vmaf}\n')
        return self.frame_probe(source)

    def concatenate_video(self):
        """With FFMPEG concatenate encoded segments into final file."""
        with open(f'{self.temp_dir / "concat"}', 'w') as f:

            encode_files = sorted((self.temp_dir / 'encode').iterdir())
            f.writelines(f"file '{file.absolute()}'\n" for file in encode_files)

        # Add the audio file if one was extracted from the input
        audio_file = self.temp_dir / "audio.mkv"
        if audio_file.exists():
            audio = f'-i {audio_file} -c:a copy'
        else:
            audio = ''

        try:
            cmd = f'{self.FFMPEG} -f concat -safe 0 -i {self.temp_dir / "concat"} {audio} -c copy -y "{self.output_file}"'
            concat = self.call_cmd(cmd, capture_output=True)
            if len(concat) > 0:
                raise Exception

            self.log('Concatenated\n')

            # Delete temp folders
            if not self.args.keep:
                shutil.rmtree(self.temp_dir)

        except Exception as e:
            print(f'Concatenation failed, error: {e}')
            self.log(f'Concatenation failed, aborting, error: {e}\n')
            sys.exit()

    def encoding_loop(self, commands):
        """Creating process pool for encoders, creating progress bar."""
        with Pool(self.workers) as pool:

            self.workers = min(len(commands), self.workers)
            enc_path = self.temp_dir / 'split'
            done_path = Path(self.temp_dir / 'done.txt')
            if self.args.resume and done_path.exists():

                self.log('Resuming...\n')
                with open(done_path, 'r') as f:
                    lines = [line for line in f]
                    data = literal_eval(lines[-1])
                    total = int(lines[0])
                    done = [x[1] for x in data]

                self.log(f'Resumed with {len(done)} encoded clips done\n\n')

                initial = sum([int(x[0]) for x in data])

            else:
                initial = 0
                with open(Path(self.temp_dir / 'done.txt'), 'w') as f:
                    total = self.frame_probe(self.args.file_path)
                    f.write(f'{total}\n')

            clips = len([x for x in enc_path.iterdir() if x.suffix == ".mkv"])
            print(f'\rQueue: {clips} Workers: {self.workers} Passes: {self.passes}\nParams: {self.video_params}')

            bar = tqdm(total=total, initial=initial, dynamic_ncols=True, unit="fr",
                       leave=False)

            loop = pool.imap_unordered(self.encode, commands)
            self.log(f'Started encoding queue with {self.workers} workers\n\n')

            try:
                for enc_frames in loop:
                    bar.update(n=enc_frames)
            except Exception as e:
                print(f'Encoding error: {e}')
                sys.exit()

    def setup_routine(self):
        """All pre encoding routine.
        Scene detection, splitting, audio extraction"""
        if not (self.args.resume and self.temp_dir.exists()):
            # Check validity of request and create temp folders/files
            self.setup(self.args.file_path)

            self.set_logging()

            # Splitting video and sorting big-first
            timestamps = self.scene_detect(self.args.file_path)
            self.split(self.args.file_path, timestamps)

            # Extracting audio
            self.extract_audio(self.args.file_path)
        else:
            self.set_logging()

        files = self.get_video_queue(self.temp_dir / 'split')

    def video_encoding(self):
        """Encoding video on local machine."""
        self.setup_routine()

        # Make encode queue
        commands = self.compose_encoding_queue(files)

        # Catch Error
        if len(commands) == 0:
            print('Error in making command queue')
            sys.exit()

        # Determine resources if workers don't set
        if self.args.workers != 0:
            self.workers = self.args.workers
        else:
            self.determine_resources()

        self.encoding_loop(commands)

        self.concatenate_video()

    def master_mode(self):
        """Master mode. Splitting, managing queue, sending chunks, receiving chunks, concat videos."""
        print('Working in master mode')

        # Setup
        self.setup_routine()
        # Creating Queue
        # Sending Chunks to server

    def server(self):
        """Encoder mode: Connecting, Receiving, Encoding."""
        print('Working in server mode')

        # while chunks
        # receive chunks
        # encode
        # send back

    def main(self):
        """Main."""
        # Start time
        tm = time.time()

        # Parse initial arguments
        self.arg_parsing()

        # Video Mode
        if self.mode == 0:
            self.video_encoding()

        # Encoder mode
        elif self.mode == 1:
            self.encoder_mode()

        # Master mode
        elif self.mode == 2:
            self.master_mode()

        else:
            print('No valid work mode')
            exit()

        print(f'Finished: {round(time.time() - tm, 1)}s')


if __name__ == '__main__':

    # Windows fix for multiprocessing
    multiprocessing.freeze_support()

    # Main thread
    try:
        start = time.time()
        Av1an().main()
    except KeyboardInterrupt:
        print('Encoding stopped')
        if sys.platform == 'linux':
            os.system('stty sane')
        sys.exit()

    # Prevent linux terminal from hanging
    if sys.platform == 'linux':
        os.system('stty sane')
