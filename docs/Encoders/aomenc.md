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
| --cpu-used=arg | CPU Used (0..6) Good mode, (5..9) realtime mode 1(default)|
| --cq-level=arg | Constant/Constrained Quality level, used in Q/CQ modes |
| --target-bitrate=arg | Bitrate (kbps) |
| --bit-depth=arg |  Bit depth (8, 10, 12) |
| --tile-columns=arg | Number of tile columns to use, log2 (number to power of 2). Therefore, if you set the number to 2, it'll do 2²= 4 tile columns. |
| --tile-rows=arg | Number of tile rows to use, log2  (number to power of 2). Therefore, if you set the number to 1, it'll do 2¹= 2 tile rows. |
| --threads=arg | Allowed number of threads to use. It is recommended to set to however many cores you have for chunked encoding, and how many threads you have for single instance encoding.|
| --lag-in-frames=arg | Number of lagged frames used by the encoder for lookahead and alternate reference frame placement: more is better, but makes the encoder slower, up to a limit of 35 (default is 19). |
| --aq-mode=arg | Adaptive quantization mode (0: Default. 1: Variance, in which the encoder lowers the quantizer value for flat low complexity blocks, like clouds or dark scenes. Not recommended unless you know what you are doing. 2: Complexity, in which the encoder lowers the quantizer for complex parts of the image. 3: Cyclic refresh. Ups the quantizer value as much as possible for static flat looking blocks, only doing block refreshes in cycles if needed. Not recommend unless when used for live-streaming or conferencing.) |
| --tune-content=arg | Tune content type (default: tuned for most content. screen: tuned for screen recordings, low complexity animations, and most videos games. Not recommended for complex animation like anime as it disables some post-processing making it look worse.) |
| --enable-fwd-kf=arg | Enable forward reference keyframes(default=0). Makes the encoder more efficient, but a bit slower. It's recommended to enable it by setting it to 1 . |
| --kf-min-dist=arg | Minimum keyframe interval in frames(default=12). It is recommended to leave it to default values |
| --kf-max-dist=arg | Maximum keyframe interval in frames(default=9999, or adaptive keyframe placement only). It is recommended to set it to a MAX of 10s worth of frames, or 240 for 24FPS, 300 for 30FPS, and 600 for 60FPS for easier seeking. For video game content or >=60FPS content, is is also possible to use a max of 5s worth of frames to help with seeking performance. Leave it to default if you want maximum efficiency. |
| --enable-keyframe-filtering=arg | Apply temporal filtering on key frame(0: no filter, 1: filter without overlay (default), 2: filter with overlay - experimental, may break random access in players.). It is recommended to leave it at default unless you really know what you're doing. |
| --arnr-maxframes=arg | Maximum number of alternate reference noise reduced frames used by the encoder(default=7). It is recommended to leave it at default unless you want to crank up the efficiency for low motion scenes. |
| --arnr-strength=arg | ARNR frames filtering strength. It is recommended to leave it at default. |
| --enable-qm=arg | Enable quantisation matrices (0: false (default), 1: true). It is recommended to turn the setting on by setting it to 1 for higher efficiency at a slight loss of speed. |
| --quant-b-adapt=arg | Use adaptive quantize_b(default=0). This setting enables adaptive quanzation for reference frames. It is recommended to turn the setting on by setting it to 1 for higher efficiency at a slight loss of speed. |
| --mv-cost-upd-freq=arg | Update freq for mv costs(motion vector estimation cost calculation) 0: SB(SuperBlock), 1: SB Row per Tile, 2: Tile, 3: Off. It is recommended to set it to 2 for higher efficiency at a slight loss of speed. |
| --enable-chroma-deltaq=arg | Enable chroma delta quant (0: false (default), 1: true). It is recommened to turn the setting on by setting to 1 for higher efficiency at a slight loss of speed. It seems to be currently broken below CQ15, so use it at your own risk if you have a minimum Q(min_q) below 15.
| --color-primaries=arg | Color primaries (CICP) of input content: bt709, unspecified, bt601, bt470m, bt470bg, smpte240, film, bt2020, xyz, smpte431, smpte432, ebu3213. Leave at default unless you have HDR content or your source's color-primaries information is different; in that case, set it to whatever your content is, usually BT2020.
| --transfer-characteristics=arg | Transfer characteristics (CICP) of input content(unspecified, bt709, bt470m, bt470bg, bt601, smpte240, lin, log100, log100sq10, iec61966, bt1361, srgb, bt2020-10bit, bt2020-12bit, smpte2084, hlg, smpte428. Leave at default unless you have HDR content or your source's transfer characteristics are different; in that case, set it to whatever your content is.
| --matrix-coefficients=arg | Matrix coefficients (CICP) of input content: identity, bt709, unspecified, fcc73, bt470bg, bt601, smpte240, ycgco, bt2020ncl, bt2020cl, smpte2085, chromncl, chromcl, ictcp. Leave at default unless you have HDR content or your source's matrix coefficients information is different; in that case, set it to whatever your content is.

### Examples of settings

##### Constant quality:

` --end-usage=q --cq-level=30 --cpu-used=4 --threads=16 `

##### Target Bitrate:

`` --end-usage=vbr --target-bitrate=1000 --cpu-used=4 --threads=16 ``


##### Tiles:
` ... --tile-columns=2 --tile-rows=1 ...`
