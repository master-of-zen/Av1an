# condor.input

`condor.input` in `condor.json`. See [condor](./index.md), [top level](../index.md), CLI decoders in [Decoder](../../types/decoder.md), VS args in [VS Args](../../types/vs-args.md).

Externally-tagged enum. Exactly one key per object.

## Variants

| Variant | JSON key | Fields |
| ------- | -------- | ------ |
| Video file | `Video` | `path`, `import_method`, `filters` |
| VapourSynth source filter | `VapourSynth` | `path`, `import_method`, `cache_path`, `filters` |
| VapourSynth script | `VapourSynthScript` | `source`, `variables`, `index`, `filters`, `stream_concurrently` |

`init` writes `VapourSynth` with `BestSource` for video inputs, `VapourSynthScript` with `Path` source for `.vpy`/`.py` inputs.

`filters` is applied to the clip the input decodes, before any consumer sees it.
`Video` accepts only [`Ffms2Filter`](../../types/filters.md#ffms2-filters), because a
natively-decoded input cannot run VapourSynth filters. The two VapourSynth variants
accept the full [`VapourSynthFilter`](../../types/filters.md) set.

## Video

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `path` | String (path) | Yes | — | Input video path |
| `import_method` | Object | Yes | — | One key: `FFMS2` |
| `filters` | Array of `Ffms2Filter` | Yes | See [top level](../index.md#input-filters) | Conversion applied to the decoded frames |

`FFMS2` object:

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `index` | Integer or null | No | `null` | Track index |
| `cache_path` | String (path) or null | No | `null` | Where to read or write the `.ffindex` cache. `null` uses `<input>.ffindex` next to the source |

Example:

```json
{
    "Video": {
        "path": "input.mp4",
        "import_method": { "FFMS2": { "index": null, "cache_path": null } },
        "filters": [{ "OutputFormat": { "bit_depth": 10, "chroma": null, "width": null, "height": null } }]
    }
}
```

## VapourSynth

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `path` | String (path) | Yes | — | Input video path |
| `import_method` | Object | Yes | — | One key: `LSMASHWorks`, `DGDecNV`, `FFMS2`, `BestSource` |
| `cache_path` | String (path) or null | No | `null` | Index cache path |
| `filters` | Array of `VapourSynthFilter` | Yes | See [top level](../index.md#input-filters) | Filters chained onto the source node |

Import methods (all take optional fields; omitted fields are `null`):

- `LSMASHWorks`: `{ "index": null }`
- `DGDecNV`: `{ "dgindexnv_executable": null }`
- `FFMS2`: `{ "index": null }`
- `BestSource`: `{ "index": null }` (default on `init`)

Example:

```json
{
    "VapourSynth": {
        "path": "input.mp4",
        "import_method": { "BestSource": { "index": null } },
        "cache_path": null,
        "filters": [
            { "Resize": { "scaler": "Bicubic", "width": null, "height": null, "format": "YUV420P10LE" } }
        ]
    }
}
```

CLI mapping: `--decoder bestsource|vs-ffms2|lsmash|dgdecnv|ffms2` selects the method. See [Decoder](../../types/decoder.md).

## VapourSynthScript

| Field | Type | Required | Default | Description |
| ----- | ---- | -------- | ------- | ----------- |
| `source` | Object | Yes | — | One key: `Path` (string path) or `Text` (script source) |
| `variables` | Object (string→string) | Yes | `{}` | `--vs-args key=value` pairs. See [VS Args](../../types/vs-args.md) |
| `index` | Integer | Yes | `0` | Output node index |
| `filters` | Array of `VapourSynthFilter` | Yes | `[]` | Filters chained onto the script's output node |
| `stream_concurrently` | Boolean | No | `true` | Let each encoder pull frames from the script at once, bounding memory use |

A config written before `stream_concurrently` existed omits it and behaves as `true`.

With concurrent streaming on, each encoder reads its own frames from the script and only a fixed
window of decoded frames is kept in memory, so memory use stays flat no matter how long a scene is.
Turning it off decodes a whole scene ahead of its encoders instead, which uses more memory but can
decode faster. Set it to `false` if the script's source plugin decodes slowly when several readers
pull frames from it at once.

Examples:

```json
{
    "VapourSynthScript": {
        "source": { "Path": "script.vpy" },
        "variables": { "message": "fluffy kittens" },
        "index": 0,
        "filters": [],
        "stream_concurrently": true
    }
}
```

```json
{
    "VapourSynthScript": {
        "source": { "Text": "import vapoursynth as vs\ncore = vs.core\nclip = core.bs.VideoSource(source='input.mp4')\nclip.set_output()" },
        "variables": {},
        "index": 0,
        "filters": [{ "Trim": { "start": 24, "end": null } }],
        "stream_concurrently": false
    }
}
```

A script-only filter such as `Rescale` cannot be chained onto a script's output node,
because the node has no name this code can reference. Put it in the script instead.
