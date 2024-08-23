# Av1an

<img align="right" width=40% src="https://github.com/master-of-zen/Av1an/assets/46526140/15f68b63-7be5-45e8-bf48-ae7eb2fc4bb6" alt="av1an fully utilizing a 96-core CPU for video encoding">


[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh) [![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml) [![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an) [![](https://tokei.rs/b1/github/master-of-zen/Av1an?category=code)](https://github.com/master-of-zen/Av1an)

Av1an is a video encoding framework which aims to increase your encoding speed and improve CPU-utilization by running multiple encoder processes in parallel. Additional features include Scene-detection, VMAF-score plotting & quality-targeting.


<a href="https://www.buymeacoffee.com/master_of_zen" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee" style="height: 60px !important;width: 217px !important;" ></a>


> [!TIP]
> Check out the [**Av1an Book**](https://master-of-zen.github.io/Av1an/) for more details on usage.

For help with av1an, feel free to reach out to us on [Discord](https://discord.gg/Ar8MvJh) or file an [issue on GitHub](https://github.com/master-of-zen/Av1an/issues).


## Features

- Hyper-scalable video encoding
- [Target Quality mode](https://master-of-zen.github.io/Av1an/Features/target_quality.html), using VMAF control encoders rate control to achieve the desired video quality
- [VapourSynth](http://www.vapoursynth.com) script support
- Cancel and resume encoding without loss of progress (see `--keep` & `--resume`)
- Minimal and clean CLI
- Docker images available
- Cross-platform application written in Rust

## Usage

Av1an offers a comprehensive command-line interface for streamlined operation. For complete reference of it's offerings, refer to the [CLI Reference](https://master-of-zen.github.io/Av1an/Cli/general.html) in the docs or run `av1an --help`.

### Examples

Encode a video file with the default parameters (uses `aom` for encoding):

```bash
$ av1an -i input.mkv
```

Encode a video file and all it's accompanying audio-tracks to 2-channel 192kbps opus:
```bash
$ av1an -i input.mkv -a "-c:a libopus -ac 2 -b:a 192k"
```

Encode a video file with cinematic cropping using ffmpeg parameters:
```bash
$ av1an -i input.mkv -f "-vf crop=1920:800"
```

Encode from a VapourSynth script with custom video encoder settings, 10 workers, [target quality](https://master-of-zen.github.io/Av1an/Features/target_quality.html), audio encoding options, logging and custom output file-name:

```bash
$ av1an -i input.vpy -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10 --target-quality 95 -a "-c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv
```

## Supported Video encoders
Av1an is able to use a variety of external video-encoders to encode your chosen video. At least one of these encoders must be installed in order to use Av1an. The following are supported:
| Codec | Encoder | `-e` option | Default encoder Parameters (`-v`) | Source |
| -:| - | - | - |:-:|
| AV1 | aomenc | `aom` | `--threads=8 --cpu-used=6 --end-usage=q --cq-level=30 --tile-columns=1 --tile-rows=0` |[Google Source](https://aomedia.googlesource.com/aom/) |
| AV1 | rav1e | `rav1e` | `--speed 6 --quantizer 100 --no-scene-detection --tiles 2` | [GitHub](https://github.com/xiph/rav1e) |
| AV1 | SvtAv1EncApp | `svt-av1` | `--preset 4 --keyint 240 --rc 0 --crf 25 --tile-columns 1 --tile-rows 0` | [GitLab](https://gitlab.com/AOMediaCodec/SVT-AV1) |
| VP8 & VP9 | vpxenc | `vpx` | `--codec=vp9 -b 10 --profile=2 --threads=4 --cpu-used=2 --end-usage=q --cq-level=30 --row-mt=1 --auto-alt-ref=6 --tile-columns=1 --tile-rows=0` | [Google Source](https://chromium.googlesource.com/webm/libvpx/) |
| H.264/AVC | x264 | `x264` | `--preset slow --crf 25` | [VideoLAN](https://www.videolan.org/developers/x264.html) |
| H.256/HEVC | x265 | `x265` | `-p slow --crf 25 -D 10 --level-idc 5.0` | [VideoLAN](https://www.videolan.org/developers/x265.html) |

> [!IMPORTANT]
> 
> Note that Av1an depends on external encoder executables as opposed to libraries.
>
> If you use a package manager to install your encoders, check that the packages you've installed include the encoder executables (e.g. `vpxenc`, `SvtAv1EncApp`) from the list above, and not simply their respective libraries (e.g. `libvpx`, `libSvtAv1Enc`).

## Supported Audio encoders
Av1an relies entirely on your system's `ffmpeg` binary for audio encoding.

## Installation

av1an can be installed from package managers, cargo.io, or compliled manually. There are also pre-built [Docker images](/site/src/DOCKER.md) available that include all dependencies and are frequently updated.

For Windows users, prebuilt binaries are also included in every [release](https://github.com/master-of-zen/Av1an/releases), and a [nightly build](https://github.com/master-of-zen/Av1an/releases/tag/latest) of the current `master` branch is also available.

### Package managers

Av1an is available officially for download from the [Arch Linux](https://archlinux.org/packages/extra/x86_64/av1an/) & [Manjaro](https://packages.manjaro.org/?query=av1an) package repositories
```bash
$ pacman -S av1an
```

In addition, Av1an is also available as a Rust Package, and can therefore easily be compiled from source and installed using Cargo:
```bash
cargo install av1an
```

See [Compiling](https://master-of-zen.github.io/Av1an/compiling.html) for details on building Av1an from source.

### Dependencies

Requirements:

- [FFmpeg](https://ffmpeg.org/download.html)
- [VapourSynth](https://github.com/vapoursynth/vapoursynth/releases)
- At least one [supported encoder](#supported-encoders)

Optional:

- [L-SMASH](https://github.com/AkarinVS/L-SMASH-Works) VapourSynth plugin for better chunking (recommended)
- [DGDecNV](https://www.rationalqm.us/dgdecnv/dgdecnv.html) Vapoursynth plugin for very fast and accurate chunking, `dgindexnv` executable needs to be present in system path and an Nvidia GPU with CUVID
- [ffms2](https://github.com/FFMS/ffms2) VapourSynth plugin for better chunking
- [bestsource](https://github.com/vapoursynth/bestsource) Vapoursynth plugin for slow but accurate chunking
- [mkvmerge](https://mkvtoolnix.download/) to use mkvmerge instead of FFmpeg for file concatenation
- [VMAF](https://github.com/Netflix/vmaf) to calculate VMAF scores and to use [target quality mode](docs/target_quality.md)

#### VapourSynth plugins on Windows

If you want to install the L-SMASH or ffms2 plugins and are on Windows, then you have [two installation options](http://vapoursynth.com/doc/installation.html#plugins-and-scripts). The easiest way is using the included plugin script:

1. Open your VapourSynth installation directory
2. Open a command prompt or PowerShell window via Shift + Right click
3. Run `python3 vsrepo.py install lsmas ffms2`
