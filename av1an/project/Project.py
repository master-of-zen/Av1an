import json
import sys
import os
import shutil
from psutil import virtual_memory
from distutils.spawn import find_executable
from pathlib import Path
from av1an.commandtypes import Command
from av1an.utils import frame_probe_fast,  hash_path, terminate
from av1an.concat import vvc_concat, concatenate_ffmpeg, concatenate_mkvmerge
from av1an.logger import log
from av1an.vapoursynth import create_vs_file, frame_probe_vspipe
import inspect
class Project(object):

    def __init__(self, initial_data):

        # Project info
        self.frames: int = 0
        self.counter = None
        self.is_vs: bool = None

        # Input/Output/Temp
        self.input: Path = None
        self.temp: Path = None
        self.output_file: Path = None
        self.mkvmerge: bool = None
        self.config = None
        self.webm = None

        # Splitting
        self.chunk_method: str = None
        self.scenes: Path = None
        self.split_method: str = None
        self.extra_split: int = None
        self.min_scene_len: int = None

        # PySceneDetect split
        self.threshold: float = None

        # AOM Keyframe split
        self.reuse_first_pass: bool = None

        # Encoding
        self.passes = None
        self.video_params: Command = None
        self.encoder: str = None
        self.workers: int = None

        # FFmpeg params
        self.ffmpeg_pipe: Command = None
        self.ffmpeg: str = None
        self.audio_params = None
        self.pix_format: Command = None

        # Misc
        self.quiet = False
        self.logging = None
        self.resume: bool = None
        self.no_check: bool = None
        self.keep: bool = None
        self.force: bool = None

        # Vmaf
        self.vmaf: bool = None
        self.vmaf_path: str = None
        self.vmaf_res: str = None

        # Target Quality
        self.target_quality: int = None
        self.probes: int = None
        self.min_q: int = None
        self.max_q: int = None
        self.vmaf_plots: bool = None
        self.probing_rate: int = None
        self.n_threads: int = None
        self.vmaf_filter: str = None

        # VVC
        self.vvc_conf: Path = None
        self.video_dimensions = (None, None)
        self.video_framerate = None

        # Set all initial values
        self.load_project(initial_data)

    def load_project(self, initial_data):
        """
        Loads project attributes to this class
        """
        # Set all initial values
        for key in initial_data:
            setattr(self, key, initial_data[key])

    def get_frames(self):
        """
        Get total frame count of input file, returning total_frames from project if already exists
        """
        # TODO: Unify get frames with vs pipe cache generation

        if self.frames > 0:
            return self.frames

        if self.chunk_method in ('vs_ffms2','vs_lsmash'):
            vs = create_vs_file(self.temp, self.input, self.chunk_method)
            fr = frame_probe_vspipe(vs)
            if fr > 0:
                self.frames = fr
                return fr

        total = frame_probe_fast(self.input, self.is_vs)

        self.frames = total

        return self.frames

    def set_frames(self, frame_count: int):
        """
        Setting total frame count for project
        """
        self.frames = frame_count

    def outputs_filenames(self):
        """
        Set output filename

        :param project: the Project
        """
        if self.webm:
            suffix = '.webm'
        else:
            suffix = '.mkv'

        self.output_file = Path(self.output_file).with_suffix(suffix) if self.output_file \
            else Path(f'{self.input.stem}_{self.encoder}{suffix}')

    def load_project_from_file(self, path_string):
        """
        Loads projedt attributes from json to this class
        """
        pth = Path(path_string)
        with open(pth) as json_data:
            data = json.load(json_data)
        self.load_project(data)

    def save_project_to_file(self, path_string):
        """
        Save project attributes from json to this class
        """
        pth = Path(path_string)
        with open(pth, 'w') as json_data:
            json_data.write(self.save_project())

    def save_project(self):
        """
        Returns json of this class, which later can be loaded
        """
        dt = dict(self.__dict__)
        del dt['input']
        del dt['output_file']
        del dt['temp']
        del dt['vmaf_path']
        del dt['config']
        return json.dumps(dt, indent=4, sort_keys=True)

    def determine_workers(self):
        """Returns number of workers that machine can handle with selected encoder."""
        if self.workers:
            return self.workers

        cpu = os.cpu_count()
        ram = round(virtual_memory().total / 2 ** 30)

        if self.encoder in ('aom', 'rav1e', 'vpx'):
            workers = round(min(cpu / 3, ram / 1.5))

        elif self.encoder in ('svt_av1', 'svt_vp9', 'x265', 'x264'):
            workers = round(min(cpu, ram)) // 8

        elif self.encoder in 'vvc':
            workers = round(min(cpu, ram)) // 4

        # fix if workers round up to 0
        if workers == 0:
            workers = 1

        self.workers = workers

    def setup(self):
        """Creating temporally folders when needed."""

        if self.temp:
            self.temp = Path(str(self.temp))
        else:
            self.temp = Path('.' + str(hash_path(str(self.input))))

        # Checking is resume possible
        done_path = self.temp / 'done.json'
        self.resume = self.resume and done_path.exists()

        if not self.resume:
            if self.temp.is_dir():
                shutil.rmtree(self.temp)

        (self.temp / 'split').mkdir(parents=True, exist_ok=True)
        (self.temp / 'encode').mkdir(exist_ok=True)

    def concat_routine(self):
        """
        Runs the concatenation routine with project

        :param project: the Project
        :return: None
        """
        try:
            if self.encoder == 'vvc':
                vvc_concat(self.temp, self.output_file.with_suffix('.h266'))
            elif self.mkvmerge:
                concatenate_mkvmerge(self.temp, self.output_file)
            else:
                concatenate_ffmpeg(self.temp, self.output_file, self.encoder)
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(f'Concatenation failed, error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
            log(f'Concatenation failed, aborting, error: {e}\n')
            terminate()

    def select_best_chunking_method(self):
        """
        Selecting best chunking method based on available methods
        """
        if not find_executable('vspipe'):
            self.chunk_method = 'hybrid'
            log('Set Chunking Method: Hybrid')
        else:
            try:
                import vapoursynth
                plugins = vapoursynth.get_core().get_plugins()

                if 'systems.innocent.lsmas' in plugins:
                    log('Set Chunking Method: L-SMASH\n')
                    self.chunk_method = 'vs_lsmash'

                elif 'com.vapoursynth.ffms2' in plugins:
                    log('Set Chunking Method: FFMS2\n')
                    self.chunk_method = 'vs_ffms2'

            except:
                log('Vapoursynth not installed but vspipe reachable\n' +
                    'Fallback to Hybrid\n')
                self.chunk_method = 'hybrid'

    def check_exes(self):
        """
        Checking required executables
        """

        if not find_executable('ffmpeg'):
            print('No ffmpeg')
            terminate()

        if self.chunk_method in ['vs_ffms2', 'vs_lsmash']:
            if not find_executable('vspipe'):
                print('vspipe executable not found')
                terminate()

            try:
                import vapoursynth
                plugins = vapoursynth.get_core().get_plugins()

                if self.chunk_method == 'vs_lsmash' and "systems.innocent.lsmas" not in plugins:
                    print('lsmas is not installed')
                    terminate()

                if self.chunk_method == 'vs_ffms2' and "com.vapoursynth.ffms2" not in plugins:
                    print('ffms2 is not installed')
                    terminate()
            except ModuleNotFoundError:
                print('Vapoursynth is not installed')
                terminate()