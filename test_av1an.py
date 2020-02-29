import pytest
import os
from av1an import Av1an


def test_log():
    av = Av1an()
    av.logging = 'test_log'
    av.log('test')

    with open('test_log', 'r') as f:
        r = f.read()
        print(r)

    os.remove('test_log')

    assert 'test' in r

