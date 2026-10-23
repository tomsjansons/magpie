# magpie plan

Screenshot library and annotator for niri + Noctalia. niri takes the
screenshots exactly as it does today; magpie only handles what happens after.

## Capture: hook, don't replace

niri keybinds (`Print`, `Ctrl+Print`, `Alt+Print`) and `niri msg action
screenshot*` stay untouched. niri 26.04 emits `ScreenshotCaptured { path }` on
`niri msg --json event-stream` for every screenshot, keybind or CLI.

`magpie watch` holds that stream open and runs `magpie add <path>` logic for
each event. When `path` is null (the interactive UI's Ctrl+C, clipboard only),
it reads `image/png` from the clipboard with `wl-paste` and stores that.

The Noctalia `[[service]]` keeps `magpie watch` running and relays its JSON
lines to `noctalia.state`, so the plugin never polls.

## Layout

```
crates/magpie-core/   library storage, shape model, tiny-skia renderer
crates/magpie/        single binary: CLI + GPUI editor
plugin/               Noctalia plugin, id toms/magpie
```

## Storage: `~/.local/share/magpie/`

```
shots/<id>/meta.json   { source, taken_at, variants: [{ id, parent, created }] }
shots/<id>/v<n>.json   full shape list for that variant
shots/<id>/v<n>.png    rendered output
```

- The original stays where niri wrote it and is never modified.
- `parent` is `"original"` or another variant id; this is the version tree the
  panel draws.
- Each variant stores its complete shape list, not a delta. Starting from v2
  loads v2's shapes as live, editable objects; saving writes v3 with
  `parent = "v2"`.
- PNGs are rendered at save time so copy and thumbnails never re-render.

## CLI

| Command | Does |
| --- | --- |
| `magpie add <path>` / `--clipboard` | import a screenshot |
| `magpie list --json` | shots + variant tree, newest first |
| `magpie copy <id>[@variant]` | `wl-copy -t image/png` |
| `magpie edit <id>[@variant]` | open the editor seeded from that version |
| `magpie watch` | follow niri events, print one JSON line per change |

## Editor (GPUI + gpui-component)

- Tools: arrow, rectangle, ellipse, text; color; stroke width.
- Select, move, delete shapes; undo/redo.
- Ctrl+C copies the composited image, Ctrl+S saves a new variant.
- Screen preview drawn by GPUI. Export drawn by the tiny-skia renderer in
  `magpie-core` from the same shape model, so output never depends on the UI.
- One bundled font used by both, so text matches on screen and in export.

## Plugin

- `[[widget]]` bar button, glyph + count of unseen shots.
- `[[panel]]` attached dropdown: shots newest first with thumbnail, Copy,
  Annotate; variants indented beneath with the same buttons.
- `[[service]]` supervises `magpie watch`.
- Thin client: every action shells out to `magpie` (argv `runAsync`, API 24).
- Declare the lowest `plugin_api` that covers what is used.

## Milestones

1. core storage + CLI (`add`, `list`, `copy`) + `watch`
2. plugin: service, bar widget, panel with copy
3. editor: canvas, tools, save variant, copy
4. README, install notes, plugin catalog

## Risks

- gpui on crates.io (0.2.2) is a year old; gpui-component 0.8 likely needs
  gpui from the Zed git repo. Pin an exact rev and expect upgrade friction.
- Text metrics must match between GPUI and tiny-skia; bundle one font.
