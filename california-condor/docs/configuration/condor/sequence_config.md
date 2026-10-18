# condor.sequence_config

`condor.sequence_config` in `condor.json`. See [condor](./index.md), [top level](../index.md). Each sub-object maps to a pipeline step in [guide](../../guide.md).

| Field | Type | Required | Default on `init` | Document |
| ----- | ---- | -------- | ----------------- | -------- |
| `scene_detector` | Object | Yes | AVSceneChange standard, min 1s frames, max 10s frames | [scene-detector](./sequence-config/scene-detector.md) |
| `zoner` | Object or null | Yes | `null` | [zoner](./sequence-config/zoner.md) |
| `noise_detector` | Object or null | Yes | `null` | [noise-detector](./sequence-config/noise-detector.md) |
| `noise_scaler` | Object or null | Yes | `null` | [noise-scaler](./sequence-config/noise-scaler.md) |
| `benchmarker` | Object | Yes | `{ "threshold": 5, "max_memory": null }` | [benchmarker](./sequence-config/benchmarker.md) |
| `parallel_encoder` | Object | Yes | workers `null`, buffer `{"Workers": 1}`, scenes dir `<temp>/scenes` | [parallel-encoder](./sequence-config/parallel-encoder.md) |
| `scene_concatenator` | Object | Yes | `mkvmerge`, scenes dir `<temp>/scenes` | [scene-concatenator](./sequence-config/scene-concatenator.md) |
| `target_quality` | Object or null | Yes | `null` | [target-quality](./sequence-config/target-quality.md) |
| `quality_check` | Object or null | Yes | `null` | [quality-check](./sequence-config/quality-check.md) |
| `bitrate_optimizer` | Object | Yes | `{ "bitrate_sigma_threshold": null }` | [bitrate-optimizer](./sequence-config/bitrate-optimizer.md) |
| `speed_scaler` | Object | Yes | `{ "speed_quantizers": [] }` | [speed-scaler](./sequence-config/speed-scaler.md) |

Example (init defaults):

```json
{
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
```
