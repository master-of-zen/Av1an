#!/usr/bin/env python3

# Todo:
# Benchmarking
# Add conf file
# Add new paths for frame check to all encoders
# Make it not split if splits are there

import time
from tqdm import tqdm
import sys
import os
import shutil
from ast import literal_eval
from psutil import virtual_memory
import argparse
from math import ceil
from multiprocessing import Pool
import multiprocessing
import subprocess
from pathlib import Path
from typing import Optional


from scenedetect.video_manager import VideoManager
from scenedetect.scene_manager import SceneManager
from scenedetect.detectors import ContentDetector


if sys.version_info < (3, 7):
    print('Av1an requires at least Python 3.7 to run.')
    sys.exit()


class Av1an:

    def __init__(self):
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

    def call_cmd(self, cmd, capture_output=False):
        if capture_output:
            return subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).stdout

        with open(self.logging, 'a') as log:
            subprocess.run(cmd, shell=True, stdout=log, stderr=log)

    def arg_parsing(self):

        # Command line parse and assigning defined and user defined params

        parser = argparse.ArgumentParser()
        parser.add_argument('--mode', '-m', type=int, default=self.mode, help='Mode 0 - video, Mode 1 - image')
        parser.add_argument('--video_params', '-v', type=str, default=self.video_params, help='encoding settings')
        parser.add_argument('--file_path', '-i', type=Path, default='bruh.mp4', help='Input File', required=True)
        parser.add_argument('--encoder', '-enc', type=str, default=self.encoder, help='Choosing encoder')
        parser.add_argument('--workers', '-w', type=int, default=0, help='Number of workers')
        parser.add_argument('--audio_params', '-a', type=str, default='-c:a copy', help='FFmpeg audio settings')
        parser.add_argument('--threshold', '-tr', type=float, default=self.threshold, help='PySceneDetect Threshold')
        parser.add_argument('--logging', '-log', type=str, default=self.logging, help='Enable logging')
        parser.add_argument('--passes', '-p', type=int, default=self.passes, help='Specify encoding passes')
        parser.add_argument('--output_file', '-o', type=Path, default=None, help='Specify output file')
        parser.add_argument('--ffmpeg', '-ff', type=str, default='', help='FFmpeg commands')
        parser.add_argument('--pix_format', '-fmt', type=str, default=self.pix_format, help='FFmpeg pixel format')
        parser.add_argument('--scenes', '-s', type=str, default=self.scenes, help='File location for scenes')
        parser.add_argument('--resume', '-r', help='Resuming previous session', action='store_true')
        parser.add_argument('--no_check', '-n', help='Do not check encodings', action='store_true')

        # Pass command line args that were passed
        self.args = parser.parse_args()

        # Set scenes if provided
        if self.args.scenes:
            scenes = self.args.scenes.strip()
            if scenes == '0':
                self.skip_scenes = True
            else:
                self.scenes = Path(scenes)

        self.threshold = self.args.threshold

        # Set encoder if provided
        self.encoder = self.args.encoder.strip()
        if self.encoder not in ('svt_av1', 'rav1e', 'aom'):
            print(f'Not valid encoder {self.encoder}')
            sys.exit()

        # Set mode (Video/Picture)
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

        # Setting logging file
        if self.args.logging:
            self.logging = f"{self.args.logging}.log"
        else:
            self.logging = os.devnull

    def determine_resources(self):

        # Returns number of workers that machine can handle with selected encoder

        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.encoder == 'aom' or self.encoder == 'rav1e':
            self.workers = ceil(min(cpu/2, ram/1.5))

        elif self.encoder == 'svt_av1':
            self.workers = ceil(min(cpu, ram)) // 5

        # fix if workers round up to 0
        if self.workers == 0:
            self.workers += 1

    def setup(self, input_file: Path):
        if not input_file.exists():
            print(f'File: {input_file} not exist')
            sys.exit()

        # Make temporal directories, and remove them if already presented
        if self.temp_dir.exists() and self.args.resume:
            pass
        else:
            if self.temp_dir.is_dir():
                shutil.rmtree(self.temp_dir)
            (self.temp_dir / 'split').mkdir(parents=True)
            (self.temp_dir / 'encode').mkdir()

    def extract_audio(self, input_vid: Path):

        # Extracting audio from video file
        # Encoding audio if needed

        audio_file = self.temp_dir / 'audio.mkv'
        if audio_file.exists():
            return

        ffprobe = 'ffprobe -hide_banner -loglevel error -show_streams -select_streams a'

        # Capture output to check if audio is present
        check = fr'{ffprobe} -i {input_vid}'
        is_audio_here = len(self.call_cmd(check, capture_output=True)) > 0

        if is_audio_here:
            cmd = f'{self.FFMPEG} -i {input_vid} -vn ' \
                    f'{self.args.audio_params} {audio_file}'
            self.call_cmd(cmd)

    def scenedetect(self, video: Path):
        # Skip scene detection if the user choosed to
        if self.skip_scenes:
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
                    return stats

            # Work on whole video
            video_manager.set_duration()

            # Set downscale factor to improve processing speed.
            video_manager.set_downscale_factor()

            # Start video_manager.
            video_manager.start()

            # Perform scene detection on video_manager.
            scene_manager.detect_scenes(frame_source=video_manager, show_progress=True)

            # Obtain list of detected scenes.
            scene_list = scene_manager.get_scene_list(base_timecode)
            # Like FrameTimecodes, each scene in the scene_list can be sorted if the
            # list of scenes becomes unsorted.

            scenes = [scene[0].get_timecode() for scene in scene_list]
            scenes = ','.join(scenes[1:])

            # We only write to the stats file if a save is required:
            if self.scenes:
                self.scenes.write_text(scenes)
            return scenes

        except Exception:

            print('Error in PySceneDetect')
            sys.exit()

    def split(self, video, timecodes):

        # Splits video with provided timecodes
        # If video is single scene, just copy video

        if len(timecodes) == 0:
            cmd = f'{self.FFMPEG} -i {video} -map_metadata 0 -an -c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "0.mkv"}'
        else:
            cmd = f'{self.FFMPEG} -i {video} -map_metadata 0 -an -f segment -segment_times {timecodes} ' \
                  f'-c copy -avoid_negative_ts 1 {self.temp_dir / "split" / "%04d.mkv"}'

        self.call_cmd(cmd)

    def frame_probe(self, source: Path):
        cmd = f'ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames ' \
              f'-of default=nokey=1:noprint_wrappers=1 {source.absolute()}'
        frames = int(self.call_cmd(cmd, capture_output=True).strip())
        return frames

    def frame_check(self, source: Path, encoded: Path):

        done_file = Path(self.temp_dir / 'done.txt')

        if self.args.no_check:
            with done_file.open('a') as done:
                done.write('"' + source.name + '", ')
                return

        s1, s2 = [self.frame_probe(i) for i in (source, encoded)]

        if s1 == s2:
            with done_file.open('a') as done:
                done.write('"' + source.name + '", ')
        else:
            print(f'Frame Count Differ for Source {source.name}: {s2}/{s1}')

    def get_video_queue(self, source_path: Path):

        # Returns sorted list of all videos that need to be encoded. Big first
        queue = [x for x in source_path.iterdir() if x.suffix == '.mkv']
        if self.args.resume:
            done_file = self.temp_dir / 'done.txt'
            if done_file.exists():
                with open(done_file, 'r') as f:
                    data = literal_eval(f.read())
                    queue = [x for x in queue if x.name not in data]

        queue = sorted(queue, key=lambda x: -x.stat().st_size)
        return queue

    def svt_av1_encode(self, file_paths):

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
                 f'  {encoder} -i stdin {self.video_params} {p2o} {file[0].with_suffix(".stat")} -b {file[0]}.bk - ',
                 f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {encoder} -i stdin {self.video_params} {p2i} {file[0].with_suffix(".stat")} -b {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_2_commands

    def aom_encode(self, file_paths):

        if self.args.encoding_params == '':
            self.encoding_params = '--threads=4 --cpu-used=5 --end-usage=q --cq-level=40'
        else:
            self.video_params = self.args.video_params

        single_pass = 'aomenc  --verbose --passes=1 '
        two_pass_1_aom = 'aomenc  --verbose --passes=2 --pass=1'
        two_pass_2_aom = 'aomenc  --verbose --passes=2 --pass=2'

        if self.passes == 1:
            pass_1_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe} ' +
                 f'  {single_pass} {self.video_params} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_1_commands

        if self.passes == 2:
            pass_2_commands = [
                (f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_1_aom} {self.video_params} --fpf={file[0].with_suffix(".log")} -o {os.devnull} - ',
                 f'-i {file[0]} {self.ffmpeg_pipe}' +
                 f' {two_pass_2_aom} {self.video_params} --fpf={file[0].with_suffix(".log")} -o {file[1].with_suffix(".ivf")} - ',
                 (file[0], file[1].with_suffix('.ivf')))
                for file in file_paths]
            return pass_2_commands

    def rav1e_encode(self, file_paths):

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

        file_paths = [(self.temp_dir / "split" / file.name,
                       self.temp_dir / "encode" / file.name,
                       file) for file in files]

        if self.encoder == 'aom':
            return self.aom_encode(file_paths)

        elif self.encoder == 'rav1e':
            return self.rav1e_encode(file_paths)

        elif self.encoder == 'svt_av1':
            return self.svt_av1_encode(file_paths)

        else:
            print(self.encoder)
            print(f'No valid encoder : "{self.encoder}"')
            sys.exit()

    def encode(self, commands):

        # Passing encoding params to ffmpeg for encoding
        # Replace ffmpeg with aom because ffmpeg aom doesn't work with parameters properly

        for i in commands[:-1]:
            cmd = rf'{self.FFMPEG} {i}'
            self.call_cmd(cmd)
        source, target = Path(commands[-1][0]), Path(commands[-1][1])
        self.frame_check(source, target)
        return self.frame_probe(source)

    def concatenate_video(self):

        # Using FFMPEG to concatenate all encoded videos to 1 file.
        # Reading all files in A-Z order and saving it to concat.txt

        with open(f'{self.temp_dir / "concat"}', 'w') as f:
            # Write all files that need to be concatenated
            # Their path must be relative to the directory where "concat.txt" is

            encode_files = sorted((self.temp_dir / 'encode').iterdir())
            f.writelines(f"file '{file.absolute()}'\n" for file in encode_files)

        # Add the audio file if one was extracted from the input
        audio_file = self.temp_dir / "audio.mkv"
        if audio_file.exists():
            audio = f'-i {audio_file} -c:a copy'
        else:
            audio = ''

        try:
            cmd = f'{self.FFMPEG} -f concat -safe 0 -i {self.temp_dir / "concat"} {audio} -c copy -y {self.output_file}'
            concat = self.call_cmd(cmd, capture_output=True)
            if len(concat) > 0:
                raise Exception

        except Exception:
            print('Concatenation failed')
            sys.exit()

    def image(self, image_path: Path):
        print('Encoding Image..', end='')

        image_pipe = rf'{self.FFMPEG} -i {image_path} -pix_fmt yuv420p10le -f yuv4mpegpipe -strict -1 - | '
        output = image_path.with_suffix('.ivf')

        if self.encoder == 'aom':
            aom = ' aomenc --passes=1 --pass=1 --end-usage=q  -b 10 --input-bit-depth=10 '
            cmd = (rf' {image_pipe} ' +
                   rf'{aom} {self.video_params} -o {output} - ')
            self.call_cmd(cmd)

        elif self.encoder == 'rav1e':
            cmd = (rf' {image_pipe} ' +
                   rf' rav1e {self.video_params} - -o {output} ')
            self.call_cmd(cmd)
        else:
            print(f'Not valid encoder: {self.encoder}')
            sys.exit()

    def main(self):

        # Parse initial arguments
        self.arg_parsing()

        # Video Mode
        if self.mode == 0:

            if not (self.args.resume and self.temp_dir.exists()):
                # Check validity of request and create temp folders/files
                self.setup(self.args.file_path)

                # Splitting video and sorting big-first
                timestamps = self.scenedetect(self.args.file_path)
                self.split(self.args.file_path, timestamps)
                # Extracting audio
                self.extract_audio(self.args.file_path)

            files = self.get_video_queue(self.temp_dir / 'split')

            # Make encode queue
            commands = self.compose_encoding_queue(files)

            # Catch Error
            if len(commands) == 0:
                print('Error: splitting and making encoding queue')
                sys.exit()

            # Determine resources if workers don't set
            if self.args.workers != 0:
                self.workers = self.args.workers
            else:
                self.determine_resources()

            # Creating threading pool to encode bunch of files at the same time and show progress bar
            with Pool(self.workers) as pool:

                self.workers = min(len(commands), self.workers)
                enc_path = self.temp_dir / 'split'
                initial = len([x for x in enc_path.iterdir() if x.suffix == '.mkv'])
                print(f'\rClips: {initial} Workers: {self.workers} Passes: {self.encode_pass}\nParams: {self.encoding_params}')

                bar = tqdm(total=self.frame_probe(self.args.file_path),
                           initial=0, dynamic_ncols=True, unit="fr",
                           leave=False)
                loop = pool.imap_unordered(self.encode, commands)
                try:
                    for b in loop:
                        bar.update(n=b)
                except ValueError:
                    print('Encoding error')
                    sys.exit()

            self.concatenate_video()

            # Delete temp folders
            shutil.rmtree(self.temp_dir)

        elif self.mode == 1:
            self.image(self.args.file_path)

        else:
            print('No valid work mode')
            exit()


if __name__ == '__main__':

    # Windows fix for multiprocessing
    multiprocessing.freeze_support()

    # Main thread
    try:
        start = time.time()
        av1an = Av1an()
        av1an.main()
        print(f'Finished: {round(time.time() - start, 1)}s')
    except KeyboardInterrupt:
        print('Encoding stopped')
        if sys.platform == 'linux':
            os.popen('stty sane', 'r')
        sys.exit()

    # Prevent linux terminal from hanging
    if sys.platform == 'linux':
        os.popen('stty sane', 'r')
