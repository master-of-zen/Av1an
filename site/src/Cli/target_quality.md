# Target Quality

Name | Flag | Type | Default
--- | --- | --- | ---
[Target Quality](#target-quality---target-quality) | `--target-quality` | Float | 
[Probes](#probes---probes) | `--probes` | Integer | `4`
[Probing Rate](#probing-rate---probing-rate) | `--probing-rate` | Integer | `1`
[Probing Speed](#probing-speed---probing-speed) | `--probing-speed` | `PROBING_SPEED` |
[Probing Statistic](#probing-statistic---probing-statistic) | `--probing-statistic` | String | `percentile-1`
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

## Probing Statistic `--probing-statistic`

Statistical method for calculating target quality from sorted probe results.

### Possible Values

Can be any of the following:

* `mean` - Arithmetic mean (average)
* `median` - Middle value
* `harmonic` - Harmonic mean (emphasizes lower scores)
* `percentile-<FLOAT>` - Percentile of a specified `<FLOAT>` value, where `<FLOAT>` is a value between 0.0 and 100.0
* `standard-deviation-<FLOAT>` - Standard deviation distance from mean (σ) clamped by the minimum and maximum probe scores of a specified `<FLOAT>` value, where `<FLOAT>` can be a positive or negative value
* `mode` - Most common integer-rounded value
* `minimum` - Lowest value
* `maximum` - Highest value

### Default

If not specified, `percentile-1` is used.

### Examples

* `> av1an -i input.mkv -o output.mkv --target-quality 80 --probing-statistic mean` - Target a VMAF score of 80 using the mean statistic
* `> av1an -i input.mkv -o output.mkv --target-quality 95 --probing-statistic percentile-25` - Target a VMAF score of 95 using the 25th percentile statistic
* `> av1an -i input.mkv -o output.mkv --target-quality 90 --probing-statistic standard-deviation--0.8` - Target a VMAF score of 90 using the value that is 0.8 standard deviations below the mean
* `> av1an -i input.mkv -o output.mkv --target-quality 75 --probing-statistic standard-deviation-2` - Target a VMAF score of 75 using the value that is 2 standard deviations above the mean.

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