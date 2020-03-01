import pytest
import shutil
import os
from av1an import Av1an
from pathlib import Path


def test_log():
    av = Av1an()
    av.logging = 'test_log'
    av.log('test')

    with open('test_log', 'r') as f:
        r = f.read()

    os.remove('test_log')

    assert  'test' in r


def test_call_cmd():
    av = Av1an()
    out = av.call_cmd('ls', capture_output=True)
    assert len(out) > 0


def test_arg_parsing():
    pass


def test_setup():
    av = Av1an()
    open('test', 'w')
    # av.args.resume = False

    av.setup(Path('.'))
    assert Path('.temp').exists()

    os.remove('test')
    shutil.rmtree('.temp')


def test_extract_audio():
    pass


def test_scene_detect():
    pass


def test_split():
    pass


def test_frame_probe():
    pass


def test_frame_check():
    pass


def test_get_video_queue():
    pass


def test_compose_encoding_queue():
    pass


def test_encode():
    pass


def test_concatenate_video():
    pass


def test_image_encoding():
    pass


def test_encoding_loop():
    pass


def test_video_encoding():
    pass