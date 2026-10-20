# sequence_config.scene_concatenator

`sequence_config.scene_concatenator` in `condor.json`. See [sequence_config](../sequence_config.md). Updated by [`concatenate`](../../../commands/concatenate.md). CLI types in [Concatenation](../../../types/concatenation.md).

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `method` | Object or String | Yes | `{ "mkvmerge": {} }` | The method and its output settings: `{ "mkvmerge": {...} }`, `{ "ffmpeg": {...} }`, or `"ivf"`. CLI `--concat` / `concatenate --method` |
| `scenes_directory` | String (path) | Yes | `<temp>/scenes` | Directory of per-scene encodes |
| `output` | String (path) or null | Yes | `null` | Override output path |

Each `method` payload holds that muxer's settings: a `mkvmerge` or `ffmpeg` object with its track/chapter/metadata options, or the plain string `"ivf"` (video only).

Example:

```json
{
    "method": {
        "mkvmerge": {
            "tracks": {
                "0": { "type": "video", "color_primaries": 9, "max_content_light": 1000 },
                "1": { "type": "audio", "language": "eng", "default_track": true },
                "2": { "type": "audio", "copy": false }
            },
            "chapters": { "copy": true, "language": "eng" },
            "metadata": { "copy": true, "title": "My Movie", "tags": { "GENRE": "Animation" } },
            "extra_inputs": []
        }
    },
    "scenes_directory": "./a1b2c3d4/scenes",
    "output": null
}
```

## `mkvmerge`

Track maps are keyed by the track ID reported by `mkvmerge --identify` for that source file. `None` copies every track.

Each value is one of `video`, `audio`, `subtitle`, or `attachment`, tagged with `"type"`. Every entry has a `copy` flag that defaults to `true`; setting it to `false` drops the track.

| Field | Applies to | mkvmerge option |
| ----- | ---------- | --------------- |
| `copy` | all | reversed `--video-tracks` / `--audio-tracks` / `--subtitle-tracks` / `--attachments` list |
| `name` | video/audio/subtitle | `--track-name` |
| `language` | video/audio/subtitle | `--language` |
| `delay` | video/audio/subtitle | `--sync` (milliseconds) |
| `default_track`, `forced`, `enabled`, `hearing_impaired`, `visual_impaired`, `text_descriptions`, `original`, `commentary` | video/audio/subtitle | the matching `--*-flag` |
| `crop` (`{left,top,right,bottom}`), `display_dimensions` (`{width,height}`), `aspect_ratio`, `aspect_ratio_factor`, `color_primaries`, `transfer_characteristics`, `matrix_coefficients`, `color_range`, `color_bits_per_channel`, `chroma_subsample`, `chroma_siting`, `max_content_light`, `max_frame_light`, `max_luminance`, `min_luminance`, `chromaticity_coordinates`, `white_color_coordinates`, `stereo_mode`, `field_order` | video | the matching video option |
| `aac_is_sbr`, `reduce_to_core` | audio | `--aac-is-sbr`, `--reduce-to-core` |
| `compression` (`none`/`zlib`), `charset` | subtitle | `--compression`, `--sub-charset` |
| `path`, `attachment_name`, `attachment_description`, `attachment_mime_type` | attachment | `--attach-file` with `--attachment-name` / `--attachment-description` / `--attachment-mime-type` |

The `video` entry addresses the encoded video track (the concatenated scenes, track `0`); any other ID is rejected. An attachment entry only renames/sets a MIME type when it has a `path`; mkvmerge cannot modify an attachment copied from a source.

`chapters` and `metadata` are `null` by default, which copies the source's chapters and global tags. Set `"copy": false` to drop them, or `title`/`tags` to write new global metadata. `extra_inputs` is a list of `{ "path", "tracks", "chapters", "metadata" }` objects merged alongside the encoded scenes.

## `ffmpeg`

Track maps are keyed by the stream index within that input file. `None` copies every audio, subtitle, and attachment/data stream. An empty payload (`{ "ffmpeg": {} }`) adds the original input and copies all of its audio, subtitle, and attachment/data streams.

Because an absent stream is not mapped, a track map replaces the whole selection: list every stream to keep, not only the ones to change. `--discard` fills the map from the probe for this reason. Data streams have no settings of their own and are always mapped.

Each entry is tagged with `"type"`: `video`, `audio`, or `subtitle` (each with the fields below), or `attachment`.

| Field | Description |
| ----- | ----------- |
| `copy` | Copy the stream. Defaults to `true` |
| `codec` | Output codec (`-c:a:N`, `-c:s:N`, `-c:v:N`). `copy` keeps the source codec |
| `filter` | Filtergraph for audio (`-filter:a:N`) or subtitles (`-filter:s:N`). Rejected for video and requires a non-`copy` codec |
| `parameters` | Extra FFmpeg arguments, keys prefixed with `-` |

An `attachment` entry instead takes `path` (a file attached with `-attach`) plus `attachment_name`, `attachment_description`, and `attachment_mime_type` (required when `path` is set). Without a `path` it selects an existing attachment stream to copy.

Keys that would fight Condor's own arguments are rejected: codec and mapping options (`-c`, `-map`, `-codec:a`, ...), filter options (`-af`, `-filter:*`, `-filter_complex`), input/output control (`-i`, `-f`, `-y`, ...), and stream selection (`-vn`, `-an`, `-sn`, `-dn`). Use the `codec`/`filter` fields instead.

`chapters` and `metadata` are `null` by default, which copies the source's chapters and global tags. Set `"copy": false` to pass `-map_chapters -1` / `-map_metadata -1`. Unlike mkvmerge there is no chapter `language` or `charset`, so setting either only produces a warning.

Example of filtering a second audio track:

```json
{
    "method": {
        "ffmpeg": {
            "tracks": {
                "1": { "type": "audio", "codec": "aac", "filter": "loudnorm=I=-16:TP=-1.5:LRA=11" },
                "2": { "type": "audio", "copy": false }
            },
            "metadata": { "copy": true }
        }
    }
}
```
