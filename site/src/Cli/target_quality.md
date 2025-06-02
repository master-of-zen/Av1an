# Target Quality

Name | Flag | Type | Default
--- | --- | --- | ---
[Target Quality](#target-quality---target-quality) | `--target-quality` | Float | 
[Probes](#probes---probes) | `--probes` | Integer | `4`
[Probing Rate](#probing-rate---probing-rate) | `--probing-rate` | Integer | `1`
[Probing Speed](#probing-speed---probing-speed) | `--probing-speed` | `PROBING_SPEED` |
[Probe Slow](#probe-slow---probe-slow) | `--probe-slow` || 
[Minimum Quantizer](#minimum-quantizer---min-q) | `--min-q` | Integer | Based on Encoder
[Maximum Quantizer](#maximum-quantizer---max-q) | `--max-q` | Integer | Based on Encoder


## Target Quality `--target-quality`

Target a [VMAF](https://github.com/Netflix/vmaf) score for encoding.

For each chunk, Target Quality searches for the quantizer/crf needed to achieve a certain VMAF score. Target Quality mode is much slower than normal encoding, but can improve the consistency of quality in some cases.

The VMAF score range is `0`-`100` where `0` is the worst quality and `100` is the best.

### Possible Values

Any float value between `0` and `100`.

### Examples

* `> av1an -i input.mkv -o output.mkv --target-quality 80` - Target a VMAF score of 80
* `> av1an -i input.mkv -o output.mkv --target-quality 90.5` - Target a VMAF score of 90.5

## Probes `--probes`

Maximum number of probes allowed for Target Quality.

### Possible Values

Can be any positive integer.

### Default

If not specified, `4` is used.

## Probing Rate `--probing-rate`

Framerate for probes.

### Possible Values

Can be any integer from `1` to `4`.

### Default

If not specified, `1` is used.

## Probing Speed `--probing-speed`

Speed for probes.

If used with `--probe-slow`, it overrides the respective speed parameter (eg. `--cpu-used`, `--preset`, etc.)

### Possible Values

Can be any of the following:

* `veryslow`
* `slow`
* `medium`
* `fast`
* `veryfast`

### Default

If not specified, `veryfast` is used unless `--probe-slow` is specified.

## Probe Slow `--probe-slow`

Use encoding settings for probes specified by `--video-params` rather than faster, less accurate settings.

Note that this always performs encoding in one-pass mode, regardless of `--passes`.

## Minimum Quantizer `--min-q`

Lower bound for Target Quality Quantizer-search early exit.

If the minimum quantizer is tested and the probe's VMAF score is lower than the Target Quality (`--target-quality`), the Quantizer-search exits early and the minimum quantizer is used for the chunk.

### Possible Values

Depends on the encoder.

### Default

If not specified, the default value is used (chosen per encoder).

## Maximum Quantizer `--max-q`

Upper bound for Target Quality Quantizer-search early exit.

If the maximum quantizer is tested and the probe's VMAF score is higher than the Target Quality (`--target-quality`), the Quantizer-search exits early and the maximum quantizer is used for the chunk.

### Possible Values

Depends on the encoder.

### Default

If not specified, the default value is used (chosen per encoder).