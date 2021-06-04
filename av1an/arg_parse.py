#!/bin/env python
import argparse
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

        if self.project.config:
            self.save_load_project_file()

        return self.project

    def get_difference(self) -> dict:
        """
        Return difference of defaults and new
        """
        return dict([x for x in self.parsed.items() if x not in self.defaults.items()])

    def parse(self):
        """
        Parse command line parameters provided by user
        """
        self.parsed = json.loads(parse_args())
        self.parsed["input"] = [Path(self.parsed["input"])]
        if not self.parsed["input"]:
            print("No input")
            sys.exit()

    def save_load_project_file(self):
        """
        Saves current/Loads given project file, loads saved project first and when overwrites only unique values from current parse
        """
        cfg_path = Path(self.project.config)

        if cfg_path.exists():

            new = self.get_difference()
            self.project.load_project_from_file(self.project.config)
            self.project.load_project(new)

        else:
            self.project.save_project_to_file(self.project.config)
