
<h1 align="center">
    <br>
    Av1an
    </br>
</h1>

<h2 align="center">All-in-one tool for streamlining av1 encoding</h2>

![alt text](https://cdn.discordapp.com/attachments/665440744567472169/678627425973698599/172948_16_02_202.png)

<h4 align="center"> <img src="https://ci.appveyor.com/api/projects/status/cvweipdgphbjkkar?svg=true" alt="Project Badge"> <a href="https://codeclimate.com/github/master-of-zen/Av1an/maintainability"><img src="https://api.codeclimate.com/v1/badges/41ea7ad221dcdad3fe8d/maintainability" /></a></h4>
<h2 align="center">Easy And Efficient </h2>

Start using AV1 encoding. All open-source encoders are supported (Aom, Rav1e, SVT-AV1).
Image to .ivf encoding supported (Aomenc, Rav1e)

Example with default parameters:

    ./av1an.py -i input

With your own parameters:

    ./av1an.py -i input -enc aom -e '--cpu-used=3 --end-usage=q --cq-level=30' -a '-c:a libopus -b:a 24k'

<h2 align="center">Usage</h2>

    -i   --file_path        Input file (relative or absolute path)

    -o   --output_file      Name/Path for output file (Default: (input file name)_av1.mkv)

    -enc --encoder          Encoder to use (aom or rav1e or svt_av1. Default: aom)
                            Example: -enc rav1e

    -e   --encoding_params  Encoder settings flags (If not set, will be used default parameters.
                            Required for SVT-AV1s)
                            Can be set for both video and image mode
                            Must be inside ' ' or " "

    -p   --pass             Set number of passes for encoding
                            (Default: Aomenc: 2, Rav1e: 1, SVT-AV1: 2)
                            At current moment 2nd pass Rav1e not working

    -a   --audio_params     FFmpeg audio settings flags (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'

    -t   --workers          Maximum number of workers (overrides automatically set number of workers.
                            Aomenc recommended value is YOUR_THREADS - 2 (Single thread per worker)
                            Rav1e and SVT-AV1 uses multiple threads,
                            Example: '--tile-rows 2 --tile-cols 2' load 2.5 - 3.5 threads
                            4 rav1e workers is optimal for 6/12 cpu

    -s   --scenes           Path to file with scenes timestamps.
                            If given `0` spliting will be ignored
                            If file not exist, new will be generated in current folder
                            First run to generate stamps, all next reuse it.
                            Example: `-s scenes.csv`

    -tr  --threshold        PySceneDetect threshold for scene detection

    -ff  --ffmpeg_com       FFmpeg options. Example:
                            --ff ' -r 24 -vf scale=320:240 '

    -fmt --pix_format       Setting custom pixel/bit format(Default: 'yuv420p')
                            Example for 10 bit: 'yuv420p10le'

    --resume                If encode was stopped/quit resumes encode with saving all progress
                            Resuming automatically skips scenedetection, audio encoding/copy,
                            spliting, so resuming only possible after actuall encoding is started.
                            /.temp folder must be presented for resume.

    --no_check              Skip checking numbers of frames for source and encoded chunks
                            Needed if framerate changes.
                            By default any differences in frames of encoded files will be reported

     -m   --mode            0 - Video encoding (Default), 1 - Image encoding
                            By default used 10 bit encoding.
                            Constant quality mode for Aomenc
                            Encoding options should be adjusted accordingly

    -log --logging          Path to .log file(Default: no logging)

<h2 align="center">Main Features</h2>

**Spliting video by scenes for parallel encoding** because AV1 encoders currently not good at multithreading, encoding is limited to single or couple of threads at the same time.

* [PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for spliting video by scenes and running multiple encoders.

* Both Video and Image encoding* (Single frame to .ivf, can be viewed by videoplayer)

* Resuming encoding without loss of encoded progress

* Simple and clean console look

* Automatic determination of how many workers the host can handle

* Building encoding queue with bigger files first, minimizing waiting for last scene to encode

* Both video and audio encoding option with FFmpeg

* Logging of progress of all encoders

And many more to go..

## Install on Windows

[![Codacy Badge](https://api.codacy.com/project/badge/Grade/75960961ad934d1b988faa24dfab4417)](https://app.codacy.com/manual/Grenight/Av1an?utm_source=github.com&utm_medium=referral&utm_content=master-of-zen/Av1an&utm_campaign=Badge_Grade_Dashboard)

#### 1. Use ready [Release](https://github.com/master-of-zen/Av1an/releases)
   Autobuilding .exe from current git available at [AppVeyour](https://ci.appveyor.com/project/master-of-zen/av1an)

#### 2. Install with dependancies
* [Python3](https://www.python.org/downloads/)
* [FFmpeg](https://ffmpeg.org/download.html) with ffprobe and add it on env variable
* [AOMENC](https://aomedia.googlesource.com/aom/) For Aomenc encoder
* [Rav1e](https://github.com/xiph/rav1e) For Rav1e encoder
* [SVT-AV1](https://github.com/OpenVisualCloud/SVT-AV1) For SVT-AV1 encoder

Install all requirements listed in `requirements` file.

If installed programs don't added to enviroment variable,
executables can be put in same folder with av1an

Run with command: `python -i ./avian.py params..`

## Install on Linux

* [Python3](https://www.python.org/downloads/)
* [FFmpeg](https://ffmpeg.org/download.html) with ffprobe
* [AOMENC](https://aomedia.googlesource.com/aom/) For Aomenc encoder
* [Rav1e](https://github.com/xiph/rav1e) For Rav1e encoder
* [SVT-AV1](https://github.com/OpenVisualCloud/SVT-AV1) For SVT-AV1 encoder

Install all requirements listed in `requirements` file

Optionally add Av1an to your PATH

Run with command: `av1an.py params...`
