TEMP STUFF

Av1an is written in Rust and can be used on Linux, macOS and Windows. It is highly configurable but tries to set good default values to make it easier to use.

Binary releases for Windows are also available from this repository's [releases page](https://github.com/master-of-zen/Av1an/releases).

---

# Avian

[![Discord server](https://discordapp.com/api/guilds/696849974230515794/embed.png)](https://discord.gg/Ar8MvJh)
[![CI tests](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml/badge.svg)](https://github.com/master-of-zen/Av1an/actions/workflows/tests.yml)
[![](https://img.shields.io/crates/v/av1an.svg)](https://crates.io/crates/av1an)

Av1an is a video encoding framework for modern encoders. It can increase your encoding efficiency by automatically splitting the input file into smaller segments and encoding these segments in parallel. This improves CPU usage when you have a lot of CPU cores and increases the speed of some AV1 encoders dramatically.

---

## Table of Contents

- [Installation](#installation)
- [Supported encoders](#supported-encoders)
- [Usage](#usage)
- [Configuration](#configuration)
- [Building](#building-av1an)

---

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

Example with default parameters:

    av1an -i input

Or with your own parameters:

    av1an -i input -v " --cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10
    --target-quality 95 -a " -c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv

## Configuration

<h2 align="center">General Usage</h2>

    -i  --input             Input file, or Vapoursynth (.py,.vpy) script
                            (relative or absolute path)

    -o  --output-file       Name/Path for output file (Default: (input file name)_(encoder).mkv)
                            Output is `mkv` by default
                            Ouput extension can be set to: `mkv`, `webm`, `mp4`

    -e  --encoder           Encoder to use
                            [default: aom] [possible values: aom, rav1e, vpx, svt-av1, x264, x265]

    -v  --video-params      Encoder settings flags (If not set, will be used default parameters.)
                            Must be inside ' ' or " "

    -p  --passes            Set number of passes for encoding
                            (Default: AOMENC: 2, rav1e: 1, SVT-AV1: 1,
                            VPX: 2, x265: 1, x264: 1)

    -w  --workers           Override number of workers.

    -r  --resume            Resumes encoding.

    --keep                  Doesn't delete temporary folders after encode has finished.

    -q  --quiet             Do not print a progress bar to the terminal.

    -l  --logging           Path to .log file(By default created in temp folder)

    --temp                  Set path for the temporary folder. [default: .hash]

    -c  --concat            Concatenation method to use for splits Default: ffmpeg
                            [possible values: ffmpeg, mkvmerge, ivf]

<h3 align="center">FFmpeg Options</h3>

    -a  --audio-params      FFmpeg audio settings (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'

    -f  --ffmpeg            FFmpeg options video options.
                            Applied to each encoding segment individually.
                            (Warning: Cropping doesn't work with Target VMAF mode
                            without specifying it in --vmaf-filter)
                            Example:
                            --ff " -vf scale=320:240 "

    --pix-format            Setting custom pixel/bit format for piping
                            (Default: 'yuv420p10le')

<h3 align="center">Chunking Options</h3>

    --split-method          Method used for generating splits. (Default: av-scenechange)
                            Options: `av-scenechange`, `none`
                            `none` -  skips scenedetection.

    -m  --chunk-method      Determine the method in which chunks are made for encoding.
                            By default the best method is selected automatically.
                            [possible values: segment, select, ffms2, lsmash, hybrid]

    -s  --scenes            File to save/read scenes.

    -x  --extra-split       Size of chunk after which it will be split [default: fps * 10]

    --min-scene-len         Specifies the minimum number of frames in each split.

<h3 align="center">Target Quality</h3>

    --target-quality        Quality value to target.
                            VMAF used as substructure for algorithms.
                            When using this mode, you must use quantizer/quality modes of encoder.

    --target-quality-method Type of algorithm for use.
                            Options: per_shot

    --min-q, --max-q        Min,Max Q values limits
                            If not set by the user, the default for encoder range will be used.

    --vmaf                  Calculate VMAF after encoding is done and make a plot.

    --vmaf-path             Custom path to libvmaf models.
                            example: --vmaf-path "vmaf_v0.6.1.pkl"
                            Recommended to place both files in encoding folder
                            (`vmaf_v0.6.1.pkl` and `vmaf_v0.6.1.pkl.model`)
                            (Required if VMAF calculation doesn't work by default)

    --vmaf-res              Resolution for VMAF calculation.
                            [default: 1920x1080]

    --probes                Number of probes for target quality. [default: 4]

    --probe-slow            Use probided video encoding parameters for vmaf probes.

    --vmaf-filter           Filter used for VMAF calculation. The passed format is filter_complex.
                            So if crop filter used ` -ff " -vf crop=200:1000:0:0 "`
                            `--vmaf-filter` must be : ` --vmaf-filter "crop=200:1000:0:0"`

    --probing-rate          Setting rate for VMAF probes. Using every N frame used in probe.
                            [default: 4]

    --vmaf-threads          Limit number of threads that are used for VMAF calculation

<h2 align="center">Main Features</h2>

Av1an allows for **splitting input video by scenes for parallel encoding** to improve encoding performance, because most AV1 encoders are currently not very good at multithreading and encoding is limited to a very limited number of threads.

- [Vapoursynth](http://www.vapoursynth.com) script input support.
- Speed up video encoding.
- "Target Quality" mode. Targeting end result reference visual quality. VMAF used as a substructure
- Resuming encoding without loss of encoded progress.
- Simple and clean console look.
- Automatic detection of the number of workers the host can handle.
- Both video and audio transcoding.


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

## Usage in Docker

Av1an can be run in a Docker container with the following command if you are in the current directory
Linux

```bash
docker run --privileged -v "$(pwd):/videos" --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

Windows

```powershell
docker run --privileged -v "${PWD}:/videos" -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

Docker can also be built by using

```bash
docker build -t "av1an" .
```

To specify a different directory to use you would replace $(pwd) with the directory

```bash
docker run --privileged -v "/c/Users/masterofzen/Videos":/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```

The --user flag is required on linux to avoid permission issues with the docker container not being able to write to the location, if you get permission issues ensure your user has access to the folder that you are using to encode.

### Docker tags

The docker image has the following tags

|    Tag    | Description                                           |
| :-------: | ----------------------------------------------------- |
|   latest  | Contains the latest stable av1an version release      |
|   master  | Contains the latest av1an commit to the master branch |
| sha-##### | Contains the commit of the hash that is referenced    |
|    #.##   | Stable av1an version release                          |

### Support the developer

Bitcoin - 1GTRkvV4KdSaRyFDYTpZckPKQCoWbWkJV1

![av1an fully utilizing a 96-core CPU for video encoding](https://cdn.discordapp.com/attachments/804148977347330048/928879953825640458/av1an_preview.jpg)
