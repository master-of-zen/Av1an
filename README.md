<h1 align="center">
    <br>
    Av1an
    </br>
</h1>

<h2 align="center">A cross-platform framework to streamline encoding</h2>

![alt text](https://cdn.discordapp.com/attachments/696849974666985494/774368268860915732/av1an_pick2.png)

<h4 align="center">
<a href="https://discord.gg/Ar8MvJh"><img src="https://discordapp.com/api/guilds/696849974230515794/embed.png" alt="Discord server" /></a>
<img src="https://github.com/master-of-zen/Av1an/workflows/tests/badge.svg">
<a href="https://crates.io/crates/av1an"><img src="https://img.shields.io/crates/v/av1an.svg"></a>
    
</h4>
<h2 align="center">Easy, Fast, Efficient and Feature Rich</h2>

### <center>An easy way to start using AV1 / HEVC / H264 / VP9 / VP8 encoding. AOM, RAV1E, SVT-AV1, VPX, x265, x264 are supported</center>

Example with default parameters:

    av1an -i input

With your own parameters:

    av1an -i input -v " --cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10
    --target-quality 95 -a " -c:a libopus -ac 2 -b:a 192k" -l my_log -o output.mkv

<h2 align="center">Usage</h2>

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

<h3 align="center">FFmpeg options</h3>

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

### <center>Segmenting<center>

    --split-method          Method used for generating splits. (Default: av-scenechange)
                            Options: `av-scenechange`, `none`
                            `none` -  skips scenedetection.

    -m  --chunk-method      Determine the method in which chunks are made for encoding.
                            By default the best method is selected automatically.
                            [possible values: segment, select, ffms2, lsmash, hybrid]

    -s  --scenes            File to save/read scenes.

    -x  --extra-split       Size of chunk after which it will be split [default: 240]

    --min-scene-len         Specifies the minimum number of frames in each split.

<h3 align="center">Target Quality</h3>

    --target-quality        Quality value to target.
                            VMAF used as substructure for algorithms.
                            When using this mode, you must use quantizer/quality modes of enocoder.

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

**Splitting video by scenes for parallel encoding** because AV1 encoders are currently not very good at multithreading and encoding is limited to a very limited number of threads.

- [Vapoursynth](http://www.vapoursynth.com) script input support.
- Speed up video encoding.
- Target Quality mode. Targeting end result reference visual quality. VMAF used as a substructure
- Resuming encoding without loss of encoded progress.
- Simple and clean console look.
- Automatic detection of the number of workers the host can handle.
- Both video and audio transcoding.

## Install

- With a package manager:
  - Cargo: `cargo install av1an`
  - Arch Linux: `pacman -S av1an`

- Prerequisites:
  - [Install FFmpeg](https://ffmpeg.org/download.html)
  - Recommended to install vapoursynth with lsmash/ffms2 for faster and better processing

- Encoder of choice:
  - [Install AOMENC](https://aomedia.googlesource.com/aom/)
  - [Install rav1e](https://github.com/xiph/rav1e)
  - [Install SVT-AV1](https://gitlab.com/AOMediaCodec/SVT-AV1)
  - [Install vpx](https://chromium.googlesource.com/webm/libvpx/) VP9, VP8 encoding

- Optional :
  - [Vapoursynth](http://www.vapoursynth.com/)
  - [ffms2](https://github.com/FFMS/ffms2)
  - [lsmash](https://github.com/VFR-maniac/L-SMASH-Works)
  - [mkvmerge](https://mkvtoolnix.download/)

- Manually:
  - Clone Repo or Download from Releases
  - `cargo build --release`

## Docker

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
