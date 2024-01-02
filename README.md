# Av1an

![av1an fully utilizing a 96-core CPU for video encoding](https://cdn.discordapp.com/attachments/804148977347330048/928879953825640458/av1an_preview.jpg)

[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh)
[![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml)
[![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an)
[![](https://tokei.rs/b1/github/master-of-zen/Av1an?category=code)](https://github.com/master-of-zen/Av1an)

Av1an is a video encoding framework. It can increase your encoding speed and improve cpu utilization by running multiple encoder processes in parallel. Target quality, VMAF plotting, and more, available to take advantage for video encoding.

## Features

- Hyper-scalable video encoding
- [Target Quality](/docs/TargetQuality.md) mode, using VMAF control encoders rate control to achieve the desired video quality
- [VapourSynth](http://www.vapoursynth.com) script support
- Cancel and resume encoding without loss of progress
- Minimal and clean CLI
- Docker images available
- Cross-platform application written in Rust

## Usage

For complete reference, refer to [CLI](/docs/CLI.md)
or run `av1an --help`

Encode a video file with the default parameters:

```sh
av1an -i input.mkv
```

Or use a VapourSynth script and custom parameters:

```sh
av1an -i input.vpy -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10 --target-quality 95 -a "-c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv
```

## Supported encoders

At least one encoder is required to use Av1an. The following encoders are supported:

- [aomenc](https://aomedia.googlesource.com/aom/) (AV1)
- [SvtAv1EncApp](https://gitlab.com/AOMediaCodec/SVT-AV1) (AV1)
- [rav1e](https://github.com/xiph/rav1e) (AV1)
- [vpxenc](https://chromium.googlesource.com/webm/libvpx/) (VP8 and VP9)
- [x264](https://www.videolan.org/developers/x264.html) (H.264/AVC)
- [x265](https://www.videolan.org/developers/x265.html) (H.265/HEVC)

Note that Av1an requires the executable encoder. If you use a package manager to install encoders, check that the installation includes an executable encoder (e.g. vpxenc, SvtAv1EncApp) from the list above. Just installing the library (e.g. libvpx, libSvtAv1Enc) is not enough.

## Installation

av1an can be installed from package managers, cargo.io, or compliled manually. There are also pre-built [Docker images](/docs/DOCKER.md) which include all dependencies and are frequently updated.

For Windows users, prebuilt binaries are also included in every [release](https://github.com/master-of-zen/Av1an/releases), and a [nightly build](https://github.com/master-of-zen/Av1an/releases/tag/latest) of the current `master` branch is also available.

### Package managers

Arch Linux & Manjaro: `pacman -S av1an`
Cargo: `cargo install av1an`

### Manual installation

Prerequisites:

- [FFmpeg](https://ffmpeg.org/download.html)
- [VapourSynth](https://github.com/vapoursynth/vapoursynth/releases)
- At least one [encoder](#supported-encoders)

Optional:

- [L-SMASH](https://github.com/AkarinVS/L-SMASH-Works) VapourSynth plugin for better chunking (recommended)
- [ffms2](https://github.com/FFMS/ffms2) VapourSynth plugin for better chunking
- [bestsource](https://github.com/vapoursynth/bestsource) Vapoursynth plugin for slow but accurate chunking
- [mkvmerge](https://mkvtoolnix.download/) to use mkvmerge instead of FFmpeg for file concatenation
- [VMAF](https://github.com/Netflix/vmaf) to calculate VMAF scores and to use [target quality mode](docs/TargetQuality.md)

### VapourSynth plugins on Windows

If you want to install the L-SMASH or ffms2 plugins and are on Windows, then you have [two installation options](http://vapoursynth.com/doc/installation.html#plugins-and-scripts). The easiest way is using the included plugin script:

1. Open your VapourSynth installation directory
2. Open a command prompt or PowerShell window via Shift + Right click
3. Run `python3 vsrepo.py install lsmas ffms2`

## Support the developer

Bitcoin - 1GTRkvV4KdSaRyFDYTpZckPKQCoWbWkJV1
