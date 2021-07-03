from multiprocessing.managers import BaseManager
from av1an_pyo3 import update_bar, init_progress_bar, finish_progress_bar


def Manager():
    """
    Thread save manager for frame counter
    """
    m = BaseManager()
    m.start()
    return m


class Counter:
    def __init__(self, total, initial, quiet):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.current = 0
        self.quiet = quiet
        init_progress_bar(total)

    def update(self, value):
        update_bar(value)

    def close(self):
        finish_progress_bar()

    def get_frames(self):
        return self.current
