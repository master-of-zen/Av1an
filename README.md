# Av1an

[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh)
[![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml)
[![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an)

Av1an is a video encoding framework for modern encoders. It can increase your encoding efficiency by running multiple encoder processes in parallel. This can improve CPU usage and increases the speed of some AV1 encoders dramatically.

Av1an can also calculate a VMAF score for you to assess the encode quality, and it can even target a specific VMAF score when encoding.

## Features

- Vastly improved encoding speed for some encoders
- Cancel and resume encoding without loss of progress
- [VapourSynth](http://www.vapoursynth.com) script support
- [Target Quality](/docs/TargetQuality.md) mode, using [VMAF](https://github.com/Netflix/vmaf) to automatically set encoder options to achieve the wanted visual quality
- Simple and clean console interface
- Convenient Docker images available
- Cross-platform application written in Rust

## Supported encoders

At least one encoder is required to use Av1an. Install any of these that you wish to use.

- [aomenc](https://aomedia.googlesource.com/aom/) (AV1)
- [SVT-AV1](https://gitlab.com/AOMediaCodec/SVT-AV1) (AV1)
- [rav1e](https://github.com/xiph/rav1e) (AV1)
- [libvpx](https://chromium.googlesource.com/webm/libvpx/) (VP8 and VP9)
- [x264](https://www.videolan.org/developers/x264.html) (H.264/AVC)
- [x265](https://www.videolan.org/developers/x265.html) (H.265/HEVC)

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

- [L-SMASH](https://github.com/VFR-maniac/L-SMASH-Works) for an alternative chunking method (recommended)
- [ffms2](https://github.com/FFMS/ffms2) for an alternative chunking method
- [mkvmerge](https://mkvtoolnix.download/) to use mkvmerge instead of FFmpeg for file concatenation
- [VMAF](https://github.com/Netflix/vmaf) to calculate VMAF scores and to use [target quality mode](docs/TargetQuality.md)

## Usage

Encode a video file with the default parameters:

```sh
av1an -i input.mkv
```

Or use a VapourSynth script and custom parameters:

```sh
av1an -i input.vpy -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10 --target-quality 95 -a "-c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv
```

To check all available options for your version of Av1an use `av1an -h`.

## Support the developer

Bitcoin - 1GTRkvV4KdSaRyFDYTpZckPKQCoWbWkJV1

![av1an fully utilizing a 96-core CPU for video encoding](https://cdn.discordapp.com/attachments/804148977347330048/928879953825640458/av1an_preview.jpg)
