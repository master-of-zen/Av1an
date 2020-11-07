import os
import platform
import shlex
import subprocess
import sys
from pathlib import Path
from subprocess import PIPE, STDOUT

from libAv1an.LibAv1an.args import Args
from libAv1an.LibAv1an.callbacks import Callbacks


def concat_routine(args: Args, cb: Callbacks):
    """
    Runs the concatenation routine with args

    :param args: the Args
    :param cb: the callbacks
    :return: None
    """
    try:
        if args.encoder == 'vvc':
            vvc_concat(args.temp, args.output_file.with_suffix('.h266'))
        elif args.mkvmerge:
            concatenate_mkvmerge(args.temp, args.output_file, cb)
        else:
            concatenate_ffmpeg(args.temp, args.output_file, args.encoder, cb)
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Concatenation failed, error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
        cb.run_callback("log", f'Concatenation failed, aborting, error: {e}\n')
        cb.run_callback("terminate", 1)


def vvc_concat(temp: Path, output: Path):
    """
    Concatenates vvc files

    :param temp: the temp directory
    :param output: the output video
    :return: None
    """
    encode_files = sorted((temp / 'encode').iterdir())
    bitstreams = [x.as_posix() for x in encode_files]
    bitstreams = ' '.join(bitstreams)
    cmd = f'vvc_concat  {bitstreams} {output.as_posix()}'

    output = subprocess.run(cmd, shell=True)


def concatenate_ffmpeg(temp: Path, output: Path, encoder: str, cb: Callbacks):
    """
    Uses ffmpeg to concatenate encoded segments into the final file

    :param temp: the temp directory
    :param output: the final output file
    :param encoder: the encoder
    :param cb: the callbacks
    :return: None
    """
    """With FFMPEG concatenate encoded segments into final file."""

    cb.run_callback("log", 'Concatenating\n')

    with open(temp / "concat", 'w') as f:

        encode_files = sorted((temp / 'encode').iterdir())
        # Replace all the ' with '/'' so ffmpeg can read the path correctly
        f.writelines(f'file {shlex.quote(str(file.absolute()))}\n' for file in encode_files)

    # Add the audio file if one was extracted from the input
    audio_file = temp / "audio.mkv"
    if audio_file.exists():
        audio = ('-i', audio_file.as_posix(), '-c:a', 'copy', '-map', '1')
    else:
        audio = ()

    if encoder == 'x265':

        cmd = ['ffmpeg', '-y', '-fflags', '+genpts', '-hide_banner', '-loglevel', 'error', '-f', 'concat', '-safe', '0',
               '-i', (temp / "concat").as_posix(), *audio, '-c', 'copy', '-movflags', 'frag_keyframe+empty_moov',
               '-map', '0', '-f', 'mp4', output.as_posix()]
        concat = subprocess.run(cmd, stdout=PIPE, stderr=STDOUT).stdout

    else:
        cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i',
               (temp / "concat").as_posix(), *audio, '-c', 'copy', '-map', '0', output.as_posix()]

        concat = subprocess.run(cmd, stdout=PIPE, stderr=STDOUT).stdout

    if len(concat) > 0:
        cb.run_callback("log", concat.decode())
        print(concat.decode())
        raise Exception


def concatenate_mkvmerge(temp: Path, output, cb: Callbacks):
    """
    Uses mkvmerge to concatenate encoded segments into the final file

    :param temp: the temp directory
    :param output: the final output file
    :param cb: the callbacks
    :return: None
    """

    cb.run_callback("log", 'Concatenating\n')

    output = shlex.quote(output.as_posix())

    encode_files = sorted((temp / 'encode').iterdir(),
                          key=lambda x: int(x.stem)
                          if x.stem.isdigit() else x.stem)
    encode_files = [shlex.quote(f.as_posix()) for f in encode_files]

    if platform.system() == "Linux":
        import resource
        file_limit, _ = resource.getrlimit(resource.RLIMIT_NOFILE)
        cmd_limit = os.sysconf(os.sysconf_names['SC_ARG_MAX'])
    else:
        file_limit = -1
        cmd_limit = 32767

    audio_file = temp / "audio.mkv"
    audio = audio_file.as_posix() if audio_file.exists() else ''

    if len(encode_files) > 1:
        encode_files = [
            _concatenate_mkvmerge(encode_files, output, file_limit, cmd_limit, cb)
        ]

    cmd = ['mkvmerge', '-o', output, encode_files[0]]

    if audio:
        cmd.append(audio)

    concat = subprocess.Popen(cmd, stdout=PIPE, universal_newlines=True)
    message, _ = concat.communicate()
    concat.wait()

    if concat.returncode != 0:
        cb.run_callback("log", message)
        print(message)
        raise Exception

    # remove temporary files used by recursive concat
    if os.path.exists("{}.tmp0.mkv".format(output)):
        os.remove("{}.tmp0.mkv".format(output))

    if os.path.exists("{}.tmp1.mkv".format(output)):
        os.remove("{}.tmp1.mkv".format(output))


def _concatenate_mkvmerge(files, output, file_limit, cmd_limit, cb: Callbacks, flip=False):
    tmp_out = "{}.tmp{}.mkv".format(output, int(flip))
    cmd = ["mkvmerge", "-o", tmp_out, files[0]]

    remaining = []
    for i, file in enumerate(files[1:]):
        new_cmd = cmd + ['+{}'.format(file)]
        if sum(len(s) for s in new_cmd) < cmd_limit \
            and (file_limit == -1 or i < max(1, file_limit - 10)):
            cmd = new_cmd
        else:
            remaining = files[i + 1:]
            break

    concat = subprocess.Popen(cmd, stdout=PIPE, universal_newlines=True)
    message, _ = concat.communicate()
    concat.wait()

    if concat.returncode != 0:
        cb.run_callback("log", message)
        print(message)
        raise Exception

    if len(remaining) > 0:
        return _concatenate_mkvmerge(
            [tmp_out] + remaining, output, file_limit, cmd_limit, not flip)
    else:
        return tmp_out
