# sequence_config.zoner

`sequence_config.zoner` in `condor.json`. See [sequence_config](../sequence_config.md). Created by [`zone`](../../../commands/zone.md) or [`condor --zones`](../../../commands/condor.md#zones---zones) (`null` on `init`), and applied by [`condor`](../../../commands/condor.md) right after Scene Detection.

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `zones` | Array | No | `[]` | The zone overrides, applied in start-frame order. Each item is a [Zone](../../../types/zone.md) |

CLI mapping: `zone ZONE...` and `condor --zones ZONE...`, where each value is a zone definition or a path to a JSON, JSON lines, or text zone file. Values replace the zones already configured.

## Implementation structure

Zoner belongs entirely to `california-condor`; it does not add a sequence or zone configuration to `andean-condor`.

- `src/zoner/zone.rs` owns the zone model, encoder overrides, and scene splitting.
- `src/zoner/parse.rs` reads inline definitions and JSON, JSON lines, or text files, reusing the encoder parameter parser.
- `src/zoner/mod.rs` owns `ZonerConfig`, `ZonerData`, range validation, ordered application, and the `Sequence` adapter. The configured zone vector is the source of truth; the only state the sequence keeps is the record it writes back onto each scene's `sequence_data.zoner.zoned`.
- The command handler loads definitions into `CliSequenceConfig.zoner` in memory; the sequence saves only after successful validation/application, and records the pass on every scene. While that record stands, later runs skip until [`condor scenes reset zoner`](../../../commands/scenes.md) clears it. `zone --dry-run` uses the same sequence with a no-op save callback and disposable state. The pipeline applies them immediately after Scene Detector and before noise processing. All CLI sequence runners share one configuration-save callback.
- `andean-condor` only supplies generic encoder accessors and equality derives, not zoning policy.

Zoner only warns about stale saved results and encode/probe files, including later indices shifted by splits; it never removes or invalidates them automatically.

Individual overrides are idempotent, but scene splitting can change later zone intersections. Re-detect scenes, or clear the zoning record with `condor scenes reset zoner`, before reapplying interacting zones if the original scene layout is required.

Example:

```json
{
    "zones": [
        {
            "start_frame": null,
            "end_frame": 200,
            "maximum_frames": 240,
            "encoder": {
                "Merge": {
                    "base": "SVTAV1",
                    "options": { "Merge": "--preset 3 --crf 23" },
                    "photon_noise": { "Merge": { "iso": 400 } }
                }
            }
        }
    ]
}
```
