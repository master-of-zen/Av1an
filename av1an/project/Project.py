import os
import shutil
import sys
from distutils.spawn import find_executable
from pathlib import Path

from av1an.utils import frame_probe_fast
from av1an_pyo3 import create_vs_file
from av1an_pyo3 import frame_probe_vspipe, get_ffmpeg_info, hash_path, log


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
        self.output_ivf: bool = None
        self.webm = None

        # Splitting
        self.chunk_method: str = None
        self.scenes: Path = None
        self.split_method: str = None
        self.extra_split: int = None
        self.min_scene_len: int = None

        # PySceneDetect split
        self.threshold: float = None

        # Encoding
        self.passes = None
        self.video_params = None
        self.encoder: str = None
        self.workers: int = None

        # FFmpeg params
        self.ffmpeg_pipe = None
        self.ffmpeg: str = None
        self.audio_params = None
        self.pix_format = None

        # Misc
        self.quiet = False
        self.logging = None
        self.resume: bool = None
        self.keep: bool = None
        self.force: bool = None

        # Vmaf
        self.vmaf: bool = None
        self.vmaf_path: str = None
        self.vmaf_res: str = None

        # Target Quality
        self.target_quality: int = None
        self.probes: int = None
        self.probe_slow: bool = None
        self.min_q: int = None
        self.max_q: int = None
        self.vmaf_plots: bool = None
        self.probing_rate: int = None
        self.n_threads: int = None
        self.vmaf_filter: str = None

        # Set all initial values
        for key in initial_data:
            setattr(self, key, initial_data[key])

    def get_frames(self):
        # TODO: Unify get frames with vs pipe cache generation

        if self.frames > 0:
            return self.frames

        if self.chunk_method in ("vs_ffms2", "vs_lsmash"):
            vs = (
                self.input
                if self.is_vs
                else create_vs_file(
                    str(self.temp.resolve()),
                    str(self.input.resolve()),
                    self.chunk_method,
                )
            )
            fr = frame_probe_vspipe(str(vs))
            if fr > 0:
                self.frames = fr
                return fr

        total = frame_probe_fast(self.input, self.is_vs)

        self.frames = total

        return self.frames

    def set_frames(self, frame_count: int):
        self.frames = frame_count

    def outputs_filenames(self):
        if self.webm:
            suffix = ".webm"
        else:
            suffix = ".mkv"

        # Check for non-empty string
        if isinstance(self.output_file, str) and self.output_file:
            if self.output_file[-1] in ("\\", "/"):
                if not Path(self.output_file).exists():
                    os.makedirs(Path(self.output_file), exist_ok=True)
                self.output_file = Path(
                    f"{self.output_file}{self.input.stem}_{self.encoder}{suffix}"
                )
            else:
                self.output_file = Path(self.output_file).with_suffix(suffix)
        else:
            self.output_file = Path(f"{self.input.stem}_{self.encoder}{suffix}")

    def select_best_chunking_method(self):
        if not find_executable("vspipe"):
            self.chunk_method = "hybrid"
            log("Set Chunking Method: Hybrid")
        else:
            try:
                import vapoursynth

                plugins = vapoursynth.get_core().get_plugins()

                if "systems.innocent.lsmas" in plugins:
                    log("Set Chunking Method: L-SMASH")
                    self.chunk_method = "vs_lsmash"

                elif "com.vapoursynth.ffms2" in plugins:
                    log("Set Chunking Method: FFMS2")
                    self.chunk_method = "vs_ffms2"
                else:
                    log(f"Vapoursynth installed but no supported chunking methods.")
                    log("Fallback to Hybrid")
                    self.chunk_method = "hybrid"

            except Exception as e:
                log(f"Vapoursynth not installed but vspipe reachable")
                log(f"Error: {e}" + "Fallback to Hybrid")
                self.chunk_method = "hybrid"
