# VMAF

Name | Flag | Type | Default
--- | --- | --- | ---
[VMAF](#vmaf---vmaf) | `--vmaf` || 
[VMAF Path](#vmaf-path---vmaf-path) | `--vmaf-path` | String | 
[VMAF Resolution](#vmaf-resolution---vmaf-res) | `--vmaf-res` | String | `1920x1080`
[VMAF Threads](#vmaf-threads---vmaf-threads) | `--vmaf-threads` | Integer | 
[VMAF Filter](#vmaf-filter---vmaf-filter) | `--vmaf-filter` | String | 


## VMAF `--vmaf`

Plot an SVG of the [VMAF](https://github.com/Netflix/vmaf) for the encode.

This option is independent of [Target Quality](./target_quality.md) (`--target-quality`), i.e. it can be used with or without it. The SVG plot is created in the same directory as the [Output](./general.md#output--o) file.

## VMAF Path `--vmaf-path`

Path to VMAF model.

This option is also used by [Target Quality](./target_quality.md) (`--target-quality`).

### Default

If not specified, FFmpeg's default is used.

## VMAF Resolution `--vmaf-res`

Resolution used for VMAF calculation.

If set to the input resolution, the output video will be scaled to the resolution of the input video.

### Default

If not specified, `1920x1080` is used.

## VMAF Threads `--vmaf-threads`

Number of threads to use for [Target Quality](./target_quality.md) (`--target-quality`) VMAF calculation.

## VMAF Filter `--vmaf-filter`

Filter applied to source at VMAF calcualation.

This option should be specified if the source is cropped, for example.
