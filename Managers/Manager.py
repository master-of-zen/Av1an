import sys
import time
from Startup.file_validation import process_inputs
from pathlib import Path
from Projects.Project import Project
from Av1an.encode import encode_file

class Main:

    def __init__(self, args):
        self.file_queue: list[Path] = []
        self.args = args
        self.file_queue = process_inputs(args.input)
        self.projects = self.create_project_list()

    def create_project_list(self):
        """
        Returns list of initialized Project objects with single input
        """
        queue = []
        for file in self.file_queue:
            project = Project(vars(self.args))
            project.input = file
            project.outputs_filenames()
            queue.append(project)
        return queue


    def run(self):
        """
        Run encoding in queue or single file
        """
        for i, proj in enumerate(self.projects):
            proj.outputs_filenames()

            if proj.output_file.exists() and len(self.projects) > 1:
                print(f":: Skipping file {proj.input.name}\n:: Outputfile {proj.output_file.name} exists")

                # Don't print new line on last project to console
                if i+1 < len(self.projects):
                    print()

                continue
            try:
                tm = time.time()

                if len(self.projects) > 1:
                    print(f":: Encoding file {proj.input.name}")

                encode_file(proj)

                print(f'Finished: {round(time.time() - tm, 1)}s\n')
            except KeyboardInterrupt:
                print('Encoding stopped')
                sys.exit()



class EncodingManager:

    def __init__(self):
        self.workers = None

