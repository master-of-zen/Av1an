 # Aomenc

GIT: [AOM](https://aomedia.googlesource.com/aom/)

## Table of Contents
1. [Command line options](#Important-command-line-options)
2. [Example of settings](#Examples-of-settings)

### Important command line options
| Command Line | Description  |
| -------------| -------------|
| --help | Show usage options and exit |
| --end-usage=arg | Rate control mode (vbr, cbr(default), cq, q) |
| --cpu-used=arg | CPU Used (0....6) Good mode, (5..9) realtime mode 1(default)|
| --cq-level=arg | Constant/Constrained Quality level, used in Q/CQ modes |
| --target-bitrate=arg | Bitrate (kbps) |
| --bit-depth=arg |  Bit depth (8, 10, 12) |
| --tile-columns=arg | Number of tile columns to use, log2 (number to power of 2) |
| --tile-rows=arg | Number of tile rows to use, log2  (number to power of 2)|
| --threads=arg | Allowed number of threads to use|

### Examples of settings

##### Constant quality:

` --end-usage=q --cq-level=30 --cpu-used=4 --threads=64 `

##### Target Bitrate:

`` --end-usage=vbr --target-bitrate=1000 --cpu-used=4 --threads=64 ``


##### Tiles:
` ... --tile-columns=2 --tile-rows=1 ...`