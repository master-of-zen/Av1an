#!/usr/bin/env python3
from pathlib import Path
from av1an.arg_parse import Args
import sys
import os
import atexit

import time


def main():
    """
    Running Av1an CLI
    """

    parser = Args()
    project = parser.get_project()
    project.startup_check()

    if sys.platform == "linux":

        def restore_term():
            os.system("stty sane")

        atexit.register(restore_term)

    if Path(project.output_file).exists():
        print(
            f":: Output file '{project.output_file}' exists, overwrite? [y/n or enter]: ",
            end="",
        )
        promt = input()
        print("\r\033[1A\033[0K", end="")
        if "y" in promt.lower() or promt.strip() == "":
            pass
        else:
            print("Stopping")
            sys.exit()

    try:
        tm = time.time()
        project.encode_file()

        print(f"Finished: {round(time.time() - tm, 1)}s")
    except KeyboardInterrupt:
        print("Encoding stopped")
        sys.exit()


if __name__ == "__main__":
    main()
