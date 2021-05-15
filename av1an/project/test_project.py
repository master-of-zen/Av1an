from av1an.av1an import get_ffmpeg_info, determine_workers, vspipe_get_num_frames
from ..arg_parse import Args


def test_ffmpeg_exists():
    assert get_ffmpeg_info().startswith("ffmpeg")


def test_rust_get_workers():
    rav1e = Args().get_project_with_args(
        ["-i", "example.mkv", "-enc", "rav1e", "-o", "out.mkv"]
    )
    rav1e.determine_workers()
    assert determine_workers("rav1e") > 0


def test_rust_vspipe():
    assert vspipe_get_num_frames("/home/redzic/CodecTest/x.vpy") == 1
