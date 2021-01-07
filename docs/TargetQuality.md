 # Target Quality

## Table of Contents
1. [Description](#Description)
2. [Requirements](#Requirements)
3. [Commands](#Commands)
4. [Example of usage](#Example-of-usage)

## Description

Target Quality has a really simple goal, instead of guessing what the CQ/CRF value to choose for desired level of video quality we set quality level we wan, quality goal is set in value of VMAF score we want to achieve and let the algorithm find CRF/CQ value that will result in that score, for each segment. Which simultaneously achieve 3 things, if compared to usual, single value CRF/CQ encode.

- Ensuring better level of visual consistency than default rate controls
- Give enough bitrate to complex segments to match target quality.
- Save bitrate by not overspending on scenes, which saves bit rate.

## Requirements

- Working VMAF setup
    - FFMPEG with libvmaf (It's de facto default configuration from 2020)
    - Installed or manually selected VMAF models
        - by default it grabs /usr/share/model/vmaf_v0.6.1.pkl

- Supported encoder
    - aomenc
    - rav1e
    - svt-av1
    - x265
    - x264
    - vpx

- Quality/Constant Rate control (Target quality change crf/cq value for each segment). Which means that encoders must be in mode that use CRF/CQ and have those options specified ( `--crf 30`, `--cq-level=30`) those values get replaced for each segment

## Commands

- `--target_quality FLOAT` - enables target quality with default settings for that encoder, targets FLOAT value

- `--probes INT` - Overrides maximum amount of probes to make for each segment (Default 4)

- `--min_q INT --max_q INT` - Overrides default CRF/CQ boundaries for search

## Example of usage

`av1an -i file --target_quality 90` - Will run aomenc with default settings of target_quality

`av1an -i file --target_quality 95 --vmaf_path "vmaf_v.0.6.3.pkl" --probes 6 ` - With specified path to vmaf model and 6 probes per segment