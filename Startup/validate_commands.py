import re
import subprocess
import shlex
import sys
from subprocess import PIPE
from Encoders import ENCODERS
from typing import List, Union

#TODO: Suggesting on invalid arguments

def run_command(command: List) -> str:
    r = subprocess.run(command, stdout=PIPE, stderr=PIPE)
    return r.stderr.decode("utf-8") + r.stdout.decode("utf-8")


def sort_params(params: List) -> List:
    """
    Sort arguments to 2 list based on -/--
    Return 2 lists of argumens
    """
    # Sort Params
    one_params = []
    two_params = []

    for param in params:
        if param.startswith('--'):
            two_params.append(param)
        elif param.startswith('-'):
            one_params.append(param)

    return one_params, two_params


def match_commands(params: List, valid_options: List) -> Union[str, bool]:
    """
    Check is parameter present in options list
    """
    invalid = []
    for pr in params:
        if not any(opt in pr for opt in valid_options):
            invalid.append(pr)

    return invalid


def validate_inputs(args):
    help_command = ENCODERS[args.encoder].encoder_help.split()

    help_text = run_command(help_command)

    matches = re.findall(r'\s+(-\w+|(?:--\w+(?:-\w+)*))', help_text)
    parameters = set(matches)

    video_params = args.video_params.split() if args.video_params \
            else ENCODERS[args.encoder].default_args

    # Sort arguments and params
    valid1, valid2 = sort_params(parameters)
    args1, args2 = sort_params(video_params)

    # Match arguments
    invalid = match_commands(args1, valid1) + \
              match_commands(args2, valid2)

    if len(invalid) > 0:
        print('Invalid commands:\n',' '.join(invalid),'\nIf you sure: use --force')
        if not args.force:
            sys.exit(0)
