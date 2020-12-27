from multiprocessing.managers import BaseManager
from tqdm import tqdm

def Manager():
    """
    Thread save manager for frame counter
    """
    m = BaseManager()
    m.start()
    return m


class Counter:
    """
    Frame Counter based on TQDM
    """
    def __init__(self, total, initial):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.tqdm_bar = tqdm(total=self.left, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.01)

    def update(self, value):
        if self.first_update:
            self.tqdm_bar.reset(self.left)
            self.first_update = False
        self.tqdm_bar.update(value)

    def close(self):
        self.tqdm_bar.close()


