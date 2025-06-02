# Scene Detection

Name | Flag | Type | Default
--- | --- | --- | ---
[Scenes](#scenes--s---scenes) | `-s`, `--scenes` | Path | 
[Scene Detection Only](#scene-detection-only---sc-only) | `--sc-only` | 
[Split Method](#split-method---split-method) | `--split-method` | `SPLIT_METHOD` | `av-scenechange`
[Scene Detection Method](#scene-detection-method---sc-method) | `--sc-method` | `SC_METHOD` | `standard`
[Scene Downscale Height](#scene-downscale-height---sc-downscale-height) | `--sc-downscale-height` | Integer | 
[Scene Pixel Format](#scene-pixel-format---sc-pix-format) | `--sc-pix-format` | `PIXEL_FORMAT` | 
[Extra Split Frames](#extra-split-frames--x---extra-split) | `-x`, `--extra-split` | Integer | 
[Extra Split Seconds](#extra-split-seconds---extra-split-sec) | `--extra-split-sec` | Integer | 10
[Minimum Scene Length](#minimum-scene-length---min-scene-len) | `--min-scene-len` | Integer | 24
[Force Keyframes](#force-keyframes---force-keyframes) | `--force-keyframes` | Integer List

## Scenes `-s`, `--scenes`

File location for scenes.

Scenes are stored as JSON.

### Examples

* `> av1an -i input.mkv -o output.mkv -s scenes.json` - Creates scenes file `./scenes.json`
* `> av1an -i input.mkv -o output.mkv --scenes C:\Av1an\scenes\1.json` - Creates scenes file `C:\Av1an\scenes\1.json`

## Scene Detection Only `--sc-only`

Run the scene detection only before exiting.

Requires a scene file with `--scenes`.

## Split Method `--split-method`

Method used to determine chunk boundaries.

"av-scenechange" uses an algorithm to analyze which frames of the video are the start of new scenes, while "none" disables scene detection entirely (and only relies on -x/--extra-split to add extra scenecuts).

### Possible Values

* `av-scenechange`
* `none`

### Default

If not specified, `av-scenechange` is used.

## Scene Detection Method `--sc-method`

Scene detection algorithm to use for av-scenechange.

### Possible Values

* `standard` - Most accurate, still reasonably fast. Uses a cost-based algorithm to determine keyframes.
* `fast` - Very fast, but less accurate. Determines keyframes based on the raw difference between pixels.

### Default

If not specified, `standard` is used.

## Scene Downscale Height `--sc-downscale-height`

Optional downscaling for scene detection.

Specify as the desired maximum height to scale to (e.g. `720` to downscale to 720p — this will leave lower resolution content untouched). Downscaling improves scene detection speed but lowers accuracy, especially when scaling to very low resolutions.

By default, no downscaling is performed.

## Scene Pixel Format `--sc-pix-format`

Perform scene detection with this pixel format.

### Possible Values

Any valid pixel format name. See [FFmpeg](https://www.ffmpeg.org/doxygen/0.11/pixfmt_8h.html#60883d4958a60b91661e97027a85072a) for a full list.

### Examples

* `> av1an -i input.mkv -o output.mkv --sc-pix-format yuv420p` - Use YUV420P for scene detection
* `> av1an -i input.mkv -o output.mkv --sc-pix-format yuv444p` - Use YUV444P for scene detection

## Extra Split Frames `-x`, `--extra-split`

Maximum scene length, in frames.

When a scenecut is found whose distance to the previous scenecut is greater than the value specified by this option, one or more extra splits (scenecuts) are added. Set this option to `0` to disable adding extra splits.

### Examples

* `> av1an -i input.mkv -o output.mkv -x 100` - Adds an extra split every 100 frames
* `> av1an -i input.mkv -o output.mkv --extra-split 240` - Adds an extra split every 240 frames
* `> av1an -i input.mkv -o output.mkv --extra-split 0` - Disables adding extra splits

## Extra Split Seconds `--extra-split-sec`

Maximum scene length, in seconds.

If both frames and seconds are specified, then the number of frames will take priority.

### Default

If not specified, `10` is used.

### Examples

* `> av1an -i input.mkv -o output.mkv --extra-split-sec 10` - Adds an extra split every 10 seconds
* `> av1an -i input.mkv -o output.mkv --extra-split-sec 5` - Adds an extra split every 5 seconds
* `> av1an -i input.mkv -o output.mkv --extra-split-sec 15 --extra-split 50` - Adds an extra split every 50 frames, ignoring `--extra-split-sec 15`

## Minimum Scene Length `--min-scene-len`

Minimum number of frames for a scenecut.

If a scene contains fewer frames than this value, it will not be cut.

### Default

If not specified, `24` is used.

### Examples

* `> av1an -i input.mkv -o output.mkv --min-scene-len 60` - Adds an extra split every 60 frames

## Force Keyframes `--force-keyframes`

List of frames to force as keyframes.

### Possible Values

A comma-separated list of frame numbers as positive integers.

### Examples

* `> av1an -i input.mkv -o output.mkv --force-keyframes 82,346,622` - Force frames 82, 346, and 622 as keyframes
