import re
import subprocess
import sys
from subprocess import PIPE
from av1an.encoder import ENCODERS
from typing import List, Union
from difflib import SequenceMatcher


def run_command(command: List) -> str:
    r = subprocess.run(command, stdout=PIPE, stderr=PIPE)
    return r.stderr.decode("utf-8") + r.stdout.decode("utf-8")


def sort_params(params: List) -> List:
    """
    Sort arguments to 2 list based on -/--
    Return 2 lists of arguments
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
        if not any(opt == pr for opt in valid_options):
            invalid.append(pr)

    return invalid


def suggest_fix(wrong_arg, arg_dictionary):
    arg_dictionary = list(arg_dictionary)
    scores = [(SequenceMatcher(None, wrong_arg, b).ratio(), b)
              for b in arg_dictionary]
    return max(scores, key=lambda x: x[0])[1]


def get_encoder_project(project):
    help_command = ENCODERS[project.encoder].encoder_help.split()

    help_text = run_command(help_command)

    matches = re.findall(r'\s+(-\w+|(?:--\w+(?:-\w+)*))', help_text)
    parameters = set(matches)

    return parameters


def validate_inputs(project):
    video_params = project.video_params

    video_params = [x.split('=')[0] for x in video_params if x[0] == "-"]

    parameters = get_encoder_project(project)

    suggested = [(x, suggest_fix(x, parameters))
                 for x in match_commands(video_params, parameters)]

    if len(suggested) > 0:
        print('WARNING: Invalid params:')
        for cmd in suggested:
            print(f"'{cmd[0]}' isn't a valid param for {project.encoder}. Did you mean '{cmd[1]}'?")
        if not project.force:
            print('To continue anyway, run Av1an with --force')
            sys.exit(1)
