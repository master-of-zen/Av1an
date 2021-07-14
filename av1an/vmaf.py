import shlex
import subprocess
from pathlib import Path
from subprocess import PIPE, STDOUT

from av1an_pyo3 import validate_vmaf, Chunk

from av1an.manager.Pipes import process_pipe


class VMAF:
    def __init__(self, n_threads=0, model=None, res=None, vmaf_filter=None):
        self.n_threads = f":n_threads={n_threads}" if n_threads else ""
        self.model = f":model_path={model}" if model else ""
        self.res = res if res else "1920x1080"
        self.vmaf_filter = vmaf_filter + "," if vmaf_filter else ""
        validate_vmaf(self.model)

    def call_vmaf(
        self, chunk: Chunk, encoded: Path, vmaf_rate: int = None, fl_path: Path = None
    ):
        cmd = ""

        if fl_path is None:
            fl_path = (Path(chunk.temp) / "split") / f"{chunk.name}.json"
        fl = fl_path.as_posix()

        cmd_in = (
            "ffmpeg",
            "-loglevel",
            "error",
            "-y",
            "-thread_queue_size",
            "1024",
            "-hide_banner",
            "-r",
            "60",
            "-i",
            encoded.as_posix(),
            "-r",
            "60",
            "-i",
            "-",
        )

        filter_complex = ("-filter_complex",)

        # Change framerate of comparison to framerate of probe
        select = (
            f"select=not(mod(n\\,{vmaf_rate})),setpts={1 / vmaf_rate}*PTS,"
            if vmaf_rate
            else ""
        )

        distorted = f"[0:v]scale={self.res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[distorted];"

        ref = fr"[1:v]{select}{self.vmaf_filter}scale={self.res}:flags=bicubic:force_original_aspect_ratio=decrease,setpts=PTS-STARTPTS[ref];"

        vmaf_filter = f"[distorted][ref]libvmaf=log_fmt='json':eof_action=endall:log_path={shlex.quote(fl)}{self.model}{self.n_threads}"

        cmd_out = ("-f", "null", "-")

        cmd = (*cmd_in, *filter_complex, distorted + ref + vmaf_filter, *cmd_out)

        ffmpeg_gen_pipe = subprocess.Popen(
            chunk.ffmpeg_gen_cmd, stdout=PIPE, stderr=STDOUT
        )

        pipe = subprocess.Popen(
            cmd,
            stdin=ffmpeg_gen_pipe.stdout,
            stdout=PIPE,
            stderr=STDOUT,
            universal_newlines=True,
        )
        utility = (ffmpeg_gen_pipe,)
        process_pipe(pipe, chunk.index, utility)

        return fl_path
