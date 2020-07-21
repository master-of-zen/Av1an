#!/usr/bin/env python3


import shutil
import subprocess
import sys
import time
from pathlib import Path
import json

from utils import *


class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encoding"""
    def __init__(self):
        self.args = arg_parsing()

    def main_thread(self):
        """Main."""
        startup_check(self.args)
        conf(self.args)
        main_queue(self.args)


def main():
    Av1an().main_thread()


if __name__ == '__main__':
    main()
