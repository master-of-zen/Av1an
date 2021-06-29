import json
from pathlib import Path
from typing import List

from av1an.project import Project
from av1an.chunk import Chunk
from av1an_pyo3 import create_vs_file, get_keyframes, segment, output_extension, log
import sys


def save_chunk_queue(temp: Path, chunk_queue: List[Chunk]) -> None:
    chunk_dicts = [c.to_dict() for c in chunk_queue]
    with open(temp / "chunks.json", "w") as file:
        json.dump(chunk_dicts, file)


def read_chunk_queue(temp: Path) -> List[Chunk]:
    with open(temp / "chunks.json", "r") as file:
        chunk_dicts = json.load(file)
    return [Chunk.create_from_dict(cd, temp) for cd in chunk_dicts]


def load_or_gen_chunk_queue(
    project: Project, resuming: bool, split_locations: List[int]
) -> List[Chunk]:
    # if resuming, read chunks from file and remove those already done
    if resuming:
        chunk_queue = read_chunk_queue(project.temp)

        done_path = project.temp / "done.json"
        with open(done_path) as done_file:
            data = json.load(done_file)
        done_chunk_names = data["done"].keys()
        chunk_queue = [c for c in chunk_queue if c.name not in done_chunk_names]
        return chunk_queue

    # create and save
    chunk_queue = create_encoding_queue(project, split_locations)
    save_chunk_queue(project.temp, chunk_queue)

    return chunk_queue


def create_encoding_queue(project: Project, split_locations: List[int]) -> List[Chunk]:
    chunk_method_gen = {
        "segment": create_video_queue_segment,
        "select": create_video_queue_select,
        "vs_ffms2": create_video_queue_vsffms2,
        "vs_lsmash": create_video_queue_vslsmash,
        "hybrid": create_video_queue_hybrid,
    }
    chunk_queue = chunk_method_gen[project.chunk_method](project, split_locations)

    # Sort largest first so chunks that take a long time to encode start first
    chunk_queue.sort(key=lambda c: c.size, reverse=True)
    return chunk_queue


def create_video_queue_hybrid(
    project: Project, split_locations: List[int]
) -> List[Chunk]:
    keyframes = get_keyframes(str(project.input.resolve()))

    end = [project.get_frames()]

    splits = [0] + split_locations + end

    segments_list = list(zip(splits, splits[1:]))
    to_split = [x for x in keyframes if x in splits]
    segments = []

    # Make segments
    log("Segmenting Video")
    segment(str(project.input.resolve()), str(project.temp.resolve()), to_split[1:])
    log("Segment Done")
    source_path = project.temp / "split"
    queue_files = [x for x in source_path.iterdir() if x.suffix == ".mkv"]
    queue_files.sort(key=lambda p: p.stem)

    kf_list = list(zip(to_split, to_split[1:] + end))
    for f, (x, y) in zip(queue_files, kf_list):
        to_add = [
            (f, [s[0] - x, s[1] - x])
            for s in segments_list
            if s[0] >= x and s[1] <= y and s[0] - x < s[1] - x
        ]
        segments.extend(to_add)

    chunk_queue = [
        create_select_chunk(project, index, file, *cb)
        for index, (file, cb) in enumerate(segments)
    ]
    return chunk_queue


def create_video_queue_vsffms2(
    project: Project, split_locations: List[int]
) -> List[Chunk]:
    return create_video_queue_vs(project, split_locations)


def create_video_queue_vslsmash(
    project: Project, split_locations: List[int]
) -> List[Chunk]:
    return create_video_queue_vs(project, split_locations)


def create_video_queue_vs(project: Project, split_locations: List[int]) -> List[Chunk]:
    last_frame = project.get_frames()
    split_locs_fl = [0] + split_locations + [last_frame]

    # pair up adjacent members of this list ex: [0, 10, 20, 30] -> [(0, 10), (10, 20), (20, 30)]
    chunk_boundaries = zip(split_locs_fl, split_locs_fl[1:])

    source_file = project.input.absolute().as_posix()
    if project.is_vs:
        vs_script = project.input
    else:
        vs_script = create_vs_file(
            project.temp.as_posix(), source_file, project.chunk_method
        )

    chunk_queue = [
        create_vs_chunk(project, index, vs_script, *cb)
        for index, cb in enumerate(chunk_boundaries)
    ]

    return chunk_queue


def create_vs_chunk(
    project: Project, index: int, vs_script: Path, frame_start: int, frame_end: int
) -> Chunk:
    assert frame_end > frame_start, "Can't make a chunk with <= 0 frames!"

    frames = frame_end - frame_start
    frame_end -= 1  # the frame end boundary is actually a frame that should be included in the next chunk

    vspipe_gen_cmd = [
        "vspipe",
        vs_script,
        "-y",
        "-",
        "-s",
        str(frame_start),
        "-e",
        str(frame_end),
    ]
    extension = output_extension(project.encoder)
    size = frames  # use the number of frames to prioritize which chunks encode first, since we don't have file size

    chunk = Chunk(project.temp, index, vspipe_gen_cmd, extension, size, frames)

    return chunk


def create_video_queue_select(
    project: Project, split_locations: List[int]
) -> List[Chunk]:
    # add first frame and last frame
    last_frame = project.get_frames()
    split_locs_fl = [0] + split_locations + [last_frame]

    # pair up adjacent members of this list ex: [0, 10, 20, 30] -> [(0, 10), (10, 20), (20, 30)]
    chunk_boundaries = zip(split_locs_fl, split_locs_fl[1:])

    chunk_queue = [
        create_select_chunk(project, index, project.input, *cb)
        for index, cb in enumerate(chunk_boundaries)
    ]

    return chunk_queue


def create_select_chunk(
    project: Project, index: int, src_path: Path, frame_start: int, frame_end: int
) -> Chunk:
    assert frame_end > frame_start, "Can't make a chunk with <= 0 frames!"

    frames = frame_end - frame_start
    frame_end -= 1  # the frame end boundary is actually a frame that should be included in the next chunk

    ffmpeg_gen_cmd = [
        "ffmpeg",
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        src_path.as_posix(),
        "-vf",
        f"select=between(n\\,{frame_start}\\,{frame_end}),setpts=PTS-STARTPTS",
        *project.pix_format,
        "-f",
        "yuv4mpegpipe",
        "-",
    ]
    extension = output_extension(project.encoder)
    size = frames  # use the number of frames to prioritize which chunks encode first, since we don't have file size

    chunk = Chunk(project.temp, index, ffmpeg_gen_cmd, extension, size, frames)

    return chunk


def create_video_queue_segment(
    project: Project, split_locations: List[int]
) -> List[Chunk]:
    log("Split Video")
    segment(project.input, project.temp, split_locations)
    log("Split Done")
    # get the names of all the split files
    source_path = project.temp / "split"
    queue_files = [x for x in source_path.iterdir() if x.suffix == ".mkv"]
    queue_files.sort(key=lambda p: p.stem)

    if len(queue_files) == 0:
        er = "Error: No files found in temp/split, probably splitting not working"
        print(er)
        log(er)
        sys.exit(1)

    chunk_queue = [
        create_chunk_from_segment(project, index, file)
        for index, file in enumerate(queue_files)
    ]

    return chunk_queue


def create_chunk_from_segment(project: Project, index: int, file: Path) -> Chunk:
    ffmpeg_gen_cmd = [
        "ffmpeg",
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        file.as_posix(),
        *project.pix_format,
        "-f",
        "yuv4mpegpipe",
        "-",
    ]
    file_size = file.stat().st_size
    frames = project.get_frames()
    extension = output_extension(project.encoder)

    chunk = Chunk(project.temp, index, ffmpeg_gen_cmd, extension, file_size, frames)

    return chunk
