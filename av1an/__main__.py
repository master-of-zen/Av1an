from .arg_parse import Args
from .manager import Manager
from .startup.setup import startup_check


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