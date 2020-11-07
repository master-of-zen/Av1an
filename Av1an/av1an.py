#!/usr/bin/env python3

from Av1an.config import conf
from Startup.validation import validate_inputs
from Startup.startup import startup_check
from libAv1an.LibAv1an.encode import main_queue
from Av1an.arg_parse import arg_parsing
from Av1an.handle_callbacks import add_callbacks
from libAv1an.LibAv1an.callbacks import Callbacks


class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encoding"""
    def __init__(self):
        self.args = arg_parsing()

    def main_thread(self):
        """Main."""
        startup_check(self.args)
        conf(self.args)
        validate_inputs(self.args)
        c = add_callbacks(self.args)
        main_queue(self.args, c)


def main():
    Av1an().main_thread()


if __name__ == '__main__':
    main()
