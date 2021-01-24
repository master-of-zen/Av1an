import json
from pathlib import Path
from typing import List
from subprocess import Popen, DEVNULL
from shlex import split

from av1an.project import Project
from av1an.chunk import Chunk
from av1an.encoder import ENCODERS
from av1an.ffmpeg import get_keyframes
from av1an.logger import log
from av1an.resume import read_done_data
from av1an.split import segment
from av1an.utils import terminate


def save_chunk_queue(temp: Path, chunk_queue: List[Chunk]) -> None:
    """
    Writes the chunk queue to the chunks.json file

    :param temp: the temp directory
    :param chunk_queue: the chunk queue
    :return: None
    """
    chunk_dicts = [c.to_dict() for c in chunk_queue]
    with open(temp / 'chunks.json', 'w') as file:
        json.dump(chunk_dicts, file)


def read_chunk_queue(temp: Path) -> List[Chunk]:
    """
    Reads the chunk queue from the chunks.json file

    :param temp: the temp directory
    :return: the chunk queue
    """
    with open(temp / 'chunks.json', 'r') as file:
        chunk_dicts = json.load(file)
    return [Chunk.create_from_dict(cd, temp) for cd in chunk_dicts]


def load_or_gen_chunk_queue(project: Project, resuming: bool, split_locations: List[int]) -> List[Chunk]:
    """
    If resuming, loads the chunk queue and removes already done chunks or
    creates a chunk queue and saves it for resuming later.

    :param project: the Project
    :param resuming: if we are resuming
    :param split_locations: a list of frames to split on
    :return: A chunk queue (list of chunks)
    """
    # if resuming, read chunks from file and remove those already done
    if resuming:
        chunk_queue = read_chunk_queue(project.temp)
        done_chunk_names = read_done_data(project.temp)['done'].keys()
        chunk_queue = [c for c in chunk_queue if c.name not in done_chunk_names]
        return chunk_queue

    # create and save
    chunk_queue = create_encoding_queue(project, split_locations)
    save_chunk_queue(project.temp, chunk_queue)

    return chunk_queue


def create_encoding_queue(project: Project, split_locations: List[int]) -> List[Chunk]:
    """
    Creates a list of chunks using the cli option chunk_method specified

    :param project: Project
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """
    chunk_method_gen = {
        'segment': create_video_queue_segment,
        'select': create_video_queue_select,
        'vs_ffms2': create_video_queue_vsffms2,
        'vs_lsmash': create_video_queue_vslsmash,
        'hybrid': create_video_queue_hybrid
    }
    chunk_queue = chunk_method_gen[project.chunk_method](project, split_locations)

    # Sort largest first so chunks that take a long time to encode start first
    chunk_queue.sort(key=lambda c: c.size, reverse=True)
    return chunk_queue


def create_video_queue_hybrid(project: Project, split_locations: List[int]) -> List[Chunk]:
    """
    Create list of chunks using hybrid segment-select approach

    :param project: the Project
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """
    keyframes = get_keyframes(project.input)

    end = [project.get_frames()]

    splits = [0] + split_locations + end

    segments_list = list(zip(splits, splits[1:]))
    to_split = [x for x in keyframes if x in splits]
    segments = []

    # Make segments
    segment(project.input, project.temp, to_split[1:])
    source_path = project.temp / 'split'
    queue_files = [x for x in source_path.iterdir() if x.suffix == '.mkv']
    queue_files.sort(key=lambda p: p.stem)

    kf_list = list(zip(to_split, to_split[1:] + end))
    for f, (x, y) in zip(queue_files, kf_list):
        to_add = [(f, [s[0] - x, s[1] - x]) for s in segments_list
                 if s[0] >= x and s[1] <= y
                 and s[0] - x < s[1] - x]
        segments.extend(to_add)

    chunk_queue = [create_select_chunk(project, index, file, *cb) for index, (file, cb) in enumerate(segments)]
    return chunk_queue


def create_video_queue_vsffms2(project: Project, split_locations: List[int]) -> List[Chunk]:
    script = "from vapoursynth import core\n" \
             "core.ffms2.Source(\"{}\", cachefile=\"{}\").set_output()"
    return create_video_queue_vs(project, split_locations, script)


def create_video_queue_vslsmash(project: Project, split_locations: List[int]) -> List[Chunk]:


    script = "from vapoursynth import core\n" \
             "core.lsmas.LWLibavSource(\"{}\", cachefile=\"{}\").set_output()"
    return create_video_queue_vs(project, split_locations, script)


def create_video_queue_vs(project: Project, split_locations: List[int], script: str) -> List[Chunk]:
    """
    Create a list of chunks using vspipe and ffms2 for frame accurate seeking

    :param project: the Project
    :param split_locations: a list of frames to split on
    :param script: source filter script to use with vspipe (ignored with vs input)
    :return: A list of chunks
    """
    # add first frame and last frame
    last_frame = project.get_frames()
    split_locs_fl = [0] + split_locations + [last_frame]

    # pair up adjacent members of this list ex: [0, 10, 20, 30] -> [(0, 10), (10, 20), (20, 30)]
    chunk_boundaries = zip(split_locs_fl, split_locs_fl[1:])

    source_file = project.input.absolute().as_posix()
    vs_script = project.input

    if not project.is_vs:
        # create a vapoursynth script that will load the source with ffms2
        load_script = project.temp / 'split' / 'loadscript.vpy'
        if project.chunk_method == 'vs_ffms2':
            cache_file = (project.temp / 'split' / 'cache.ffindex').resolve().as_posix()
        else:
            cache_file = (project.temp / 'split' / 'cache.lwi').resolve().as_posix()
        with open(load_script, 'w+') as file:
            file.write(script.format(source_file, cache_file))

        cache_generation = f'vspipe -i {load_script.as_posix()} -i -'
        d = Popen(split(cache_generation), stdout=DEVNULL, stderr=DEVNULL).wait()
        vs_script = load_script

    chunk_queue = [create_vs_chunk(project, index, vs_script, *cb) for index, cb in enumerate(chunk_boundaries)]

    return chunk_queue


def create_vs_chunk(project: Project, index: int, vs_script: Path, frame_start: int, frame_end: int) -> Chunk:
    """
    Creates a chunk using vspipe

    :param project: the Project
    :param load_script: the path to the .vpy script for vspipe
    :param index: the index of the chunk
    :param frame_start: frame that this chunk should start on (0-based, inclusive)
    :param frame_end: frame that this chunk should end on (0-based, exclusive)
    :return: a Chunk
    """
    assert frame_end > frame_start, "Can't make a chunk with <= 0 frames!"

    frames = frame_end - frame_start
    frame_end -= 1  # the frame end boundary is actually a frame that should be included in the next chunk

    vspipe_gen_cmd = ['vspipe', vs_script.resolve().as_posix(), '-y', '-', '-s', str(frame_start), '-e', str(frame_end)]
    extension = ENCODERS[project.encoder].output_extension
    size = frames  # use the number of frames to prioritize which chunks encode first, since we don't have file size

    chunk = Chunk(project.temp, index, vspipe_gen_cmd, extension, size, frames)

    return chunk


def create_video_queue_select(project: Project, split_locations: List[int]) -> List[Chunk]:
    """
    Create a list of chunks using the select filter

    :param project: the Project
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """
    # add first frame and last frame
    last_frame =  project.get_frames()
    split_locs_fl = [0] + split_locations + [last_frame]

    # pair up adjacent members of this list ex: [0, 10, 20, 30] -> [(0, 10), (10, 20), (20, 30)]
    chunk_boundaries = zip(split_locs_fl, split_locs_fl[1:])

    chunk_queue = [create_select_chunk(project, index, project.input, *cb) for index, cb in enumerate(chunk_boundaries)]

    return chunk_queue


def create_select_chunk(project: Project, index: int, src_path: Path, frame_start: int, frame_end: int) -> Chunk:
    """
    Creates a chunk using ffmpeg's select filter

    :param project: the Project
    :param src_path: the path of the entire unchunked source file
    :param index: the index of the chunk
    :param frame_start: frame that this chunk should start on (0-based, inclusive)
    :param frame_end: frame that this chunk should end on (0-based, exclusive)
    :return: a Chunk
    """
    assert frame_end > frame_start, "Can't make a chunk with <= 0 frames!"

    frames = frame_end - frame_start
    frame_end -= 1  # the frame end boundary is actually a frame that should be included in the next chunk

    ffmpeg_gen_cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', src_path.as_posix(), '-vf',
                      f'select=between(n\\,{frame_start}\\,{frame_end}),setpts=PTS-STARTPTS', *project.pix_format,
                      '-color_range', '0', '-f', 'yuv4mpegpipe', '-']
    extension = ENCODERS[project.encoder].output_extension
    size = frames  # use the number of frames to prioritize which chunks encode first, since we don't have file size

    chunk = Chunk(project.temp, index, ffmpeg_gen_cmd, extension, size, frames)

    return chunk


def create_video_queue_segment(project: Project, split_locations: List[int]) -> List[Chunk]:
    """
    Create a list of chunks using segmented files

    :param project: Project
    :param split_locations: a list of frames to split on
    :return: A list of chunks
    """

    # segment into separate files
    segment(project.input, project.temp, split_locations)

    # get the names of all the split files
    source_path = project.temp / 'split'
    queue_files = [x for x in source_path.iterdir() if x.suffix == '.mkv']
    queue_files.sort(key=lambda p: p.stem)

    if len(queue_files) == 0:
        er = 'Error: No files found in temp/split, probably splitting not working'
        print(er)
        log(er)
        terminate()

    chunk_queue = [create_chunk_from_segment(project, index, file) for index, file in enumerate(queue_files)]

    return chunk_queue


def create_chunk_from_segment(project: Project, index: int, file: Path) -> Chunk:
    """
    Creates a Chunk object from a segment file generated by ffmpeg

    :param project: the Project
    :param index: the index of the chunk
    :param file: the segmented file
    :return: A Chunk
    """
    ffmpeg_gen_cmd = ['ffmpeg', '-y', '-hide_banner', '-loglevel', 'error', '-i', file.as_posix(), *project.pix_format,
                      '-color_range', '0','-f', 'yuv4mpegpipe', '-']
    file_size = file.stat().st_size
    frames =  project.get_frames()
    extension = ENCODERS[project.encoder].output_extension

    chunk = Chunk(project.temp, index, ffmpeg_gen_cmd, extension, file_size, frames)

    return chunk
