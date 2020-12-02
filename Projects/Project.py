
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


