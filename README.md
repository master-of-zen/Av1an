
<h1 align="center">
    <br>
    Av1an
    </br>
</h1>

<h2 align="center">All-in-one tool for streamline and easy av1 encoding</h2>

![alt text](https://cdn.discordapp.com/attachments/665440744567472169/665760393498460196/banner.jpg)

<h2 align="center">  Easy And Efficient </h2>



Start using AV1 encoding. At current moment only available encoders are Aomenc, Rav1e.
 
Example with default parameters:

    ./avian.py -i input

Your own parameters:

    ./avian.py -i input -enc aomenc -e '--cpu-used=3 --end-usage=q --cq-level=30' -a '-c:a libopus -b:a 24k'

<h2 align="center">Main Features</h2>

#### Spliting video by scenes for parallel encoding

AV1 encoders at current moment not good at multithreading so encoding limited to single or couple of cores at the same time.

[PySceneDetect](https://pyscenedetect.readthedocs.io/en/latest/) used for spliting video by scenes and running multiple encoders.

Simple and clean console look

Automatic determination of how many workers PC can handle

Building encoding queue with bigger files first, minimizing waiting for last scene to encode

Both video and audio encoding option with FFmpeg

And many more to go..

## Dependencies

* [FFmpeg](https://ffmpeg.org/download.html)
* [AOMENC](https://aomedia.googlesource.com/aom/)
* [PyScenedetect](https://pyscenedetect.readthedocs.io/en/latest/)
* [mkvmerge/python-pymkv](https://pypi.org/project/pymkv/)
