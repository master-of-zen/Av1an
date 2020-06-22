import sys
import subprocess
from subprocess import PIPE, STDOUT
import re
from multiprocessing.managers import BaseManager
from tqdm import tqdm
from utils.utils import terminate
from .logger import log, set_log_file

# Stuff for updating encoded progress in real-time
class MyManager(BaseManager):
    pass

def Manager():
    m = MyManager()
    m.start()
    return m


class Counter():
    def __init__(self, total, initial):
        self.first_update = True
        self.initial = initial
        self.left = total - initial
        self.tqdm_bar = tqdm(total=self.left, initial=0, dynamic_ncols=True, unit="fr", leave=True, smoothing=0.2)

    def update(self, value):
        if self.first_update:
            self.tqdm_bar.reset(self.left)
            self.first_update = False
        self.tqdm_bar.update(value)


MyManager.register('Counter', Counter)


def tqdm_bar(i, encoder, counter, frame_probe_source, passes):
    try:
        f, e = i.split('|')
        f = " ffmpeg -y -hide_banner -loglevel error " + f
        f, e = f.split(), e.split()
        frame = 0
        ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
        pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                                stderr=STDOUT,
                                universal_newlines=True)

        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break

            if len(line) == 0:
                continue

            if encoder in ('aom', 'vpx', 'rav1e'):
                match = None
                if encoder in ('aom', 'vpx'):
                    if 'fatal' in line.lower():
                        print('\n\nERROR IN ENCODING PROCESS\n\n', line)
                        terminate()
                    if 'Pass 2/2' in line or 'Pass 1/1' in line:
                        match = re.search(r"frame.*?\/([^ ]+?) ", line)
                elif encoder == 'rav1e':
                    if 'error' in line.lower():
                        print('\n\nERROR IN ENCODING PROCESS\n\n', line)
                        terminate()
                    match = re.search(r"encoded.*? ([^ ]+?) ", line)

                if match:
                    new = int(match.group(1))
                    if new > frame:
                        counter.update(new - frame)
                        frame = new

            if encoder == 'svt_av1':
                counter.update(frame_probe_source // passes)
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error at encode {e}\nAt line {exc_tb.tb_lineno}')