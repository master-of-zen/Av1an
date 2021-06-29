#!/bin/env python
import json
import sys
from pathlib import Path

from av1an_pyo3 import default_args, parse_args

from .project import Project


class Args:
    def __init__(self):

        self.defaults = json.loads(default_args())
        self.parsed = None
        self.project = None

    def get_project(self):
        """
        Create and return project object with all parameters
        """
        if not self.parsed:
            self.parse()

        self.project = Project(self.parsed)

        return self.project

    def parse(self):
        """
        Parse command line parameters provided by user
        """
        self.parsed = json.loads(parse_args())
        self.parsed["input"] = [Path(self.parsed["input"])]
        self.parsed["ffmpeg"] = self.parsed["ffmpeg"] if self.parsed["ffmpeg"] else ""
        if not self.parsed["input"]:
            print("No input")
            sys.exit()
