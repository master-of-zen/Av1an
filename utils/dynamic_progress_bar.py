import subprocess
from subprocess import PIPE, STDOUT
import re


def tqdm_bar(i, encoder, counter, frame_probe_source, passes):
    f, e = i.split('|')
    f = " ffmpeg -y -hide_banner -loglevel error " + f
    f, e = f.split(), e.split()
    frame = 0
    ffmpeg_pipe = subprocess.Popen(f, stdout=PIPE, stderr=STDOUT)
    pipe = subprocess.Popen(e, stdin=ffmpeg_pipe.stdout, stdout=PIPE,
                            stderr=STDOUT,
                            universal_newlines=True)

    if encoder in ('aom', 'vpx'):
        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break
            if 'Pass 2/2' in line or 'Pass 1/1' in line:
                match = re.search(r"frame.*?\/([^ ]+?) ", line)
                if match:
                    new = int(match.group(1))
                if new > frame:
                    counter.update(new - frame)
                    frame = new

    elif encoder == 'rav1e':
        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break
            match = re.search(r"encoded.*? ([^ ]+?) ", line)
            if match:
                new = int(match.group(1))
            if new > frame:
                counter.update(new - frame)
                frame = new

    elif encoder == 'svt_av1':
        while True:
            line = pipe.stdout.readline().strip()
            if len(line) == 0 and pipe.poll() is not None:
                break
            counter.update(frame_probe_source // passes)