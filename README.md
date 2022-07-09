# Av1an

[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh)
[![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml)
[![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an)

Av1an is a video encoding framework for modern encoders. It can increase your encoding efficiency by running multiple encoder processes in parallel. This can improve CPU usage and encoding speed. The speed increase can be very significant if the selected encoder does not multi-thread well on its own.

Av1an can also calculate a [VMAF](https://github.com/Netflix/vmaf) score for you to assess the encode quality, and it can even target a specific VMAF score when encoding.

## Features

- Vastly improved encoding speed for some encoders (especially `aomenc`, `rav1e` and `vpxenc`)
- Cancel and resume encoding without loss of progress
- [VapourSynth](http://www.vapoursynth.com) script support
- [Target Quality](/docs/TargetQuality.md) mode, using VMAF to automatically set encoder options to achieve the desired video quality
- Simple and clean console interface
- Convenient Docker images available
- Cross-platform application written in Rust

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

The simplest way to install av1an is to use a package manager. There are also pre-built [Docker images](/docs/docker.md) which include all dependencies and are frequently updated.

For Windows users that do not want to use Docker, prebuilt binaries are also included in every [release](https://github.com/master-of-zen/Av1an/releases), and a [nightly build](https://github.com/master-of-zen/Av1an/releases/tag/latest) of the current `master` branch is also available.

### Package managers

Arch Linux & Manjaro: `pacman -S av1an`

If your distribution's package manager does not have Av1an or if you're on Windows, you can still install Av1an manually.

### Manual installation

Prerequisites:

- [FFmpeg](https://ffmpeg.org/download.html)
- [VapourSynth](https://github.com/vapoursynth/vapoursynth/releases)
- At least one [encoder](#supported-encoders)

Optional:

- [L-SMASH](https://github.com/AkarinVS/L-SMASH-Works) VapourSynth plugin for better chunking (recommended)
- [ffms2](https://github.com/FFMS/ffms2) VapourSynth plugin for better chunking
- [mkvmerge](https://mkvtoolnix.download/) to use mkvmerge instead of FFmpeg for file concatenation
- [VMAF](https://github.com/Netflix/vmaf) to calculate VMAF scores and to use [target quality mode](docs/TargetQuality.md)

### VapourSynth plugins on Windows

If you want to install the L-SMASH or ffms2 plugins and are on Windows, then you have [two installation options](http://vapoursynth.com/doc/installation.html#plugins-and-scripts). The easiest way is using the included plugin script:

1. Open your VapourSynth installation directory
2. Open a command prompt or PowerShell window via Shift + Right click
3. Run `python3 vsrepo.py install lsmas ffms2`

## Usage

Encode a video file with the default parameters:

```sh
av1an -i input.mkv
```

Or use a VapourSynth script and custom parameters:

```sh
av1an -i input.vpy -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10 --target-quality 95 -a "-c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv
```

To check all available options for your version of Av1an use `av1an --help`.

## Support the developer

Bitcoin - 1GTRkvV4KdSaRyFDYTpZckPKQCoWbWkJV1

![av1an fully utilizing a 96-core CPU for video encoding](https://cdn.discordapp.com/attachments/804148977347330048/928879953825640458/av1an_preview.jpg)
