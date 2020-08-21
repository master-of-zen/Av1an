import shlex
import subprocess
import sys
from pathlib import Path
from subprocess import PIPE, STDOUT

from Av1an.arg_parse import Args
from Av1an.logger import log
from Av1an.utils import terminate


def concat_routine(args: Args):
    """
    Runs the concatenation routine with args

    :param args: the Args
    :return: None
    """
    try:
        if args.encoder == 'vvc':
            vvc_concat(args.temp, args.output_file.with_suffix('.h266'))
        elif args.mkvmerge:
            concatenate_mkvmerge(args.temp, args.output_file)
        else:
            concatenate_ffmpeg(args.temp, args.output_file, args.encoder)
    except Exception as e:
        _, _, exc_tb = sys.exc_info()
        print(f'Concatenation failed, error\nAt line: {exc_tb.tb_lineno}\nError:{str(e)}')
        log(f'Concatenation failed, aborting, error: {e}\n')
        terminate()


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


def concatenate_ffmpeg(temp: Path, output, encoder: str):
    """
    Uses ffmpeg to concatenate encoded segments into the final file

    :param temp: the temp directory
    :param output: the final output file
    :param encoder: the encoder
    :return: None
    """
    """With FFMPEG concatenate encoded segments into final file."""

    log('Concatenating\n')

    with open(temp / "concat", 'w') as f:

        encode_files = sorted((temp / 'encode').iterdir())
        # Replace all the ' with '/'' so ffmpeg can read the path correctly
        f.writelines(f'file {shlex.quote(str(file.absolute()))}\n' for file in encode_files)

    # Add the audio file if one was extracted from the input
    audio_file = temp / "audio.mkv"
    if audio_file.exists():
        audio = ('-i', audio_file, '-c:a', 'copy', '-map', '1')
    else:
        audio = ()

    if encoder == 'x265':

        cmd = ['ffmpeg', '-y', '-fflags', '+genpts', '-hide_banner', '-loglevel', 'error', '-f', 'concat', '-safe', '0',
               '-i', temp / "concat", *audio, '-c', 'copy', '-movflags', 'frag_keyframe+empty_moov', '-map', '0', '-f',
               'mp4', output]
        concat = subprocess.run(cmd, stdout=PIPE, stderr=STDOUT).stdout

    else:
        cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i',
               temp / "concat", *audio, '-c', 'copy', '-map', '0', output]

        concat = subprocess.run(cmd, stdout=PIPE, stderr=STDOUT).stdout

    if len(concat) > 0:
        log(concat.decode())
        print(concat.decode())
        raise Exception


def concatenate_mkvmerge(temp: Path, output):
    """
    Uses mkvmerge to concatenate encoded segments into the final file

    :param temp: the temp directory
    :param output: the final output file
    :return: None
    """

    log('Concatenating\n')

    encode_files = sorted((temp / 'encode').iterdir())
    concat = ' +'.join(shlex.quote(f.as_posix()) for f in encode_files)

    audio_file = temp / "audio.mkv"
    audio = audio_file.as_posix() if audio_file.exists() else ''

    cmd = f' mkvmerge {concat} {shlex.quote(audio)} -o {shlex.quote(output.as_posix())}'
    concat = subprocess.Popen(cmd, stdout=PIPE, universal_newlines=True, shell=True)
    output, err = concat.communicate()
    concat.wait()

    if concat.returncode != 0:
        log(output)
        print(output)
        raise Exception
