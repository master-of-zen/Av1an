#!/bin/env python
import sys
import json
from pathlib import Path
from .project import Project
from av1an_pyo3 import parse_args, default_args


class Args:
    """
    Class responsible for arg parsing
    Creation of original project file
    """

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
