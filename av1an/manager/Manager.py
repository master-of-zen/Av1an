import json
import shutil
import sys
from pathlib import Path

from av1an.chunk.chunk_queue import load_or_gen_chunk_queue
from av1an.concat import concatenate_mkvmerge
from av1an.split import split_routine
from av1an_pyo3 import (
    concatenate_ffmpeg,
    concatenate_ivf,
    extract_audio,
    log,
    plot_vmaf,
    set_log,
    determine_workers,
    hash_path,
    init_progress_bar,
    update_bar,
    finish_progress_bar,
    Project,
)

from .Queue import Queue


def encode_file(project: Project):
    hash = str(hash_path(str(project.input)))

    if project.temp:
        if project.temp[-1] in ("\\", "/"):
            project.temp = Path(f"{project.temp}{'.' + hash}")
        else:
            project.temp = Path(str(project.temp)).as_posix()
    else:
        project.temp = Path("." + hash)

    log(f"File hash: {hash}")
    # Checking is resume possible
    done_path = Path(project.temp) / "done.json"
    project.resume = project.resume and done_path.exists()

    if not project.resume and Path(project.temp).is_dir():
        shutil.rmtree(project.temp)

    (Path(project.temp) / "split").mkdir(parents=True, exist_ok=True)
    (Path(project.temp) / "encode").mkdir(exist_ok=True)
    if project.logging:
        set_log(Path(project.logging).with_suffix(".log").as_posix())
    else:
        set_log((Path(project.temp) / "log.log").as_posix())

    # find split locations
    split_locations = split_routine(project, project.resume)

    # create a chunk queue
    chunk_queue = load_or_gen_chunk_queue(project, project.resume, split_locations)

    done_path = Path(project.temp) / "done.json"
    if project.resume and done_path.exists():
        log("Resuming...")
        with open(done_path) as done_file:
            data = json.load(done_file)

        project.set_frames(data["frames"])
        done = len(data["done"])
        initial_frames = sum(data["done"].values())
        log(f"Resumed with {done} encoded clips done")
    else:
        initial_frames = 0
        total = project.get_frames()
        d = {"frames": total, "done": {}}
        with open(done_path, "w") as done_file:
            json.dump(d, done_file)

    if not project.resume:
        extract_audio(
            str(Path(project.input).resolve()),
            str(Path(project.temp).resolve()),
            project.audio_params,
        )

    # do encoding loop
    project.workers = (
        project.workers if project.workers else determine_workers(project.encoder)
    )
    project.workers = min(project.workers, len(chunk_queue))
    print(
        f"\rQueue: {len(chunk_queue)} Workers: {project.workers} Passes: {project.passes}\n"
        f'Params: {" ".join(project.video_params)}\n'
    )
    init_progress_bar(project.get_frames() - initial_frames)
    project.counter_update = update_bar
    project.finish_progress_bar = finish_progress_bar
    queue = Queue(project, chunk_queue)
    queue.encoding_loop()

    if queue.status.lower() == "fatal":
        msg = "FATAL Encoding process encountered fatal error, shutting down"
        print("\n::", msg)
        log(msg)
        sys.exit(1)

    # concat
    log("Concatenating")
    if project.output_ivf:
        concatenate_ivf(
            str((Path(project.temp) / "encode").resolve()),
            str(Path(project.output_file).with_suffix(".ivf").resolve()),
        )
    elif project.mkvmerge:
        concatenate_mkvmerge(project.temp, project.output_file)
    else:
        concatenate_ffmpeg(
            str(Path(project.temp).resolve()),
            str(Path(project.output_file).resolve()),
            project.encoder,
        )

    if project.vmaf:
        plot_vmaf(Path(project.input).as_posix(), Path(project.output_file).as_posix())

    # Delete temp folders
    if not project.keep:
        shutil.rmtree(project.temp)
