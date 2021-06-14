import sys
import os
import shutil
from distutils.spawn import find_executable
from pathlib import Path
from av1an.commandtypes import Command
from av1an.utils import frame_probe_fast
from av1an.concat import concatenate_mkvmerge
from av1an_pyo3 import log
from av1an_pyo3 import (
    get_ffmpeg_info,
    hash_path,
    create_vs_file,
    determine_workers as determine_workers_rust,
    frame_probe_vspipe,
    concatenate_ivf,
    concatenate_ffmpeg,
)


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
        self.video_params: Command = None
        self.encoder: str = None
        self.workers: int = None

        # FFmpeg params
        self.ffmpeg_pipe: Command = None
        self.ffmpeg: str = None
        self.audio_params = None
        self.pix_format = None

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
        self.probe_slow: bool = None
        self.min_q: int = None
        self.max_q: int = None
        self.vmaf_plots: bool = None
        self.probing_rate: int = None
        self.n_threads: int = None
        self.vmaf_filter: str = None

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
        Set output filename and promts overwrite if file exists

        :param project: the Project
        """
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

    def promt_output_overwrite(self):
        if self.output_file.exists():
            print(
                f":: Output file {self.output_file} exist, overwrite? [y/n or enter]:",
                end="",
            )

            promt = input()

            if "y" in promt.lower() or promt.strip() == "":
                pass
            else:
                print("Stopping")
                sys.exit()

    def determine_workers(self):
        """Returns number of workers that machine can handle with selected encoder."""
        if self.workers:
            return self.workers

        self.workers = determine_workers_rust(self.encoder)

    def setup(self):
        """Creating temporally folders when needed."""

        hash = str(hash_path(str(self.input)))

        if self.temp:
            if self.temp[-1] in ("\\", "/"):
                self.temp = Path(f"{self.temp}{'.' + hash}")
            else:
                self.temp = Path(str(self.temp))
        else:
            self.temp = Path("." + hash)

        log(f"File hash: {hash}")
        # Checking is resume possible
        done_path = self.temp / "done.json"
        self.resume = self.resume and done_path.exists()

        if not self.resume and self.temp.is_dir():
            shutil.rmtree(self.temp)

        (self.temp / "split").mkdir(parents=True, exist_ok=True)
        (self.temp / "encode").mkdir(exist_ok=True)

    def concat_routine(self):
        """
        Runs the concatenation routine with project

        :param project: the Project
        :return: None
        """
        try:
            log("Concatenating")
            if self.output_ivf:
                concatenate_ivf(
                    str((self.temp / "encode").resolve()),
                    str(self.output_file.with_suffix(".ivf").resolve()),
                )
            elif self.mkvmerge:
                concatenate_mkvmerge(self.temp, self.output_file)
            else:
                concatenate_ffmpeg(
                    str(str(self.temp.resolve())),
                    str(str(self.output_file.resolve())),
                    self.encoder,
                )
        except Exception as e:
            _, _, exc_tb = sys.exc_info()
            print(
                f"Concatenation failed, error At line: {exc_tb.tb_lineno}\nError:{str(e)}"
            )
            log(f"Concatenation failed, aborting, error: {e}")
            sys.exit(1)

    def select_best_chunking_method(self):
        """
        Selecting best chunking method based on available methods
        """
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

    def check_exes(self):
        """
        Checking required executables
        """

        if not find_executable("ffmpeg"):
            print("No ffmpeg")
            sys.exit(1)
        else:
            log(get_ffmpeg_info())

        if self.chunk_method in ["vs_ffms2", "vs_lsmash"]:
            if not find_executable("vspipe"):
                print("vspipe executable not found")
                sys.exit(1)

            try:
                import vapoursynth

                plugins = vapoursynth.get_core().get_plugins()

                if (
                    self.chunk_method == "vs_lsmash"
                    and "systems.innocent.lsmas" not in plugins
                ):
                    print("lsmas is not installed")
                    sys.exit(1)

                if (
                    self.chunk_method == "vs_ffms2"
                    and "com.vapoursynth.ffms2" not in plugins
                ):
                    print("ffms2 is not installed")
                    sys.exit(1)
            except ModuleNotFoundError:
                print("Vapoursynth is not installed")
                sys.exit(1)
