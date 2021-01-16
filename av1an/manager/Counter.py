from multiprocessing.managers import BaseManager
try:
    from tqdm import tqdm
except ImportError:
    tqdm = None


def Manager():
    """
    Thread save manager for frame counter
    """
    m = BaseManager()
    m.start()
    return m


class Counter:
    """
    Frame Counter
    """
    def __init__(self, total, initial, use_tqdm=True):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.current = 0
        self.use_tqdm = (use_tqdm and (tqdm is not None))
        if use_tqdm:
            self.tqdm_bar = tqdm(total=self.left, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.01)

    def update(self, value):
        if self.use_tqdm:
            if self.first_update:
                self.tqdm_bar.reset(self.left)
                self.first_update = False
            self.tqdm_bar.update(value)
        else:
            self.current += value

    def close(self):
        if self.use_tqdm:
            self.tqdm_bar.close()

    def get_frames(self):
        return self.current

