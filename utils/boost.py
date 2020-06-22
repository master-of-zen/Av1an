#!/bin/env python

from utils.utils import man_cq, get_cq
import sys
from utils.utils import get_brightness
from .logger import log, set_log_file

def boost(command: str, brightness, b_limit, b_range, new_cq=0):
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

def boosting(bl, br, source, commands, passes):
    try:
        brightness = get_brightness(source.absolute().as_posix())
        com0, cq = boost(commands[0], brightness, bl, br )

        if passes == 2:
            com1, _ = boost(commands[1], brightness, bl, br, new_cq=cq)
            commands = (com0, com1) + commands[2:]
        else:
            commands = com0 + commands[1:]
        log(f'{source.name}\n[Boost]\nAvg brightness: {br}\nAdjusted CQ: {cq}\n\n')
        return commands, cq

    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Error in encoding loop {e}\nAt line {exc_tb.tb_lineno}')