# Magpie

Screenshot history for niri in the Noctalia bar. Every screenshot niri takes,
by keybind or by `niri msg action screenshot*`, shows up in a dropdown where you
can copy it or open it in the magpie annotation editor. Annotated versions are
listed under their screenshot, and each one can be copied or used as the start
of a new version.

## Requirements

- `magpie`: the CLI and editor from this repository
  (`cargo install --path crates/magpie --locked`)
- `niri`: magpie follows its event stream for captured screenshots
- `wl-copy` and `wl-paste` from wl-clipboard

## Entries

| Entry | Kind | What it does |
| --- | --- | --- |
| `watch` | service | Keeps `magpie watch` running and publishes the shot list |
| `button` | bar widget | Camera glyph with the number of new shots; click opens the panel, right click copies the newest shot |
| `shots` | panel | The screenshot list with Copy and Annotate for every shot and version |

Open the panel directly with:

```sh
noctalia msg panel-toggle toms/magpie:shots
```

Add the button to a bar:

```toml
[widget.magpie]
type = "toms/magpie:button"

[bar.top-bar]
start = ["launcher", "magpie"]
```

## Settings

| Key | Default | Meaning |
| --- | --- | --- |
| `magpie_command` | `magpie` | Path or name of the magpie executable |
| `list_limit` | `30` | How many of the newest shots the panel lists |

## IPC

```sh
noctalia msg plugin toms/magpie:watch all refresh     # re-read the library
noctalia msg plugin toms/magpie:watch all reconnect   # restart magpie watch
noctalia msg plugin toms/magpie:watch all command ~/dev/magpie/target/debug/magpie
```
