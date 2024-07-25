# Target Quality

## Table of Contents

1. [Introduction](#introduction)
2. [Prerequisites](#prerequisites)
3. [Parameters](#parameters)
4. [Usage examples](#usage-examples)
5. [Considerations when Cropping](#considerations-when-cropping)

## Overview

Av1an's Target Quality feature has a really simple goal; instead of having the user guess what the appropriate CQ/CRF value is to achieve their desired video quality, we simply set a VMAF score 'target' that we wish to achieve and let av1an automatically determine the appropriate CRF/CQ values per segment through testing. This approach offers a multitude of benefits:

- It ensures a better level of visual consistency than regular rate controls.
- It allocates enough bitrate to complex segments to match target quality, while similarly saving bitrate by not overspending on simpler scenes.
- Unlike CRF, av1an's Target Quality feature a unified scale/interface for setting encoding quality across all encoders.

However, using Target Quality also greatly increases total-encoding time, as each segement will be re-tested until the the appropriate CRF/CQ value is found. The time that's usually taken to manually determine these values is instead simply automated and standarized.

## Prerequisites

 - A build of `ffmpeg` compiled with `libvmaf` (included by default configuration since 2020)
 - Pre-installed or or otherwise manually selected VMAF models
   - The default model used by ffmpeg is `/usr/share/model/vmaf_v0.6.1.pkl`
   - Windows-user are likely to have to manually specify their VMAF-model. See [this](https://github.com/Netflix/vmaf/blob/master/resource/doc/ffmpeg.md#note-about-the-model-path-on-windows) for using Target Quality / VMAF on Windows.
 - An encoder with Constant Quality (CQ) / Constant Rate Control (CRF) support (e.g. `--crf 30`, `--cq-level=30`). These values will then be tweaked for each scene/segment until the desired score is achieved.

## Parameters

- `--target-quality <float>` - Enables target quality with default settings for your encoder. Targets the `<float>` VMAF score (0-100, higher = better)

- `--probes <int>` - Overrides maximum amount of probes to make for each segment (Default: `4`)

- `--min_q <int> --max_q <int>` - Overrides default CRF/CQ boundaries for search

- `--vmaf-res <resolution>` - Overrides default VMAF calculation resolution for video (Default: `1920x1080`)

## Usage examples

Run av1an with default settings (aomenc), targetting a VMAF-score of 90:
```bash
$ av1an -i file --target-quality 90
```

Target a VMAF score of 95 using a custom VMAF-model and probe-count:
```bash
av1an -i file --target-quality 95 --vmaf_path "vmaf_v.0.6.3.pkl" --probes 6
```

## Considerations when Cropping

When cropping video during Av1an's encoding process using ffmpeg filters, one should additionally pass these filters to the VMAF process, as well as the target-resolution: 

```bash
$ av1an -i input.mkv --ffmpeg "-vf crop=3840:1900:0:0" --vmaf-filter "crop=3840:1900:0:0" --vmaf-res "3840x1900" --target-quality 90
```

If cropping is performed through a VaporSynth script, the user need only set `--vmaf-res` to the appropriate output resolution:

```bash
$ av1an -i 4k_crop.vpy --vmaf-res "3840x1600" --target-quality 90
```
