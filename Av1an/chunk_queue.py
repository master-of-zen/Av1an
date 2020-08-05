from pathlib import Path
from typing import List

from .arg_parse import Args
from .chunk import Chunk
from .compose import get_file_extension_for_encoder
from .ffmpeg import frame_probe
from .logger import log
from .split import segment
from .utils import terminate


def create_encoding_queue(args: Args, split_locations: List[int]) -> List[Chunk]:
    """
    Creates a list of chunks using the cli option specified

    :param args: Args
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """
    chunk_queue = create_video_queue_segment(args, split_locations)

    # Sort largest first so chunks that take a long time to encode start first
    chunk_queue.sort(key=lambda c: c.size, reverse=True)
    return chunk_queue


def create_video_queue_segment(args: Args, split_locations: List[int]) -> List[Chunk]:
    """
    Create a list of chunks using segmented files

    :param args: Args
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """

    # segment into separate files
    segment(args.input, args.temp, split_locations)

    # get the names of all the split files
    source_path = args.temp / 'split'
    queue_files = [x for x in source_path.iterdir() if x.suffix == '.mkv']
    queue_files.sort(key=lambda p: p.stem)

    if len(queue_files) == 0:
        er = 'Error: No files found in temp/split, probably splitting not working'
        print(er)
        log(er)
        terminate()

    chunk_queue = [create_chunk_from_file(args, index, file) for index, file in enumerate(queue_files)]

    return chunk_queue


def create_chunk_from_file(args: Args, index: int, file: Path) -> Chunk:
    """
    Creates a Chunk object from a file

    :param args: the Args
    :param index: the index of the chunk
    :param file: the segmented file
    :return: A Chunk
    """
    ffmpeg_gen_cmd = f'ffmpeg -y -hide_banner -loglevel error -i {file.as_posix()} {args.pix_format} -f yuv4mpegpipe -'
    file_size = file.stat().st_size
    frames = frame_probe(file)
    extension = get_file_extension_for_encoder(args.encoder)

    chunk = Chunk(index, ffmpeg_gen_cmd, file_size, args.temp, frames, extension)
    chunk.generate_pass_cmds(args)

    return chunk
