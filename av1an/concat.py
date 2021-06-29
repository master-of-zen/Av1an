import os
import platform
import shlex
import subprocess
import sys
from pathlib import Path
from subprocess import PIPE

from av1an_pyo3 import log

if platform.system() == "Linux":
    import resource


def concatenate_mkvmerge(temp: Path, output):
    output = shlex.quote(output.as_posix())

    encode_files = sorted(
        (temp / "encode").iterdir(),
        key=lambda x: int(x.stem) if x.stem.isdigit() else x.stem,
    )
    encode_files = [shlex.quote(f.as_posix()) for f in encode_files]

    if platform.system() == "Linux":
        file_limit, _ = resource.getrlimit(resource.RLIMIT_NOFILE)
        cmd_limit = os.sysconf(os.sysconf_names["SC_ARG_MAX"])
    else:
        file_limit = -1
        cmd_limit = 32767

    audio_file = temp / "audio.mkv"
    audio = audio_file.as_posix() if audio_file.exists() else ""

    if len(encode_files) > 1:
        encode_files = [
            _concatenate_mkvmerge(encode_files, output, file_limit, cmd_limit)
        ]

    cmd = ["mkvmerge", "-o", output, encode_files[0]]

    if audio:
        cmd.append(audio)

    concat = subprocess.Popen(cmd, stdout=PIPE, universal_newlines=True)
    message, _ = concat.communicate()
    concat.wait()

    if concat.returncode != 0:
        log(message)
        print(message)
        tb = sys.exc_info()[2]
        raise RuntimeError.with_traceback(tb)

    # remove temporary files used by recursive concat
    if os.path.exists("{}.tmp0.mkv".format(output)):
        os.remove("{}.tmp0.mkv".format(output))

    if os.path.exists("{}.tmp1.mkv".format(output)):
        os.remove("{}.tmp1.mkv".format(output))


def _concatenate_mkvmerge(files, output, file_limit, cmd_limit, flip=False):
    tmp_out = "{}.tmp{}.mkv".format(output, int(flip))
    cmd = ["mkvmerge", "-o", tmp_out, files[0]]

    remaining = []
    for i, file in enumerate(files[1:]):
        new_cmd = cmd + ["+{}".format(file)]
        if sum(len(s) for s in new_cmd) < cmd_limit and (
            file_limit == -1 or i < max(1, file_limit - 10)
        ):
            cmd = new_cmd
        else:
            remaining = files[i + 1 :]
            break

    concat = subprocess.Popen(cmd, stdout=PIPE, universal_newlines=True)
    message, _ = concat.communicate()
    concat.wait()

    if concat.returncode != 0:
        log(message)
        print(message)
        tb = sys.exc_info()[2]
        raise RuntimeError.with_traceback(tb)

    if len(remaining) > 0:
        return _concatenate_mkvmerge(
            [tmp_out] + remaining, output, file_limit, cmd_limit, not flip
        )
    return tmp_out
