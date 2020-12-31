

from collections import deque

class Queue:
    """
    Queue manager with ability to add/remove/restart jobs
    """
    def __init__(self, workers):
        self.queue = deque(maxlen=workers)