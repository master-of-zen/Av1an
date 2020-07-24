<h1 align="center">
    <br>
    Av1an
    </br>
</h1>

<h2 align="center">A cross-platform framework to streamline encoding</h2>

![alt text](https://cdn.discordapp.com/attachments/702307493623103518/733639763919110184/prew3.png)

<h4 align="center">
<a href="https://discord.gg/bhkYVtF"><img src="https://discordapp.com/api/guilds/696849974230515794/embed.png" alt="Discord server" /></a>
<img src="https://github.com/master-of-zen/Av1an/workflows/tests/badge.svg">
<a href="https://codeclimate.com/github/master-of-zen/Av1an/maintainability"><img src="https://api.codeclimate.com/v1/badges/41ea7ad221dcdad3fe8d/maintainability" />
<img= src="https://app.codacy.com/manual/Grenight/Av1an?utm_source=github.com&utm_medium=referral&utm_content=master-of-zen/Av1an&utm_campaign=Badge_Grade_Dashboard"></a>
<a href="https://www.codacy.com/manual/Grenight/Av1an?utm_source=github.com&amp;utm_medium=referral&amp;utm_content=master-of-zen/Av1an&amp;utm_campaign=Badge_Grade"><img src="https://api.codacy.com/project/badge/Grade/4632dbb2f6f34ad199142c01a3eb2aaf"/></a>
</h4>
<h2 align="center">Easy, Fast, Efficient and Feature Rich</h2>

An easy way to start using VVC / AV1 / X265 / VP9 / VP8 encoding. AOM, rav1e, SVT-AV1, VPX, x265, VTM are supported.

Example with default parameters:

    av1an -i input

With your own parameters:

    av1an -i input -enc aom -v " --cpu-used=3 --end-usage=q --cq-level=30 --threads=8 " -w 10
     -ff " -vf scale=-1:720 "  -a " -c:a libopus -b:a 24k " -s scenes.csv -log my_log -o output

<h2 align="center">Usage</h2>

    -i   --file_path        Input file(s) (relative or absolute path). Will be processed with same
                            settings.

    -o   --output_file      Name/Path for output file (Default: (input file name)_av1.mkv)
                            Output file ending is always `.mkv`

    -enc --encoder          Encoder to use (`aom`,`rav1e`,`svt_av1`,`vpx`,`x265`,`vvc`. Default: aom)
                            Example: -enc rav1e

    -v   --video_params     Encoder settings flags (If not set, will be used default parameters.
                            Required for SVT-AV1s)
                            Must be inside ' ' or " "

    -p   --passes           Set number of passes for encoding
                            (Default: AOMENC: 2, rav1e: 1, SVT-AV1: 2, VPX: 2)
                            At current moment 2nd pass rav1e not working

    -a   --audio_params     FFmpeg audio settings flags (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'

    -w   --workers          Overrides automatically set number of workers.
                            Example: rav1e settings " ... --tile-rows 2 --tile-cols 2 ... " -w 3

    --split_method          Method used for generating splits.(Default: PySceneDetect)
                            Options: `pyscene`, `aom_keyframes`
                            `pyscene` - PyScenedetect, content based scenedetection
                            with threshold.
                            `aom_keyframes` - using stat file of 1 pass of aomenc encode
                            to get exact place where encoder will place new keyframes.

    -tr  --threshold        PySceneDetect threshold for scene detection Default: 50

    -s   --scenes           Path to file with scenes timestamps.
                            If given `0` spliting will be ignored
                            If file not exist, new will be generated in current folder
                            First run to generate stamps, all next reuse it.
                            Example: "-s scenes.csv"

    -xs  --extra_split      Adding extra splits if frame distance beetween splits bigger than
                            given value. Split only on keyframes. Works with/without PySceneDetect
                            Example: 1000 frames video with single scene,
                            -xs 200 will try to add splits at keyframes
                            that closest to 200,400,600,800.

    -cfg                    Save/Read config file with encoder, encoder parameters,
                            FFmpeg and audio settings.

    -ff  --ffmpeg           FFmpeg options. Applied to each segment individually.
                            Example:
                            --ff " -r 24 -vf scale=320:240 "

    -fmt --pix_format       Setting custom pixel/bit format(Default: 'yuv420p')
                            Example for 10 bit: 'yuv420p10le'
                            Encoding options should be adjusted accordingly.

    --resume                If encode was stopped/quit resumes encode with saving all progress
                            Resuming automatically skips scenedetection, audio encoding/copy,
                            spliting, so resuming only possible after actuall encoding is started.
                            /.temp folder must be presented for resume.

    --no_check              Skip checking numbers of frames for source and encoded chunks.
                            Needed if framerate changes to avoid console spam.
                            By default any differences in frames of encoded files will be reported.

    --keep                  Not deleting temprally folders after encode finished.

    -log --logging          Path to .log file(By default created in temp folder)

    --temp                  Set path for temporally folders. Default: .temp

    --boost                 Enable experimental CQ boosting for dark scenes. 
                            Aomenc/VPX only. See 1.7 release notes.

    -br --boost_range       CQ limit for boosting. CQ can't get lower than this value.

    -bl --boost_limit       CQ range for boosting. Delta for which CQ can be changed

    --vmaf                  Calculate vmaf for each encoded clip.
                            Saves plot after encode, showing vmaf values for all frames,
                            mean, 1,25,75 percentile.

    --vmaf_path             Custom path to libvmaf models. By default used system one.

    --vmaf_target           Vmaf value to target. Currently aomenc only.
                            Best works with 85-97.
                            When using this mode specify full encoding options.
                            ` --end-usage=q ` or ` --end-usage=cq `
                            with ` --cq-level=N ` must be specified.

    --vmaf_steps            Number of probes for interpolation.
                            Must be bigger than 3. Optimal is 4-6 probes. Default: 4

    --min_q, --max_q      Min,Max Q values limits for Target VMAF
                            If not set by user, encoder default will be used.

<h2 align="center">Main Features</h2>

**Spliting video by scenes for parallel encoding** because AV1 encoders are currently not good at multithreading, encoding is limited to single or couple of threads at the same time.

*  [PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for spliting video by scenes and running multiple encoders.
*  Fastest way to encode AV1 without losing quality, as fast as many CPU cores you have :).
*  Target VMAF mode. Targeting end result visual quality.
*  Resuming encoding without loss of encoded progress.
*  Simple and clean console look.
*  Automatic detection of the number of workers the host can handle.
*  Building encoding queue with bigger files first, minimizing waiting for the last scene to encode.
*  Both video and audio transcoding with FFmpeg.
*  Logging of progress of all encoders.
*  "Boosting" quality of dark scenes based on their brightness.

## Install

* Prerequisites:
  *  [Install Python3](https://www.python.org/downloads/) <br>
When installing under Windows, select the option `add Python to PATH` in the installer
  *  [Install FFmpeg](https://ffmpeg.org/download.html)
* Encoder of choice:
  *  [Install AOMENC](https://aomedia.googlesource.com/aom/)
  *  [Install rav1e](https://github.com/xiph/rav1e)
  *  [Install SVT-AV1](https://github.com/OpenVisualCloud/SVT-AV1)
  *  [Install vpx](https://chromium.googlesource.com/webm/libvpx/) VP9, VP8 encoding
  *  [Install VTM](https://vcgit.hhi.fraunhofer.de/jvet/VVCSoftware_VTM) VVC encoding test model

* With a package manager:
  *  [PyPI](https://pypi.org/project/Av1an/)
  *  [AUR](https://aur.archlinux.org/packages/python-av1an/)

* Manually:
  *  Clone Repo or Download from Releases
  *  `python setup.py install`
* Also:
    On Ubuntu systems packages `python3-opencv` and `libsm6` are required

### I have no money

Bitcoin - 1gU9aQ2qqoQPuvop2jqC68JKZh5cyCivG
