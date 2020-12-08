import json
from pathlib import Path
from Av1an.commandtypes import Command
from Av1an.utils import frame_probe_fast

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
        if self.frames > 0:
            return self.frames
        else:
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

