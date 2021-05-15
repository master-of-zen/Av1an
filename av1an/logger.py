#!/bin/env python

import sys
import time
from pathlib import Path

# Todo: Add self testing on startup
class Logger:
    def __init__(self):
        self.set_file = False
        self.buffer = ""

    def set_path(self, file):
        self.set_file = Path(file)

    def log(self, *info):
        """Default logging function, write to file."""
        for i in info:
            if self.set_file and self.buffer:
                with open(self.set_file, "a") as logf:
                    logf.write(self.buffer)
                    self.buffer = None

            if self.set_file:
                with open(self.set_file, "a") as logf:
                    logf.write(f'[{time.strftime("%X")}] {i}\n')
            else:
                self.buffer += f'[{time.strftime("%X")}] {i}\n'


# Creating logger
logger = Logger()
log_file = logger.set_path
log = logger.log


def set_log(log_path: Path, temp):
    """Setting logging file location"""

    if log_path:
        log_path = Path(log_path)
        if log_path.suffix == "":
            log_path = log_path.with_suffix(".log")
        log_file(log_path)

    else:
        log_file(temp / "log.log")

    log(f"Av1an Started")
    log(f"Command:")
    log(f"{' '.join(sys.argv)}")
