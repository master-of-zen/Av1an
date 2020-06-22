#!/usr/bin/env python3
import time
from pathlib import Path
import sys
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
set_log_file = l.set_path
log = l.log


def set_logging(log_path: Path, temp):
        """Setting logging file location"""
        if log_path:
            log_path = Path(log_path)
            if log_path.suffix == '':
                log_path = log_path.with_suffix('.log')
            set_log_file(log_path)
        else:
            set_log_file(temp / 'log.log')

        log(f"Av1an Started\nCommand:\n{' '.join(sys.argv)}\n")