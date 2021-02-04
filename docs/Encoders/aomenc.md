 # Aomenc

GIT: [AOM](https://aomedia.googlesource.com/aom/)

## Table of Contents
- [Important command line options](#important-command-line-options)
- [Example settings and explanation](#example-settings-and-explanation)

### Important command line options
| Command Line | Description  |
| -------------| -------------|
| --help | Show usage options and exit |
| --end-usage=arg | Rate control mode (vbr, cbr(default), cq, q). VBR and CBR are self explanatory. CQ is constant quantizer with a bitrate ceiling(Constrained Quality), Q is for Quality. |
| --cq-level=arg | Constant/Constrained Quality level, used in Q/CQ modes. |
| --cpu-used=arg | CPU Used (0..6) Good mode, (5..9) realtime mode 1(default). Default is good mode(CPU-0 to CPU-6): unless you specify the realtime flag(--rt), every speed set above 6 will default back to 6. Lower numbers are slower. |
| --target-bitrate=arg | Bitrate (kbps) |
| --bit-depth=arg |  Bit depth (8, 10, 12). Default is the bit-depth recognized by aomenc from the source file. 12-bit is not recommended for end compression, as it is not supported in the main AV1 HW decoding profile. |
| --tile-columns=arg | Number of tile columns to use, log2 (number to power of 2). With --tile-columns=2, it'll do 2²= 4 tile columns. |
| --tile-rows=arg | Number of tile rows to use, log2  (number to power of 2). With --tile-rows=1, it'll do 2¹= 2 tile rows. |
| --threads=arg | Allowed number of threads to use. |
| --lag-in-frames=arg | Number of lagged frames used by the encoder for lookahead and alternate reference frame placement(default 19, max 35).
| --enable-cdef=arg | Enable the constrained directional enhancement filter (0: false, 1: true (default)). CDEF is a very powerful filter used to clean up ringing, haloing and ringing artifacts. It works very well in that regard. |
| --aq-mode=arg | Adaptive quantization mode(0: default. 1: Variance. 2: Complexity. 3: Cyclic Refresh |
| --tune-content=arg | Tune content type (default,screen). |
| --enable-fwd-kf=arg | Enable forward reference keyframes(default=0). |
| --kf-min-dist=arg | Minimum keyframe interval in frames(default=12). |
| --kf-max-dist=arg | Maximum keyframe interval in frames(default=9999, or adaptive keyframe placement only). |
| --enable-keyframe-filtering=arg | Apply temporal filtering on key frame(0: no filter, 1: filter without overlay (default), 2: filter with overlay - experimental, may break random access in players.). It is recommended to leave it at default unless you really know what you're doing. |
| --arnr-maxframes=arg | Maximum number of alternate reference noise reduced frames used by the encoder(default=7). |
| --arnr-strength=arg | ARNR frames filtering strength(default=5). |
| --enable-qm=arg | Enable quantisation matrices (0: false (default), 1: true). |
| --quant-b-adapt=arg | Use adaptive quantize_b(default=0). |
| --mv-cost-upd-freq=arg | Update freq for mv costs(motion vector estimation cost calculation) 0: SB(SuperBlock), 1: SB Row per Tile, 2: Tile, 3: Off. |
| --enable-chroma-deltaq=arg | Enable chroma delta quant (0: false (default), 1: true). May be broken below --cq-level=15.
| --color-primaries=arg | Color primaries (CICP) of input content: bt709, unspecified, bt601, bt470m, bt470bg, smpte240, film, bt2020, xyz, smpte431, smpte432, ebu3213. | Leave at default unless you have HDR content or your source's color-primaries information is different; in that case, set it to whatever your content is, usually BT2020.
| --transfer-characteristics=arg | Transfer characteristics (CICP) of input content(unspecified, bt709, bt470m, bt470bg, bt601, smpte240, lin, log100, log100sq10, iec61966, bt1361, srgb, bt2020-10bit, bt2020-12bit, smpte2084, hlg, smpte428. | Leave at default unless you have HDR content or your source's transfer characteristics are different; in that case, set it to whatever your content is.
| --matrix-coefficients=arg | Matrix coefficients (CICP) of input content: identity, bt709, unspecified, fcc73, bt470bg, bt601, smpte240, ycgco, bt2020ncl, bt2020cl, smpte2085, chromncl, chromcl, ictcp. | Leave at default unless you have HDR content or your source's matrix coefficients information is different; in that case, set it to whatever your content is.

### Example settings and explanation

##### Constant quality:

` --end-usage=q --cq-level=30 --cpu-used=4 --threads=16 `

It is recommended to set it the rate control --end-usage=q to get the highest quality rate control method possible. Only use CQ if you are planning to stream with a maximum bitrate, and CBR for livestreaming.

It is recommended to the --cq-level in range  20-40 depending on your source.

##### Target Bitrate:

`` --end-usage=vbr --target-bitrate=1000 --cpu-used=4 --threads=16 ``

To get good efficiency with VBR, it is strongly recommended to use aomenc in 2-pass mode(which is the default in av1an).

##### Tiles(tile columns and rows)
` ... --tile-columns=2 --tile-rows=1 ...`

For highest efficiency while keeping good threading, it is recommended to set it to --tile-columns=2 and --tile-rows=1 at 1080p. For higher resolution/higher framerate  encoding, set it to --tile-columns=3 and --tile-rows=2.

#### CPU preset:
` ... --cpu-used=6 ... `

--cpu-used=6 is recommended if you want good compression efficiency and fast encoding, and --cpu-used=4 if you want to crank up the efficiency even further. It is not recommended to use slower presets as they are much slower without much gain.

#### Bit-depth:
` ... --bit-depth=10 ... `
It is recommended to set it to 10-bit even for 8-bit content  for higher efficiency and less banding due to no precision losses going from an 8-bit YUV source to 8-bit YUV, and allows for higher calculation precision. However, as of January 23rd 2021, it is not very easy to decode 10-bit AV1 video on x86_64 CPUs due to a lack of assembly optimizations. For now, use 10-bit encoding for 1080p30 content. Otherwise, use 8-bit with --bit-depth=8.

#### Threading and tile threading
` ... --threads=8 --tile-columns=1 --tile-rows=0 ... ` or ` ... --threads=16 --tile-columns=2 --tile-rows=1 ... ` for single instance encoding.

If you have say an 8C/16T CPU, it is recommended to set to #cores/2 you have for chunked encoding. If you have a higher thread count CPU, limiting it to 4 threads is also a good option to prevent thread oversaturation. Give it how many threads you have for single worker encoding.

#### Lag-in-frames
` ... --lag-in-frames=35 ... `

 More is better, but it makes the encoder slower, up to a limit of 35 (default is 19).
 
#### CDEF usage
` ... --enable-cdef=0 ... ` if you want to encode >1080p30 10-bit as of January 2021.

For video game content and content in 8-bit, I always recommend keeping it on, as it makes the image much cleaner overall by preventing aliasing from ruining the imag, and gives a nice boost to subjective quality. However, for 10-bit AV1 on x86_64 CPUs, there is currently a big decoding performance penalty, so if you have >1080p30 footage or want maximum decode performance NOW, you can set --enable-cdef=0.

#### Adaptive quantization mode
` ... --aq-mode=2 ... ` 

Adaptive quantization mode (0: Default. 1: Variance, in which the encoder lowers the quantizer value for flat low complexity blocks, like clouds or dark scenes. Not recommended unless you know what you are doing. 2: Complexity, in which the encoder lowers the quantizer for complex parts of the image. 3: Cyclic refresh. Ups the quantizer value as much as possible for static flat looking blocks, only doing block refreshes in cycles if needed. Not recommend unless when used for live-streaming or conferencing.)

#### Content tune
` ... --tune-content=default ...` If you are encoding without the tune, it it **not necessary to specify it**.

default: tuned for most content. screen: tuned for screen recordings, low complexity animations, and most videos games. Not recommended for complex animation like anime as it disables some post-processing making it look worse in some way.

#### Forward keyframes and maximum adaptive keyframe distance
` ... --enable-fwd-kf=1 --kf-max-dist=240 ... `

It is recommended to set it to a MAX of 10s worth of frames, or 240 for 24FPS, 300 for 30FPS, and 600 for 60FPS for easier seeking. For video game content or >=60FPS content, is is also possible to use a max of 5s worth of frames to help with seeking performance. Leave it to default if you want maximum efficiency at the cost of worse seeking performance in some footage.

####  Alternative Reference Noise Reduced frames max number and ARNR strength
` ... --arnr-maxframes=7 --arnr-strength=5 ... ` is the default

For --arnr-maxframes: It is recommended to leave it at default unless you want to crank up the efficiency for low motion scenes. 
For --arnr-strength: It is recommended to leave it at default, although you can lower it to 4 for slightly better noise retention. 

####  Other flags that can be used to give higher efficiency
` ... --enable-qm=1 --enable-chroma-deltaq=1 --quant-b-adapt=1 --mv-cost-upd-freq=2 ... ` Description of what each setting does can be found in the description above.

#### Flags used for most native 10-bit HDR content
` ... --color-primaries=bt2020 --transfer-characteristics=smpte2084 --matrix-coefficients=bt2020ncl ... `

#### Example command line for great efficiency with good speed for chunked encoding with an 8C16T CPU(1080p24-1080p30 content)
` --end-usage=q --cq-level=22 --cpu-used=6 --threads=8 --tile-columns=1 --tile-rows=0 --bit-depth=10 --lag-in-frames=35 --enable-fwd-kf=1 --kf-max-dist=240 --enable-qm=1 --enable-chroma-deltaq=1 --quant-b-adapt=1 --mv-cost-upd-freq=2 `

#### Example command line for good efficiency with faster settings and better threading to reduce number of workers needed and RAM consumption
` --end-usage=q --cq-level=22 --cpu-used=6 --threads=16  --tile-columns=2 --tile-rows=1 --bit-depth=10 --enable-fwd-kf=1 --kf-max-dist=240 `

