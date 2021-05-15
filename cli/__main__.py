#!/usr/bin/env python3
from av1an.arg_parse import Args
from av1an.manager import Manager
from av1an.startup.setup import startup_check


def main():
    """
    Running Av1an CLI
    """
    parser = Args()
    project = parser.get_project()
    startup_check(project)
    manager = Manager.Main(project)
    manager.run()


if __name__ == "__main__":
    main()
