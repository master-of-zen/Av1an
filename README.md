TEMP STUFF

Av1an is written in Rust and can be used on Linux, macOS and Windows. It is highly configurable but tries to set good default values to make it easier to use.

Binary releases for Windows are also available from this repository's [releases page](https://github.com/master-of-zen/Av1an/releases).

---

# Avian

[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh)
[![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml)
[![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an)

Av1an is a video encoding framework for modern encoders. It can increase your encoding efficiency and speed by automatically splitting the input file into smaller segments and encoding these segments in parallel. This improves CPU usage when you have a lot of CPU cores and increases the speed of some AV1 encoders dramatically.

---

## Table of Contents

- [Features](#features)
- [How it works](#how-it-works)
- [Installation](#installation)
- [Supported encoders](#supported-encoders)
- [Usage](#usage)
- [Building](#building-av1an)

---

## Features

- Vastly improved encoding speed for some encoders
- Cancel and resume encoding without loss of progress
- [Vapoursynth](http://www.vapoursynth.com) script input support
- "Target Quality" mode, using VMAF to automatically set encoder options to achieve the wanted visual quality
- Automatic detection of worker number based on available hardware
- Simple and clean console look
- Convenient Docker images available
- Cross-platform, works on Linux, macOS and Windows

## How it works

Av1an uses a process called scene detection to split the input into smaller segments. It then encodes these segments (also called chunks) separately, starting multiple instances of the chosen encoder to better utilize the CPU and RAM than a single encoder would. Some of the AV1 encoders in particular are not very good at multithreading and will see the biggest speed improvement when using Av1an.

Because every segment can be encoded separately, cancelling the encoding process does not lose all progress. All 

After all segments have been encoded, they are concatenated into a single video. After all other processing steps like audio encoding are done, everything is combined into the resulting file. 

## Installation

The simplest way to install av1an is to use a package manager. There are also pre-built [Docker images](#usage-in-docker) which include all dependencies and are frequently updated.

### Package managers

Arch Linux & Manjaro: `pacman -S av1an`

If your distribution's package manager does not have Av1an or if you're on Windows, you can still install it manually.

### Manual installation

Prerequisites:

- [FFmpeg](https://ffmpeg.org/download.html)
- [Vapoursynth](https://github.com/vapoursynth/vapoursynth/releases)
- At least one [encoder](#supported-encoders)

Optional:

- [ffms2](https://github.com/FFMS/ffms2) for better chunking
- [L-SMASH](https://github.com/VFR-maniac/L-SMASH-Works) for better chunking
- [mkvmerge](https://mkvtoolnix.download/) to use mkvmerge for file concatenation (FFmpeg by default)
- [VMAF](https://github.com/Netflix/vmaf) to calculate VMAF and to use [target quality mode](docs/TargetQuality.md)

## Supported encoders

At least one encoder is required to use Av1an. Install any of these that you wish to use.

- [aomenc](https://aomedia.googlesource.com/aom/) (AV1)
- [SVT-AV1](https://gitlab.com/AOMediaCodec/SVT-AV1) (AV1)
- [rav1e](https://github.com/xiph/rav1e) (AV1)
- [libvpx](https://chromium.googlesource.com/webm/libvpx/) (VP8 and VP9)
- [x264](https://www.videolan.org/developers/x264.html) (H.264/AVC)
- [x265](https://www.videolan.org/developers/x265.html) (H.265/HEVC)

## Usage

Encode a video file with the default parameters:

```sh
av1an -i input.mkv
```

Or use a Vapoursynth script and custom parameters:

```sh
av1an -i input.vpy -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10 --target-quality 95 -a "-c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv
```

To check all available options for your version of Av1an use `av1an -h`.

## Building Av1an

To compile Av1an from source, [NASM](https://www.nasm.us/), [clang/LLVM](https://llvm.org/), [FFmpeg](https://ffmpeg.org/), [VapourSynth](https://www.vapoursynth.com/), and [Rust](https://www.rust-lang.org/) are required. Only FFmpeg and VapourSynth are required to run Av1an, the rest of the dependencies are required only for compilation.

Rust 1.59.0 or newer is currently required to build Av1an.

#### Compilation on Linux

- Install these dependencies from your distribution's package manager.
  - On Arch Linux, these are the `rust`, `nasm`, `clang`, `ffmpeg`, and `vapoursynth` packages.

Then clone and build Av1an:

```
git clone https://github.com/master-of-zen/Av1an && cd Av1an
cargo build --release
```

The resulting binary will be the file `./target/release/av1an`.

#### Compilation on Windows

To install Rust on Windows, first install [Microsoft Visual C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/). Then, download [`rustup-init.exe`](https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe), run the program, and follow the onscreen instructions. Choose "Proceed with installation (default)" when prompted.

Next, install [Python](https://www.python.org/) 3.10 or 3.8 (preferrably for all users). This is required for VapourSynth. Then, install VapourSynth from [this installer](https://github.com/vapoursynth/vapoursynth/releases/download/R58/VapourSynth64-R58.exe).

Next, install NASM by using [this installer](https://www.nasm.us/pub/nasm/releasebuilds/2.15.05/win64/nasm-2.15.05-installer-x64.exe).

Then, download a build of FFmpeg from here: https://github.com/GyanD/codexffmpeg/releases/download/5.0.1/ffmpeg-5.0.1-full_build-shared.7z

Extract the file `ffmpeg-5.0.1-full_build-shared.7z` to a directory, then create a new environment variable called `FFMPEG_DIR` (this can be done with with the "Edit environment variables for your account" function available in the control panel), and set it to the directory that you extracted the original file to (for example, set it to `C:\Users\Username\Downloads\ffmpeg-5.0.1-full_build-shared`).

Then, clone this repository (which can either be done via the git command line tool with the command `git clone https://github.com/master-of-zen/Av1an`, or by downloading and extracting the source code from the GitHub UI, which can be done with the "Download ZIP" button in the dropdown of the "Code" button near the top of the page).

With a command prompt, `cd` into the directory containing this repository's source code, and run the command `cargo build --release`. If this command executes successfully with no errors, the binary (`av1an.exe`) will be the file `./target/release/av1an.exe` (relative to the directory containing the source code).

To use the binary, copy all the `dll` files from `ffmpeg-5.0.1-full_build-shared\bin` to the same directory as `av1an.exe`, and ensure that `ffmpeg.exe` is in a folder accessible via the `PATH` environment variable.

## Av1an in Docker

The [docker image](https://hub.docker.com/r/masterofzen/av1an) is frequently updated and includes all supported encoders and all optional components. It is based on Arch Linux and provides recent versions of encoders and libraries.

The image provides three types of tags that you can use:
- `masterofzen/av1an:master` for the latest commit from `master`
- `masterofzen/av1an:sha-#######` for a specific git commit (short hash)
- (outdated) `masterofzen/av1an:latest` for the latest stable release (old python version)

### Examples

The following examples assume the file you want to encode is in your current working directory.

Linux

```bash
docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

Windows

```powershell
docker run --privileged -v "${PWD}:/videos" -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

The image can also be manually built by running 

```bash
docker build -t "av1an" .
```

in the root directory of this repository. The dependencies will automatically be installed into the image, no manual installations necessary.

To specify a different directory to use you would replace $(pwd) with the directory

```bash
docker run --privileged -v "/c/Users/masterofzen/Videos":/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

The --user flag is required on linux to avoid permission issues with the docker container not being able to write to the location, if you get permission issues ensure your user has access to the folder that you are using to encode.

### Support the developer

Bitcoin - 1GTRkvV4KdSaRyFDYTpZckPKQCoWbWkJV1

![av1an fully utilizing a 96-core CPU for video encoding](https://cdn.discordapp.com/attachments/804148977347330048/928879953825640458/av1an_preview.jpg)
