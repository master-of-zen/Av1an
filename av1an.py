#!/usr/bin/env python3

from av1an import Args
from manager import Manager
from vmaf import VMAF
from startup.setup import startup_check

if __name__ == '__main__':
    parser = Args()
    project = parser.get_project()
    startup_check(project)
    manager = Manager.Main(project)
    manager.run()
