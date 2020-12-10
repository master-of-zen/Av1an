import json
from pathlib import Path
from threading import Lock


done_file_lock = Lock()


def read_done_data(temp: Path):
    """
    Reads the json data from the done file

    :param temp: the temp directory
    :return: json data
    """
    try:
        done_file_lock.acquire()
        done_path = temp / 'done.json'
        with open(done_path) as done_file:
            data = json.load(done_file)
    finally:
        if done_file_lock.locked():
            done_file_lock.release()
    return data


def write_progress_file(progress_file: Path, chunk, encoded_frames: int):
    """
    Updates the given chunk in the progress (.temp/done.json) file

    :param progress_file: the .temp/done.json file
    :param chunk: the chunk that was finished
    :param encoded_frames: how many frames were encoded for the chunk
    :return: None
    """
    try:
        done_file_lock.acquire()
        with progress_file.open() as f:
            d = json.load(f)
        d['done'][chunk.name] = encoded_frames
        with progress_file.open('w') as f:
            json.dump(d, f)
    finally:
        if done_file_lock.locked():
            done_file_lock.release()
