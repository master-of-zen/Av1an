#!/bin/env python
import numpy as np


def process_inputs(inputs):
    # Check input file for being valid
    if not inputs:
        print('No input file')
        exit()

    input_list = []

    for item in inputs:
        if item.is_dir():
            new_inputs = [x for x in item.iterdir() if x.suffix in (".mkv", ".mp4", ".mov", ".avi", ".flv", ".m2ts")]
            input_list.extend(new_inputs)
        else:
            input_list.append(item)

    valid = np.array([i.exists() for i in input_list])

    if not all(valid):
        print(f'File(s) do not exist: {", ".join([str(input_list[i]) for i in np.where(not valid)[0]])}')
        exit()

    return input_list
