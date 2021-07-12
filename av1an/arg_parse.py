#!/bin/env python
import json
import sys
import shlex
from pathlib import Path

from av1an_pyo3 import (
    default_args,
    parse_args,
    hash_path,
    get_default_pass,
    Project,
    is_vapoursynth,
)


class Args:
    def __init__(self):
        self.defaults = json.loads(default_args())
        self.parsed = None
        self.project = None

    def get_project(self):
        """
        Create and return project object with all parameters
        """
        self.parse()

        self.project = Project(self.parsed)

        return self.project

    def parse(self):
        """
        Parse command line parameters provided by user
        """
        self.parsed = json.loads(parse_args())
        self.parsed["ffmpeg"] = self.parsed["ffmpeg"] if self.parsed["ffmpeg"] else ""
        if self.parsed["temp"] is None:
            self.parsed["temp"] = f".{hash_path(self.parsed['input'])}"
        self.parsed["mkvmerge"] = False
        self.parsed["output_ivf"] = False

        if self.parsed["passes"] is None:
            self.parsed["passes"] = get_default_pass(self.parsed["encoder"])

        if self.parsed["video_params"] is None:
            self.parsed["video_params"] = []
        else:
            self.parsed["video_params"] = shlex.split(self.parsed["video_params"])

        self.parsed["audio_params"] = shlex.split(self.parsed["audio_params"])
        self.parsed["ffmpeg_pipe"] = []
        if self.parsed["logging"] is None:
            self.parsed["logging"] = (
                (Path(self.parsed["temp"]) / "log").resolve().as_posix()
            )
        self.parsed["frames"] = 0
        self.parsed["is_vs"] = is_vapoursynth(self.parsed["input"])

        if not self.parsed["input"]:
            print("No input")
            sys.exit()
