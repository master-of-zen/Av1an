# Concatenate

Concatenate encoded scenes into output video (triggers TUI). See [guide](../guide.md) and [type reference](../types.md).

```bash
$ condor concatenate
$ condor concatenate --method mkvmerge
```

| Name                              | Flag       | Type     | Default    |
| --------------------------------- | ---------- | -------- | ---------- |
| [Method](#method---method)        | `--method` | [`CONCAT`](../types/concatenation.md) | `mkvmerge` |

## Method `--method`

Method used for concatenating the encoded chunks into the output file. See [Concatenation](../types/concatenation.md).

### Possible Values

- `mkvmerge` - MKVToolNix mkvmerge, Matroska (`.mkv`) only, generally best
- `ffmpeg` - FFmpeg, supports more formats but may break audio seeking
- `ivf` - Indeo Video Format (`.ivf`), video only

### Default

If not specified, `mkvmerge` is used.

### Examples

- `> condor concatenate --method mkvmerge`
- `> condor concatenate --method ffmpeg`

## Edit saved settings

`info`, `set` and `reset` read, change and reset the saved concatenation settings without running the command:

- `set method METHOD` — `mkvmerge`, `ffmpeg` or `ivf`. Keeping the current tool keeps its settings; switching to another tool starts from that tool's defaults.
- `set output PATH` — the file the concatenated video is written to; `reset output` clears it.
- `set scenes-directory PATH` — the directory the encoded scenes are read from.
- `info [method|output|scenes-directory]` prints JSON; `reset KEY` resets one of them, and a bare `reset` restores the method and output to their defaults (the temp-derived `scenes-directory` stays).

```bash
$ condor concatenate set method ffmpeg
$ condor concatenate set output final.mkv
$ condor concatenate reset output
```

Shared semantics (validation, staleness warnings): see [Editing saved settings](config-editing.md).

## Screenshots

The screenshots below are generated from the live interface. Click any image to open it full size.

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="../media/tui/scene-concatenator-light.avif"><picture><source srcset="../media/tui/scene-concatenator-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/scene-concatenator-light.avif" alt="Scene concatenator" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">Scene concatenator</p>
    </td>
    <td width="50%" valign="top">
      <a href="../media/tui/help-concatenate-light.avif"><picture><source srcset="../media/tui/help-concatenate-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-concatenate-light.avif" alt="condor concatenate --help" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor concatenate --help</p>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="../media/tui/help-concatenate-verbose-light.avif"><picture><source srcset="../media/tui/help-concatenate-verbose-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-concatenate-verbose-light.avif" alt="condor concatenate --help --verbose" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor concatenate --help --verbose</p>
    </td>
    <td width="50%" valign="top">
      <a href="../media/tui/help-concatenate-set-light.avif"><picture><source srcset="../media/tui/help-concatenate-set-dark.avif" media="(prefers-color-scheme: dark)"><img src="../media/tui/help-concatenate-set-light.avif" alt="condor concatenate set --help" style="width:100%;border-radius:8px;display:block;"></picture></a>
      <p style="text-align:center;font-size:0.85em;opacity:0.75;margin:0.35em 0 0;">condor concatenate set --help</p>
    </td>
  </tr>
</table>
