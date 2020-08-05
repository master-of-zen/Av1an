import json
from pathlib import Path
from threading import Lock


doneFileLock = Lock()


def read_done_data(temp: Path):
    try:
        doneFileLock.acquire()
        done_path = temp / 'done.json'
        with open(done_path) as done_file:
            data = json.load(done_file)
    finally:
        if doneFileLock.locked():
            doneFileLock.release()
    return data


def write_progress_file(progress_file: Path, chunk, encoded_frames: int):
    try:
        doneFileLock.acquire()
        with progress_file.open() as f:
            d = json.load(f)
        d['done'][chunk.name] = encoded_frames
        with progress_file.open('w') as f:
            json.dump(d, f)
    finally:
        if doneFileLock.locked():
            doneFileLock.release()
