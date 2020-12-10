#!/usr/bin/env python3

from Av1an.arg_parse import Args
from Startup.setup import startup_check
from Managers import Manager

class Av1an:
    """Av1an - Python framework for AV1, VP9, VP8 encoding"""
    def __init__(self):
        parser = Args()
        self.args = parser.get_project()

    def main_thread(self):
        """Main."""
        startup_check(self.args)

        manager = Manager.Main(self.args)

        manager.run()
        # main_queue(self.args)


def main():
    Av1an().main_thread()


if __name__ == '__main__':
    main()
