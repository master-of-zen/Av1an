
<h1 align="center">
    <br>
    Av1an
    </br>
</h1>

<h2 align="center">All-in-one tool for streamlining av1 encoding</h2>

![alt text](https://cdn.discordapp.com/attachments/665440744567472169/666865780012482571/Screenshot_20200115_064531.png)

<h2 align="center">Easy And Efficient </h2>

Start using AV1 encoding. All open-source encoders are supported (Aom, Rav1e, SVT-AV1).
Avif encoding supported (Aomenc, Rav1e)

Example with default parameters:

    ./av1an.py -i input

With your own parameters:

    ./av1an.py -i input -enc aom -e '--cpu-used=3 --end-usage=q --cq-level=30' -a '-c:a libopus -b:a 24k'

<h2 align="center">Usage</h2>

    -i   --file_path        Input file (relative or absolute path)
    
    -o   --output_file      Name/Path for output file (Default: (input file name)_av1.mkv)
    
    -m   --mode             0 - Video encoding (Default), 1 - Image encoding
                            By default used 10 bit encoding. 
                            Constant quality mode for Aomenc
    
    -enc --encoder          Encoder to use (aom or rav1e or svt_av1. Default: aom)
                            Example: -enc rav1e

    -e   --encoding_params  Encoder settings flags (If not set, will be used default parameters. 
                            Required for SVT-AV1s)
                            Can be set for both video and image mode
                            Must be inside ' ' or " "
     
    -a   --audio_params     FFmpeg audio settings flags (Default: copy audio from source to output)
                            Example: -a '-c:a libopus -b:a  64k'
    
    -t   --workers          Maximum number of workers (overrides automatically set number of workers.
                            Aomenc recommended value is YOUR_THREADS - 2 (Single thread per worker)
                            Rav1e and SVT-AV1 uses multiple threads, 
                            Example: '--tile-rows 2 --tile-cols 2' load 2.5 - 3.5 threads
                            4 rav1e workers is optimal for 6/12 cpu 
        
    -sc  --pyscene          PySceneDetect options.Example:
                            -sc ' -t 30 -d 7 -m 48 '
    
    -s   --scenes           Path to PySceneDetect generated .csv file.
                            If file not exist, new one will be generated in current folder
                            Example of usage:
                            First run to generate: `-s anything`
                            All next runs to reuse generated file: `-s video-Scenes.scv`
    
    -p   --pass             Set number of passes for encoding 
                            (Default: Aomenc: 2, Rav1e: 1, SVT-AV1: 2)
                            At current moment 2nd pass Rav1e not working
    
    -ff  --ffmpeg_com       FFmpeg options. Example: 
                            --ff ' -r 24 -vf scale=320:240 '  
    
    -fmt --pix_format       Setting custom pixel/bit format(Default: 'yuv420p')
                            Example for 10 bit: 'yuv420p10le'
                            Encoding options should be adjusted accordingly
    
    -log --logging          Path to .log file(Default: no logging)
                            Currently not working on Windows

<h2 align="center">Main Features</h2>

**Spliting video by scenes for parallel encoding** because AV1 encoders currently not good at multithreading, encoding is limited to single or couple of threads at the same time.

[PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for spliting video by scenes and running multiple encoders.

Both Video and Avif Image encoding

Simple and clean console look

Automatic determination of how many workers the host can handle

Building encoding queue with bigger files first, minimizing waiting for last scene to encode

Both video and audio encoding option with FFmpeg

And many more to go..

## Install on Windows

* [Python3](https://www.python.org/downloads/)
* [FFmpeg](https://ffmpeg.org/download.html) with ffprobe and add it on env variable
* [PyScenedetect](https://pyscenedetect.readthedocs.io/en/latest/)
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
* [PyScenedetect](https://pyscenedetect.readthedocs.io/en/latest/)
* [AOMENC](https://aomedia.googlesource.com/aom/) For Aomenc encoder
* [Rav1e](https://github.com/xiph/rav1e) For Rav1e encoder
* [SVT-AV1](https://github.com/OpenVisualCloud/SVT-AV1) For SVT-AV1 encoder

Install all requirements listed in `requirements` file

Optionally add Av1an to your PATH

Run with command: `av1an.py params...`
