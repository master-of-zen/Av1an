# Scale Noise

Scale Photon Noise ISO per scene based on Noise Detector results. Scenes must have Photon Noise (`--photon-noise`/`--chroma-noise`) configured. Scenes below the threshold are not scaled. See [guide](../guide.md) and [Photon Noise](../types/photon-noise.md).

```bash
$ condor scale-noise --threshold 0.002
$ condor scale-noise --threshold 0.002 --minimum-scaler 0.5 --maximum-scaler 2.0 --scale-chroma
```

| Name                                            | Flag               | Type  | Default |
| ----------------------------------------------- | ------------------ | ----- | ------- |
| [Threshold](#threshold---threshold)             | `--threshold`      | Float |         |
| [Minimum Scaler](#minimum-scaler---minimum-scaler) | `--minimum-scaler` | Float |      |
| [Maximum Scaler](#maximum-scaler---maximum-scaler) | `--maximum-scaler` | Float |      |
| [Scale Chroma](#scale-chroma---scale-chroma)    | `--scale-chroma`   |       |         |

## Threshold `--threshold`

Minimum noise value for a scene to scale Photon Noise ISO.

### Examples

- `> condor scale-noise --threshold 0.002` - Recommended value

## Minimum Scaler `--minimum-scaler`

Minimum scale factor for Photon Noise ISO scaling.

## Maximum Scaler `--maximum-scaler`

Maximum scale factor for Photon Noise ISO scaling.

## Scale Chroma `--scale-chroma`

Scale Chroma ISO.

## Edit saved settings

`info`, `set` and `reset` read, change and reset the saved noise-scaling settings without running the command:

- `set threshold NOISE` — noise below which a scene is not scaled (recommended `0.002`).
- `set minimum-scaler FACTOR` / `set maximum-scaler FACTOR` — bounds for the ISO scale factor; rejected when the minimum exceeds the maximum.
- `set scale-chroma [on|off]` — scale the chroma ISO too; the value may be omitted, which means `on`.
- `info [threshold|minimum-scaler|maximum-scaler|scale-chroma]` prints JSON; `reset KEY` resets one of them, and `reset` with no key drops the whole optional section back to `null`.

```bash
$ condor scale-noise set threshold 0.002
$ condor scale-noise set maximum-scaler 4
$ condor scale-noise reset threshold
$ condor scale-noise reset
```

Shared semantics (validation, staleness warnings): see [Editing saved settings](config-editing.md).

## Screenshots

The screenshots below are generated from the live interface. Click any image to open it full size.

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="../media/tui/help-scale-noise-light.avif"><picture><source srcset="../media/tui/help-scale-noise-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-scale-noise-light.avif" alt="condor scale-noise --help" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor scale-noise --help</p>
    </td>
    <td width="50%" valign="top">
      <a href="../media/tui/help-scale-noise-verbose-light.avif"><picture><source srcset="../media/tui/help-scale-noise-verbose-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-scale-noise-verbose-light.avif" alt="condor scale-noise --help --verbose" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor scale-noise --help --verbose</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="../media/tui/help-scale-noise-set-light.avif"><picture><source srcset="../media/tui/help-scale-noise-set-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-scale-noise-set-light.avif" alt="condor scale-noise set --help" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor scale-noise set --help</p>
    </td>
    <td width="50%" valign="top"></td>
  </tr>
</table>
