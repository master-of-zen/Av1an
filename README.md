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
<a href="https://codeclimate.com/github/master-of-zen/Av1an/maintainability"><img src="https://api.codeclimate.com/v1/badges/41ea7ad221dcdad3fe8d/maintainability" />
<img= src="https://app.codacy.com/manual/Grenight/Av1an?utm_source=github.com&utm_medium=referral&utm_content=master-of-zen/Av1an&utm_campaign=Badge_Grade_Dashboard"></a>
<a href="https://www.codacy.com/manual/Grenight/Av1an?utm_source=github.com&amp;utm_medium=referral&amp;utm_content=master-of-zen/Av1an&amp;utm_campaign=Badge_Grade"><img src="https://api.codacy.com/project/badge/Grade/4632dbb2f6f34ad199142c01a3eb2aaf"/></a>
</h4>
<h2 align="center">Easy, Fast, Efficient and Feature Rich</h2>

An easy way to start using VVC / AV1 / HEVC / H264 / VP9 / VP8 encoding. AOM, RAV1E, SVT-AV1, SVT-VP9, VPX, x265, x264, VTM(Experimental) are supported.

Example with default parameters:

    av1an -i input

With your own parameters:

    av1an -i input -enc aom -v "--cpu-used=3 --end-usage=q --cq-level=30 --threads=8" -w 10
    --split_method aom_keyframes --target_quality 95 --vmaf_path "vmaf_v0.6.1.pkl"
    -min_q 20 -max_q 60 -ff "-vf scale=-1:1080" -a "-c:a libopus -ac 2 -b:a 192k"
    -s scenes.csv -log my_log -o output



<h2 align="center">Usage</h2>

    -i   --input            Input file(s), or Vapoursynth (.py,.vpy) script
                            (relative or absolute path)

    -o   --output_file      Name/Path for output file (Default: (input file name)_(encoder).mkv)
                            Output file ending is always `.mkv`

    -enc --encoder          Encoder to use
                            (`aom`,`rav1e`,`svt_av1`,`svt_vp9`,`vpx`,`x265`, `x264`,`vvc`)
                            Default: aom
                            Example: -enc rav1e

    -v   --video_params     Encoder settings flags (If not set, will be used default parameters.)
                            Must be inside ' ' or " "

    -p   --passes           Set number of passes for encoding
                            (Default: AOMENC: 2, rav1e: 1, SVT-AV1: 1, SVT-VP9: 1,
                            VPX: 2, x265: 1, x264: 1, VVC:1)

    -w   --workers          Override number of workers.

    -r   --resume           If encode was stopped/quit resumes encode with saving all progress.
                            Resuming automatically skips scenedetection, audio encoding/copy,
                            splitting, so resuming only possible after actual encoding is started.
                            Temp folder must be present to resume.

    --no_check              Skip checking numbers of frames for source and encoded chunks.
                            Needed if framerate changes to avoid console spam.
                            By default, any differences in frames of encoded files will be reported.

    --keep                  Doesn't delete temporary folders after encode has finished.

    -q --quiet              Do not print tqdm to the terminal.

    -log --logging          Path to .log file(By default created in temp folder)

    --temp                  Set path for the temporary folder. Default: .temp

    --mkvmerge              Use mkvmerge for concatenating instead of FFmpeg.
                            Use when concatenation fails.

    -c  --config            Save/Read config file with encoder, encoder parameters,
                            FFmpeg and audio settings.
                            Options provided to cli overwrite config values.
                            All options except in/out/VMAF/log/temp/config paths are saved.

    --webm                  Outputs webm file.
                            Use only if you're sure the source video and audio are compatible.

<h3 align="center">FFmpeg options</h3>

    -a   --audio_params     FFmpeg audio settings (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'

    -ff  --ffmpeg           FFmpeg options video options.
                            Applied to each encoding segment individually.
                            (Warning: Cropping doesn't work with Target VMAF mode
                            without specifying it in --vmaf_filter)
                            Example:
                            --ff " -vf scale=320:240 "

    -fmt --pix_format       Setting custom pixel/bit format for piping
                            (Default: 'yuv420p10le')
                            Options should be adjusted accordingly, based on the encoder.

<h3 align="center">Segmenting</h3>

    --split_method          Method used for generating splits.(Default: PySceneDetect)
                            Options: `pyscene`, `aom_keyframes`, `none`
                            `pyscene` - PyScenedetect, content based scenedetection
                            with threshold.
                            `aom_keyframes` - using stat file of 1 pass of aomenc encode
                            to get exact place where encoder will place new keyframes.
                            (Keep in mind that speed also depends on set aomenc parameters)
                            `ffmpeg` - Uses FFmpeg built in content based scene detection
                            with threshold. Slower and less precise than pyscene but requires
                            fewer dependencies.
                            `none` -  skips scenedetection. Useful for splitting by time

    -cm  --chunk_method     Determine the method in which chunks are made for encoding.
                            By default the best method is selected automatically in this order:
                            vs_ffms2 > vs_lsmash > hybrid.
                            vs_ffms2 or vs_lsmash are recommended.
                            ['hybrid'(default), 'select', 'vs_ffms2', 'vs_lsmash']


    -tr  --threshold        PySceneDetect threshold for scene detection Default: 35

    -s   --scenes           Path to file with scenes timestamps.
                            If the file doesn't exist, a new file will be generated
                            in the current folder.
                            First run to generate stamps, all next reuse it.
                            Example: "-s scenes.csv"

    -xs  --extra_split      Adding extra splits if frame distance between splits bigger than the
                            given value. Pair with none for time based splitting or with any
                            other splitting method to break up massive scenes.
                            Example: 1000 frames video with a single scene,
                            -xs 200 will add splits at 200,400,600,800.

    --min_scene_len         Specifies the minimum number of frames in each split.

<h3 align="center">Target Quality</h3>

    --target_quality        Quality value to target.
                            VMAF used as substructure for algorithms.
                            Supported in all encoders supported by Av1an except for VVC.
                            Best works in range 85-97.
                            When using this mode, you must specify full encoding options.
                            These encoding options must include a quantizer based mode,
                            and some quantizer option provided. (This value will be replaced)
                            `--crf`,`--cq-level`,`--quantizer` etc

    --target_quality_method Type of algorithm for use.
                            Options: per_shot, per_frame.
                            Per frame is only supported in SVT-AV1.

    --min_q, --max_q        Min,Max Q values limits
                            If not set by the user, the default for encoder range will be used.

    --vmaf                  Calculate VMAF after encoding is done and make a plot.

    --vmaf_plots            Make plots for target quality search decisions
                            (Exception: early skips)
                            Saved in the temp folder by default.

    --vmaf_path             Custom path to libvmaf models.
                            example: --vmaf_path "vmaf_v0.6.1.pkl"
                            Recommended to place both files in encoding folder
                            (`vmaf_v0.6.1.pkl` and `vmaf_v0.6.1.pkl.model`)
                            (Required if VMAF calculation doesn't work by default)

    --vmaf_res              Resolution scaling for VMAF calculation,
                            vmaf_v0.6.1.pkl is 1920x1080 (by default),
                            vmaf_4k_v0.6.1.pkl is 3840x2160 (don't forget about vmaf_path)

    --probes                Number of probes for interpolation.
                            1 and 2 probes have special cases to try to work with few data points.
                            The optimal level is 4-6 probes. Default: 4

    --vmaf_filter           Filter used for VMAF calculation. The passed format is filter_complex.
                            So if crop filter used ` -ff " -vf crop=200:1000:0:0 "`
                            `--vmaf_filter` must be : ` --vmaf_filter "crop=200:1000:0:0"`

    --probing_rate          Setting rate for VMAF probes (Every N frame used in probe, Default: 4)

    --n_threads             Limit number of threads that are used for VMAF calculation
                            Example: --n_threads 12
                            (Required if VMAF calculation gives error on high core counts)

<h2 align="center">Main Features</h2>

**Splitting video by scenes for parallel encoding** because AV1 encoders are currently not very good at multithreading and encoding is limited to a very limited number of threads.

-   [PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for splitting video by scenes and running multiple encoders.
-   [Vapoursynth](http://www.vapoursynth.com) script input support.
-   Fastest way to encode AV1 without losing quality, as fast as many CPU cores you have :).
-   Target Quality mode. Targeting end result reference visual quality. VMAF used as a substructure
-   Resuming encoding without loss of encoded progress.
-   Simple and clean console look.
-   Automatic detection of the number of workers the host can handle.
-   Builds the encoding queue with bigger files first, minimizing waiting for the last scene to encode.
-   Both video and audio transcoding with FFmpeg.
-   Logging of the progress of all encoders.

## Install

<h2 align="center">Warning! Av1an GIT is currently under state of changing. Building and using latest Av1an GIT is differs from PIP stable.

[For current latest follow this instructions](https://gist.github.com/master-of-zen/0833bec1e7df72ed165083cd44e9187b). If latest changes not required, just use PIP version</h2>



-   Prerequisites:
    -   [Windows Prebuilds](https://ci.appveyor.com/project/master-of-zen/av1an/build/artifacts)
    -   [Install Python3](https://www.python.org/downloads/) <br>
        When installing under Windows, select the option `add Python to PATH` in the installer
    -   [Install FFmpeg](https://ffmpeg.org/download.html)
    -   Recommended to install vapoursynth with lsmash for faster and better processing

-   Encoder of choice:
    -   [Install AOMENC](https://aomedia.googlesource.com/aom/)
    -   [Install rav1e](https://github.com/xiph/rav1e)
    -   [Install SVT-AV1](https://gitlab.com/AOMediaCodec/SVT-AV1)
    -   [Install SVT-VP9](https://github.com/OpenVisualCloud/SVT-VP9)
    -   [Install vpx](https://chromium.googlesource.com/webm/libvpx/) VP9, VP8 encoding
    -   [Install VTM](https://vcgit.hhi.fraunhofer.de/jvet/VVCSoftware_VTM) VVC encoding test model

-   Optional :

    -   [Vapoursynth](http://www.vapoursynth.com/)
    -   [ffms2](https://github.com/FFMS/ffms2)
    -   [lsmash](https://github.com/VFR-maniac/L-SMASH-Works)
    -   [mkvmerge](https://mkvtoolnix.download/)

-   With a package manager:

    -   [PyPI](https://pypi.org/project/Av1an/)
    -   [AUR](https://aur.archlinux.org/packages/python-av1an/)

-   Manually:
    -   Clone Repo or Download from Releases
    -   `python setup.py install`

-   Also:
    On Ubuntu systems, the packages `python3-opencv` and `libsm6` are required

## Docker

Av1an can be run in a Docker container with the following command if you are in the current directory
Linux
```bash
docker run -v "$(pwd)":/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```
Windows
```powershell
docker run -v ${PWD}:/videos -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
```
Docker can also be built by using

```bash
docker build -t "av1an" .
```

To specify a different directory to use you would replace $(pwd) with the directory

```bash
docker run -v /c/Users/masterofzen/Videos:/videos --user $(id -u):$(id -g) -it --rm masterofzen/av1an:latest -i S01E01.mkv {options}
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
