from av1an.project.Project import Project
from av1an.av1an import get_ffmpeg_info, determine_workers
from ..arg_parse import Args


def test_ffmpeg_exists():
    assert get_ffmpeg_info().startswith("ffmpeg")


# def test_rust_integration():
#     assert determine_workers() == 0


def test_rust_get_workers():
    rav1e = Args().get_project_with_args(
        ["-i", "example.mkv", "-enc", "rav1e", "-o", "out.mkv"]
    )
    rav1e.determine_workers()
    assert determine_workers("rav1e") == rav1e.workers

    svt_av1 = Args().get_project_with_args(
        ["-i", "example.mkv", "-enc", "svt_av1", "-o", "out.mkv"]
    )
    svt_av1.determine_workers()
    assert determine_workers("svt_av1") == svt_av1.workers
