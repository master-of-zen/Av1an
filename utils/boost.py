#!/bin/env python

from utils.utils import man_cq, get_cq
import sys

def boosting(command: str, brightness, b_limit, b_range, new_cq=0):
    """Based on average brightness of video decrease(boost) Quantize value for encoding."""
    try:
        cq = get_cq(command)
        if not new_cq:
            if brightness < 128:
                new_cq = cq - round((128 - brightness) / 128 * b_range)
                new_cq = max(b_limit, new_cq)

            else:
                new_cq = cq
        cmd = man_cq(command, new_cq)

        return cmd, new_cq

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')