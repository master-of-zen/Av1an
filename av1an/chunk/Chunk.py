from pathlib import Path
from typing import Dict, Any


class Chunk:
    def __init__(
        self,
        temp: Path,
        index: int,
        ffmpeg_gen_cmd: list,
        output_ext: str,
        size: int,
        frames: int,
    ):
        self.index: int = index
        self.ffmpeg_gen_cmd: str = ffmpeg_gen_cmd
        self.size: int = size
        self.temp: Path = temp
        self.frames: int = frames
        self.output_ext: str = output_ext
        self.per_shot_target_quality_cq = None

    def to_dict(self) -> Dict[str, Any]:

        return {
            "index": self.index,
            "ffmpeg_gen_cmd": self.ffmpeg_gen_cmd,
            "size": self.size,
            "frames": self.frames,
            "output_ext": self.output_ext,
            "per_shot_target_quality_cq": self.per_shot_target_quality_cq,
        }

    @property
    def output_path(self) -> Path:
        return (self.temp / "encode") / f"{self.name}.{self.output_ext}"

    @property
    def output(self) -> str:

        return self.output_path.as_posix()

    @property
    def name(self) -> str:

        return str(self.index).zfill(5)

    @staticmethod
    def create_from_dict(d: dict, temp):
        chunk = Chunk(
            temp,
            d["index"],
            d["ffmpeg_gen_cmd"],
            d["output_ext"],
            d["size"],
            d["frames"],
        )
        chunk.per_shot_target_quality_cq = d["per_shot_target_quality_cq"]
        return chunk
