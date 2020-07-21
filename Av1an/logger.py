#!/bin/env python

import sys
import time
from pathlib import Path


class Logger():
    def __init__(self):
        self.set_file = False

    def set_path(self, file):
        self.set_file = Path(file)

    def log(self, info):
        """Default logging function, write to file."""
        if self.set_file:
            with open(self.set_file, 'a') as log:
                log.write(time.strftime('%X') + ' ' + info)


# Creating logger
l = Logger()
log_file = l.set_path
log = l.log


def set_log(log_path: Path, temp):
    """Setting logging file location"""
    if log_path:
        log_path = Path(log_path)
        if log_path.suffix == '':
            log_path = log_path.with_suffix('.log')
        log_file(log_path)
    else:
        log_file(temp / 'log.log')

    log(f"\nAv1an Started\nCommand:\n{' '.join(sys.argv)}\n")
