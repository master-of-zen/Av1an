# Aomenc

GIT: [AOM](https://aomedia.googlesource.com/aom/)

## Table of Contents
- [Important command line options](#important-command-line-options)
- [Example settings and explanation](#example-settings-and-explanation)

### Important command line options
| Command Line | Description  |
| -------------| -------------|
| --help | Show usage options and exit |
| --end-usage=arg | Rate control mode (vbr, cbr(default), cq, q). VBR and CBR are self explanatory. CQ (Constrained Quality) try to follow quantizer, adjusted to fit given rate , Q is for Quality. |
| --cq-level=arg | Constant/Constrained Quality level, used in Q/CQ modes. |
| --cpu-used=arg | CPU Used (0..6) Good mode, (5..9) realtime mode (default). Default is good mode(CPU-0 to CPU-6). If realtime flag(--rt), every speed set above 6 will default back to 6. Lower numbers are slower. |
| --target-bitrate=arg | Bitrate (kbps) |
| --bit-depth=arg |  Bit depth (8, 10, 12). Default is the bit-depth recognized by aomenc from the source file. It's recommened to set 10-bit, even with 8-bit source, as it improves efficiency, 12-bit is not recommended for end compression, as it is not supported in the main AV1 HW decoding profile. |
| --tile-columns=arg | Number of tile columns to use, log2 (number to power of 2). With --tile-columns=2, will result in 4 tile columns. |
| --tile-rows=arg | Number of tile rows to use, log2  (number to power of 2). With --tile-rows=1 will result in 2 tile rows. |
| --threads=arg | Limit on allowed number of threads to use. Up to 64.|
| --lag-in-frames=arg | Number of lagged frames used by the encoder for lookahead and alternate reference frame placement(default 19, max 35).
| --enable-cdef=arg | Enable the constrained directional enhancement filter (0: false, 1: true (default)). CDEF is a filter used to clean up artifacts inflicted by encoder |
| --aq-mode=arg | Adaptive quantization mode(0: default. 1: Variance. 2: Complexity. 3: Cyclic Refresh) |
| --tune-content=arg | Tune content type (default,screen). |
| --enable-fwd-kf=arg | Enable forward reference keyframes(default=0). |
| --kf-min-dist=arg | Minimum keyframe interval in frames(default=12). |
| --kf-max-dist=arg | Maximum interval in frames at which forced keyframes will be placed(default=9999, or adaptive keyframe placement only). |
| --enable-keyframe-filtering=arg | Apply temporal filtering on key frame(0: no filter, 1: filter without overlay (default), 2: filter with overlay - experimental, may break random access in players.)|
| --arnr-maxframes=arg | Maximum number of alternate reference noise reduced frames used by the encoder(default=7). |
| --arnr-strength=arg | ARNR frames filtering strength(default=5). |
| --enable-qm=arg | Enable quantisation matrices (0: false (default), 1: true). |
| --quant-b-adapt=arg | Use adaptive quantize_b(default=0). |
| --mv-cost-upd-freq=arg | Update freq for mv costs(motion vector estimation cost calculation) (0: SB(SuperBlock), 1: SB Row per Tile, 2: Tile, 3: Off.) |
| --enable-chroma-deltaq=arg | Enable chroma delta quant (0: false (default), 1: true). May be broken below --cq-level=15. |
| --color-primaries=arg | Color primaries (CICP) of input content: bt709, unspecified, bt601, bt470m, bt470bg, smpte240, film, bt2020, xyz, smpte431, smpte432, ebu3213. |
| --transfer-characteristics=arg | Transfer characteristics (CICP) of input content(unspecified, bt709, bt470m, bt470bg, bt601, smpte240, lin, log100, log100sq10, iec61966, bt1361, srgb, bt2020-10bit, bt2020-12bit, smpte2084, hlg, smpte428. | Leave at default unless you have HDR content or your source's transfer characteristics are different; in that case, set it to whatever your content is.
| --matrix-coefficients=arg | Matrix coefficients (CICP) of input content: identity, bt709, unspecified, fcc73, bt470bg, bt601, smpte240, ycgco, bt2020ncl, bt2020cl, smpte2085, chromncl, chromcl, ictcp.|

### Example settings and explanation

##### Constant quality:

` --end-usage=q --cq-level=30 --cpu-used=4 --threads=64 `

It is recommended to set it the rate control --end-usage=q to get the highest quality rate control method possible. Only use CQ if you are planning to stream with a maximum bitrate, and CBR/VBR for livestreaming.

It is recommended to the --cq-level in range  20-40 depending on your source.

##### Target Bitrate:

`` --end-usage=vbr --target-bitrate=1000 --cpu-used=4 --threads=64 ``

To get good efficiency with VBR, it is strongly recommended to use aomenc in 2-pass mode(which is the default in av1an).

##### Tiles(tile columns and rows)
` ... --tile-columns=2 --tile-rows=1 ...`

If tiles required to improve playback on old devices or high framerates, set --tile-columns=2 and --tile-rows=1 at 1080p. For higher resolution/higher framerate  encoding, set it to --tile-columns=3 and --tile-rows=2.

#### CPU preset:
` ... --cpu-used=6 ... `

--cpu-used=6 requires ~30% less bitrate than x265 for same quality, gains by preset increments are relatively smaller than x264/x265.

#### Bit-depth:
` ... --bit-depth=10 ... `
It is recommended to set it to 10-bit even for 8-bit content for higher efficiency (due to better compression efficiency) and less banding.

#### Lag-in-frames
` ... --lag-in-frames=35 ... `

More is better, up to a limit of 35 (default is 19).


#### Content tune
` ... --tune-content=default ...`

default: tuned for most content. screen: tuned for screen recordings

#### Flags used for most native 10-bit HDR content
` ... --color-primaries=bt2020 --transfer-characteristics=smpte2084 --matrix-coefficients=bt2020ncl ... `

#### Example command line for good quality
` --end-usage=q --cq-level=22 --cpu-used=4 --threads=8 --bit-depth=10 --lag-in-frames=35 --enable-fwd-kf=1 --enable-qm=1 --enable-chroma-deltaq=1 --quant-b-adapt=1 --mv-cost-upd-freq=2 `

#### Example command line for fast speed
` --end-usage=q --cq-level=22 --cpu-used=6 --threads=64 --tile-columns=2 --tile-rows=1 --bit-depth=8 `

