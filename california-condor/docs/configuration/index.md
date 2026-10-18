# condor.json

Top-level `condor.json` schema reference. Created by [`init`](../commands/init.md), managed via [configuration.md](../configuration.md), consumed by every step in [guide](../guide.md). For the nested `condor` object see [condor](./condor/index.md).

## Fields

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `$schema` | String (URL) | No | Release `configuration.schema.json` URL | JSON Schema URL for IDE validation (see [configuration.md](../configuration.md#external-validation)) |
| `input` | String (path) | Yes | — | Original input path. Duplicated in case `condor.input` instantiates a VapourSynth script input |
| `temp` | String (path) | Yes | — | Temporary directory. `init` defaults to `<cwd>/<hash-of-input-name>`; override with `--temp` |
| `condor` | Object | Yes | — | Sequence state. See [condor](./condor/index.md) |

## Input Filters

Filters live on the input they modify, in `condor.input.filters` and each per-step
`condor.sequence_config.*.input.filters`. There is no top-level filter list.

CLI strings use the syntax documented in [Filters](../types/filters.md) (e.g. `"resize:scaler=bilinear;width=1920;height=1080;format=yuv420p10le"`, `"crop:top=140;bottom=140;"`, `"trim:start=24;end=240;"`). Each string is parsed (`FromStr`) into one object in the array. Full object shapes for every variant (`Crop`, `Resize`, `Trim`, `Rescale`, `WNNM`, `Bilateral`, `Degrain`, `ZoomDegrain`, `FGS`) are listed in [Filters](../types/filters.md#json-shape).

A native `--decoder ffms2` input stores `Ffms2Filter` objects instead, since it cannot run
VapourSynth filters. See [Filters](../types/filters.md#ffms2-filters).

Default on `init` for a VapourSynth input (one resize to 10-bit):

```json
[
    { "Resize": { "scaler": "Bicubic", "width": null, "height": null, "format": "YUV420P10LE" } }
]
```

Default on `init` for a native `--decoder ffms2` input:

```json
[
    { "OutputFormat": { "bit_depth": 10, "chroma": null, "width": null, "height": null } }
]
```

## Defaults on Init

- Encoder: `svt-av1` with defaults in [Encoder](../types/encoder.md#default-parameters)
- Input filter: `resize:scaler=bicubic;format=yuv420p10le`
- Scene detector: `standard`, 1s minimum, 10s maximum
- `scenes`: `[]`
- `zoner`, `noise_detector`, `noise_scaler`, `target_quality`, `quality_check`: `null`

## Minimal Example

```json
{
    "$schema": "https://github.com/rust-av/Av1an/releases/download/latest/configuration.schema.json",
    "input": "input.mp4",
    "temp": "./a1b2c3d4",
    "condor": {
        "input": {
            "Video": {
                "path": "input.mp4",
                "import_method": { "FFMS2": { "index": null } },
                "filters": [
                    { "OutputFormat": { "bit_depth": 10, "chroma": null, "width": null, "height": null } }
                ]
            }
        },
        "output": { "path": "output.mkv", "tags": {}, "video_tags": {} },
        "encoder": {
            "SVTAV1": {
                "executable": null,
                "pass": { "All": 1 },
                "options": {
                    "preset": { "Number": { "prefix": "--", "delimiter": " ", "value": 4.0 } },
                    "crf": { "Number": { "prefix": "--", "delimiter": " ", "value": 25.0 } }
                },
                "photon_noise": null
            }
        },
        "scenes": [],
        "sequence_config": {
            "scene_detector": {
                "method": { "AVSceneChange": { "minimum_length": 24, "maximum_length": 240, "method": "Standard" } },
                "input": null
            },
            "zoner": null,
            "noise_detector": null,
            "noise_scaler": null,
            "benchmarker": { "threshold": 5, "max_memory": null },
            "parallel_encoder": { "workers": null, "scenes_directory": "./a1b2c3d4/scenes", "input": null },
            "scene_concatenator": { "method": { "mkvmerge": {} }, "scenes_directory": "./a1b2c3d4/scenes", "output": null },
            "target_quality": null,
            "quality_check": null,
            "bitrate_optimizer": { "bitrate_sigma_threshold": null },
            "speed_scaler": { "speed_quantizers": [] }
        }
    }
}
```

Regenerate the authoritative schema with `cargo run -p california-condor --bin generate-schema`.
