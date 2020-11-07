#!/bin/env python

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


BaseManager.register('Counter', Counter)

bar = None
counter = None


def start_counter(total, init):
    global counter
    counter = Manager().Counter(total, init)


def end_counter():
    global counter
    counter.close()


def svt_vp9_bar(frame_probe_source, passes):
    global counter
    counter.update(frame_probe_source // passes)


def new_tqdm_bar(statusmsg, total):
    print("Making new bar")
    global bar
    bar = tqdm(total=total, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)


def update_tqdm_bar(addFrames):
    global bar
    global counter
    if bar is None:
        counter.update(addFrames)
    else:
        bar.update(addFrames)
