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
    --split_method aom_keyframes --target_quality 95 --vmaf_path "vmaf_v0.6.1.pkl" -min_q 20 -max_q 60
    -ff "-vf scale=-1:1080" -a "-c:a libopus -ac 2 -b:a 192k" -s scenes.csv -log my_log -o output

<h2 align="center">Usage</h2>

    -i   --input            Input file(s) (relative or absolute path)

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

    -r   --resume           If encode was stopped/quit resumes encode with saving all progress
                            Resuming automatically skips scenedetection, audio encoding/copy,
                            spliting, so resuming only possible after actuall encoding is started.
                            temp folder must be presented for resume.

    --no_check              Skip checking numbers of frames for source and encoded chunks.
                            Needed if framerate changes to avoid console spam.
                            By default any differences in frames of encoded files will be reported.

    --keep                  Not deleting temprally folders after encode finished.

    -q --quiet              Do not print tqdm to terminal.

    -log --logging          Path to .log file(By default created in temp folder)

    --temp                  Set path for temporally folders. Default: .temp

    -cfg --config           Save/Read config file with encoder, encoder parameters,
                            FFmpeg and audio settings.

    --mkvmerge              Use mkvmerge for concatenating instead of ffmpeg.
                            Use in case when concatenation fails.

    -c  --config            Save config file with given name if doesn't exists.
                            Reads config file with that name.
                            Options provided to cli overwrite config values.
                            All options excepth in/out/vmaf/log/temp/config paths are saved.

    --webm                  Outputs webm file.
                            Use only if you're sure source video and audio is compatible.

<h3 align="center">FFmpeg options</h3>

    -a   --audio_params     FFmpeg audio settings (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'

    -ff  --ffmpeg           FFmpeg options video options. Applied to each encoding segment individually.
                            (Warning: Cropping doesn't work with Target VMAF mode without
                            specifying it in --vmaf_filter)
                            Example:
                            --ff " -vf scale=320:240 "

    -fmt --pix_format       Setting custom pixel/bit format for piping
                            (Default: 'yuv420p10le')
                            Based on encoder, options should be adjusted accordingly.

<h3 align="center">Segmenting</h3>

    --split_method          Method used for generating splits.(Default: PySceneDetect)
                            Options: `pyscene`, `aom_keyframes`, `none`
                            `pyscene` - PyScenedetect, content based scenedetection
                            with threshold.
                            `aom_keyframes` - using stat file of 1 pass of aomenc encode
                            to get exact place where encoder will place new keyframes.
                            (Keep in mind that speed also depends on set aomenc parameters)
                            `ffmpeg` - Uses ffmpeg built in content based scene detection
                            with threshold. Slower and less precise than pyscene but requires
                            fewer dependencies.
                            `none` -  skips scenedetection. Useful for splitting by time

    -cm  --chunk_method     Determine way in which chunks made for encoding.
                            By default selected best one avalable.
                            vs_ffms2 > vs_lsmash > hybrid
                            ['hybrid'(default), 'select', 'vs_ffms2'(Recomended To Install), 'vs_lsmash']

    -tr  --threshold        PySceneDetect threshold for scene detection Default: 35

    -s   --scenes           Path to file with scenes timestamps.
                            If file not exist, new will be generated in current folder
                            First run to generate stamps, all next reuse it.
                            Example: "-s scenes.csv"

    -xs  --extra_split      Adding extra splits if frame distance beetween splits bigger than
                            given value. Pair with none for time based splitting or with any
                            other splitting method to break up massive scenes.
                            Example: 1000 frames video with single scene,
                            -xs 200 will add splits at 200,400,600,800.


<h3 align="center">Target Quality</h3>


    --target_quality        Quality value to target.
                            VMAF used as substructure for algorithms.
                            Supported for all encoders(Exception:VVC).
                            Best works in range 85-97.
                            When using this mode specify full encoding options.
                            Encoding options must include quantizer based mode,
                            and some quantizer option provided. (This value got replaced)
                            `--crf`,`--cq-level`,`--quantizer` etc

    --target_quality_method Type of algorithm for use.
                            Options: per_shot, per_frame.
                            Per frame avalable only for SVT-AV1.

    --min_q, --max_q        Min,Max Q values limits
                            If not set by user, default for encoder range will be used.

    --vmaf                  Calculate vmaf after encode is done and make a plot.

    --vmaf_plots            Make plots for target quality search decisions
                            (Exception: early skips)
                            Saved in temp folder

    --vmaf_path             Custom path to libvmaf models.
                            example: --vmaf_path "vmaf_v0.6.1.pkl"
                            Recomended to place both files in encoding folder
                            (`vmaf_v0.6.1.pkl` and `vmaf_v0.6.1.pkl.model`)
                            (Required if vmaf calculation doesn't work by default)

    --vmaf_res              Resolution scaling for vmaf calculation,
                            vmaf_v0.6.1.pkl is 1920x1080 (by default),
                            vmaf_4k_v0.6.1.pkl is 3840x2160 (don't forget about vmaf_path)

    --probes                Number of probes for interpolation.
                            1 and 2 probes have special cases to try to work with few data points.
                            Optimal is 4-6 probes. Default: 4

    --vmaf_filter           Filter used for vmaf calculation. Passed format is filter_complex.
                            So if crop filter used ` -ff " -vf crop=200:1000:0:0 "`
                            `--vmaf_filter` must be : ` --vmaf_filter "crop=200:1000:0:0"`

    --probing_rate             Setting rate for vmaf probes (Every N frame used in probe, Default: 4)

    --n_threads             Limit number of threads that used for vmaf calculation
                            Example: --n_threads 12
                            (Required if VMAF calculation gives error on high core counts)



<h2 align="center">Main Features</h2>

**Spliting video by scenes for parallel encoding** because AV1 encoders are currently not good at multithreading, encoding is limited to single or couple of threads at the same time.

*  [PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for spliting video by scenes and running multiple encoders.
*  Fastest way to encode AV1 without losing quality, as fast as many CPU cores you have :).
*  Target Quality mode. Targeting end result reference visual quality. VMAF used as substructure
*  Resuming encoding without loss of encoded progress.
*  Simple and clean console look.
*  Automatic detection of the number of workers the host can handle.
*  Building encoding queue with bigger files first, minimizing waiting for the last scene to encode.
*  Both video and audio transcoding with FFmpeg.
*  Logging of progress of all encoders.

## Install

* Prerequisites:
  *  [Windows Prebuilds](https://ci.appveyor.com/project/master-of-zen/av1an/build/artifacts)
  *  [Install Python3](https://www.python.org/downloads/) <br>
When installing under Windows, select the option `add Python to PATH` in the installer
  *  [Install FFmpeg](https://ffmpeg.org/download.html)
  * Recommended to install vapoursynth with lsmash for faster and better processing
* Encoder of choice:
  *  [Install AOMENC](https://aomedia.googlesource.com/aom/)
  *  [Install rav1e](https://github.com/xiph/rav1e)
  *  [Install SVT-AV1](https://github.com/OpenVisualCloud/SVT-AV1)
  *  [Install SVT-VP9](https://github.com/OpenVisualCloud/SVT-VP9)
  *  [Install vpx](https://chromium.googlesource.com/webm/libvpx/) VP9, VP8 encoding
  *  [Install VTM](https://vcgit.hhi.fraunhofer.de/jvet/VVCSoftware_VTM) VVC encoding test model
 * Optional :
   * [Vapoursynth](http://www.vapoursynth.com/)
   * [ffms2](https://github.com/FFMS/ffms2)
   * [lsmash](https://github.com/VFR-maniac/L-SMASH-Works)
   * [mkvmerge](https://mkvtoolnix.download/)

* With a package manager:
  *  [PyPI](https://pypi.org/project/Av1an/)
  *  [AUR](https://aur.archlinux.org/packages/python-av1an/)

* Manually:
  *  Clone Repo or Download from Releases
  *  `python setup.py install`
* Also:
    On Ubuntu systems packages `python3-opencv` and `libsm6` are required

### Support developer

Bitcoin - 1gU9aQ2qqoQPuvop2jqC68JKZh5cyCivG
