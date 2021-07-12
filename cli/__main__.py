#!/usr/bin/env python3
from av1an.arg_parse import Args
from av1an.startup.setup import startup_check
from pathlib import Path
import sys
from av1an_pyo3 import process_inputs, hash_path

import time

from av1an.project.Project import Project
from av1an.manager.Manager import encode_file


def main():
    """
    Running Av1an CLI
    """
    parser = Args()
    project = parser.get_project()
    startup_check(project)
    inputs = [str(x).strip() for x in project.input]
    input_paths = [Path(x) for x in process_inputs(inputs)]

    file_queue = input_paths
    queue = []
    for file in file_queue:
        pj = Project(vars(project))
        pj.input = file
        pj.outputs_filenames()
        pj.promt_output_overwrite()
        queue.append(pj)

    for i, proj in enumerate(queue):
        if proj.output_file.exists() and len(queue) > 1:
            print(
                f":: Skipping file {proj.input.name}\n:: Outputfile {proj.output_file.name} exists"
            )

            # Don't print new line on last project to console
            if i + 1 < len(queue):
                print()

            continue
        try:
            tm = time.time()

            if len(queue) > 1:
                print(f":: Encoding file {proj.input.name}")
            if (project.temp is None) and project.keep:
                print(f":: Temp dir: '.{str(hash_path(str(project.input[i])))}'")

            encode_file(proj)

            print(f"Finished: {round(time.time() - tm, 1)}s")
        except KeyboardInterrupt:
            print("Encoding stopped")
            sys.exit()


if __name__ == "__main__":
    main()
