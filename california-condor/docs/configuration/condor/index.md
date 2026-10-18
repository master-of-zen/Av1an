# condor Object

Sequence state stored in `condor.json` under `condor`. See [top level](../index.md), [guide](../../guide.md).

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `input` | Object | Yes | See [input](./input.md) |
| `output` | Object | Yes | See [output](./output.md) |
| `encoder` | Object | Yes | See [encoder](./encoder.md) |
| `scenes` | Array | Yes | See [scene](./scene.md). `[]` on `init`. |
| `sequence_config` | Object | Yes | See [sequence-config](./sequence-config/index.md) |

Example:

```json
{
    "input": { "Video": { "path": "input.mp4", "import_method": { "FFMS2": { "index": null } } } },
    "output": { "path": "output.mkv", "tags": {}, "video_tags": {} },
    "encoder": { "SVTAV1": { "executable": null, "pass": { "All": 1 }, "options": {}, "photon_noise": null } },
    "scenes": [],
    "sequence_config": {
        "scene_detector": { "method": { "AVSceneChange": { "minimum_length": 24, "maximum_length": 240, "method": "Standard" } }, "input": null },
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
```
