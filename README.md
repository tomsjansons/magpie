# magpie

Screenshot library and annotator for [niri](https://github.com/YaLTeR/niri)
and [Noctalia](https://noctalia.dev).

niri keeps taking the screenshots, with your existing keybinds. magpie picks up
every one it captures, lists them in a Noctalia bar dropdown, and opens them in
a small [GPUI](https://gpui.rs) editor for arrows, boxes, circles and text.
Each save becomes a new version of the screenshot, so you can copy the original
or any version, or start a new version from any of them. Shapes stay editable:
a version started from v2 opens with v2's arrows and text, ready to move.

## Install

```sh
cargo install --path crates/magpie --locked
ln -s "$PWD/plugin" ~/.local/share/noctalia/plugins/magpie
noctalia msg plugins enable toms/magpie
```

Then add the `toms/magpie:button` widget to a bar (see
[plugin/README.md](plugin/README.md)). Needs wl-clipboard (`wl-copy`,
`wl-paste`).

The editor window has the app id `magpie`, so niri window rules can match it,
for example to open it floating:

```kdl
window-rule {
    match app-id="^magpie$"
    open-floating true
}
```

## How it works

`magpie watch` follows `niri msg --json event-stream`. Each
`ScreenshotCaptured` event adds the saved file to the library in place; a
clipboard-only capture (no file) is read from the clipboard and stored in the
library. The plugin's service keeps that process running and refreshes the
list on every change.

```
~/.local/share/magpie/shots/<id>/
  meta.json          source path, time, versions and their parents
  original.png       only for clipboard-only captures
  v1.json  v1.png    shapes, frame and rendered image for each version
  *.thumb.png        thumbnails for the panel
```

niri's own files are never modified. `MAGPIE_DATA_DIR` moves the library.

## CLI

| Command | Does |
| --- | --- |
| `magpie add <path>` / `--clipboard` | add a screenshot by hand |
| `magpie list [--json] [--limit N]` | shots and versions, newest first |
| `magpie copy <id>[@v2]` | copy the original or a version to the clipboard |
| `magpie path <id>[@v2]` | print its PNG path |
| `magpie edit <id>[@v2]` | open the editor, starting from that version |
| `magpie watch` | follow niri and import screenshots |

## Editor keys

| Key | Action |
| --- | --- |
| `V` `A` `R` `E` `T` | select, arrow, box, circle, text |
| drag | draw with the current tool, or move a shape with select |
| double-click text | edit it (select tool), or click it with the text tool |
| `Enter` / `Shift+Enter` | finish text / new line |
| `Del` | delete the selected shape |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `Ctrl+C` | copy what is on screen without saving |
| `Ctrl+S` | save a new version |
| `Ctrl+Shift+C` | save a new version and copy it |
| `Ctrl+W` | close; unsaved changes are saved as a new version |

Picking a color or width while a shape is selected restyles that shape.

**Background** puts the screenshot on a gradient with padding, rounded corners
and a soft shadow, like a presentation shot. Pick one of the gradients and a
padding size in the row that appears; shapes can be drawn out into the
padding. The frame is saved with the version, so it stays editable and can be
switched off again in a later version.

## License

GPL-3.0-only. The bundled Roboto font is under the SIL Open Font License
(`crates/magpie-core/assets/Roboto-OFL.txt`).
