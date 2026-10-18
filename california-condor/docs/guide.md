# Guide

End-to-end workflow for `condor`. For CLI overview see [README](../README.md). For global flags and JSON configuration validation see [configuration](./configuration.md).

## Pipeline Order

| Step | Command                              | TUI | Purpose                                              |
| ---- | ------------------------------------ | --- | ---------------------------------------------------- |
| 1    | [init](./commands/init.md)                    | No  | Create `condor.json` from `INPUT OUTPUT`             |
| 2    | [detect-scenes](./commands/detect-scenes.md)  | Yes | Split video into scenes                              |
| 3    | [zone](./commands/zone.md)                    | No  | Override the scenes inside frame ranges              |
| 4    | [detect-noise](./commands/detect-noise.md)    | Yes | Measure noise/grain per scene                        |
| 5    | [scale-noise](./commands/scale-noise.md)      | No  | Scale Photon Noise ISO from noise results            |
| 6    | [benchmark](./commands/benchmark.md)          | Yes | Find optimum worker count                            |
| 7    | [target-quality](./commands/target-quality.md)| Yes | Search optimum quantizer per scene                   |
| 8    | [optimize-bitrate](./commands/optimize-bitrate.md) | No | Clamp high-bitrate outliers                      |
| 9    | [scale-speed](./commands/scale-speed.md)      | No  | Interpolate encoder speed from quantizer             |
| 10   | [encode](./commands/encode.md)                | Yes | Encode scenes in parallel                            |
| 11   | [concatenate](./commands/concatenate.md)      | Yes | Join scene encodes into output                       |
| 12   | [quality-check](./commands/quality-check.md)  | Yes | Score output per scene                               |

Full pipeline without subcommand is documented in [condor](./commands/condor.md). It runs steps 2-12 in order. Use `--skip-scd` to reuse existing scenes.

```bash
$ condor init "input.mp4" "output.mkv"
$ condor detect-scenes
$ condor zone '-1 200 svt-av1 "--preset 3"'
$ condor detect-noise
$ condor scale-noise --threshold 0.002
$ condor benchmark
$ condor target-quality --metric ssimulacra2 --target 85
$ condor optimize-bitrate
$ condor scale-speed
$ condor encode
$ condor concatenate
$ condor quality-check
```

## Configuration Lifecycle

1. `init` creates the JSON configuration file (default `./condor.json`, override with `--config-file`). Defaults: encoder `svt-av1`, input filter `resize:scaler=bicubic;format=yuv420p10le`, scene detector standard 1s-10s.
2. Each step loads JSON configuration file, updates its relevant section, and saves on completion.
3. Resume by rerunning the same command; progress is preserved in the JSON configuration file and `temp` directory.
4. `clean` removes temporary files (currently unimplemented, see [clean](./commands/clean.md)).
5. Between steps, a command's saved section can be read, changed and reset without running it: the commands that expose `info` / `set` / `reset` subcommands (e.g. `condor target-quality set target 85`, `condor detect-noise reset`) are listed in [Editing saved settings](./commands/config-editing.md).

## Temp and Logs

- `--temp`: directory for scenes and intermediate encodes. Default is `./<hash-of-input-name>`.
- `--logs`: log file path. Default `./logs/condor.log`.
- `--config-file`: JSON configuration path. Default `./condor.json`.

## TUI vs Non-TUI

TUI commands: `detect-scenes`, `detect-noise`, `benchmark`, `target-quality`, `encode`, `concatenate`, `quality-check`, and full pipeline. See them in action in the [gallery](../README.md#gallery).

Non-TUI commands: `init`, `zone`, `scale-noise`, `optimize-bitrate`, `scale-speed`, `clean`.

## Inputs, Decoders, Filters

Complex types are documented in the [type reference](./types.md).

- Input can be video or VapourSynth script (`.py`, `.vpy`).
- `--decoder`: see [Decoder](./types/decoder.md). Default `bestsource`; methods besides `ffms2` require VapourSynth plugins.
- `--filters`: see [Filters](./types/filters.md). Default `resize:scaler=bicubic;format=yuv420p10le`.
- `--scd-input`, `--scd-decoder`, `--scd-filters`, `--scd-vs-args`: overrides for scene detection. See [Decoder](./types/decoder.md), [Filters](./types/filters.md), [VS Args](./types/vs-args.md).
- `--tq-input`, `--tq-decoder`, `--tq-filters`, `--tq-vs-args`: overrides for target quality. See [Decoder](./types/decoder.md), [Filters](./types/filters.md), [VS Args](./types/vs-args.md).
- `--vs-args`: see [VapourSynth Arguments](./types/vs-args.md). `key=value` pairs passed to VapourSynth script environment. Repeat flag for multiple values.
- `--encoder`: see [Encoder](./types/encoder.md). `--params`: see [Encoder Parameters](./types/encoder-params.md).
- `--photon-noise` / `--chroma-noise`: see [Photon Noise](./types/photon-noise.md).
- `--zones` / `zone ZONE...`: see [Zone](./types/zone.md).
- `--target-metric` / `--metric`: see [Quality Metric](./types/quality-metric.md). `--profile` / `--target-profile`: see [Quality Profile](./types/quality-profile.md).
- `--concat` / `concatenate --method`: see [Concatenation](./types/concatenation.md).
- `detect-scenes --method`: see [Scene Detection](./types/scene-detection.md).
- `condor.json`: see [Configuration](./configuration/index.md).
