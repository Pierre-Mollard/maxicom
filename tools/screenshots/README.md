# Guide screenshots

`shots.py` makes the screenshots of the user guide (`docs/guide/images/*.png`).
It drives the real maxicom in a hidden tmux window, against fake devices, then
renders the screen as a PNG.

Because every picture comes from a script, they can all be redone after a UI
change, and they all look the same: same window size, same colors, same font.

## Requirements

Linux or macOS (the fake serial ports are ptys), with:

| Tool | For | Install |
|---|---|---|
| Rust and cargo | Building maxicom | [rustup.rs](https://rustup.rs) |
| Python 3.8+ | The script; standard library only | Your system's package |
| tmux | Running maxicom in a hidden window, and capturing it | `apt install tmux` |
| [freeze](https://github.com/charmbracelet/freeze) | Rendering the captured text as an image | See below |
| ffmpeg (with ffprobe) | Cropping and scaling the images | `apt install ffmpeg` |

freeze comes from Charm's apt repository (Debian, Ubuntu):

```bash
sudo mkdir -p /etc/apt/keyrings
curl -fsSL https://repo.charm.sh/apt/gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/charm.gpg
echo "deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *" | sudo tee /etc/apt/sources.list.d/charm.list
sudo apt update && sudo apt install freeze
```

On macOS: `brew install freeze tmux ffmpeg`.

## Usage

From the repository's root:

```bash
python3 -I tools/screenshots/shots.py                    # every screenshot
python3 -I tools/screenshots/shots.py line-mode restore  # only these
```

It builds maxicom (`cargo build`), then writes each image to
`docs/guide/images/<name>.png`. A run of all of them takes about two minutes.

A scenario that fails says why and the others go on. The script exits with an
error if one failed.

## What it does

For each scenario:

1. **A throwaway home.** `target/screenshots/<name>/` becomes `HOME`, and the
   `XDG_*` variables are unset. maxicom writes its default config files there,
   which the scenario then adjusts: `mode = "line"`, a `links.toml`, a
   `triggers.toml`, past sessions in the cache.
2. **Fake devices.** A pty plays the role of a serial port, fed line by line
   by a thread: an ESP32 boot log, a board's shell, a full-screen process
   viewer. The fake port is saved as the link `esp32`, so the status bar shows
   that name instead of `/dev/pts/N`. The directory scenario also opens TCP
   ports, one listening and one closed.
3. **maxicom in tmux.** maxicom runs at 100x28 in a detached session of a tmux
   server of its own (`tmux -L maxicom-shots`). The script sends keys to it,
   like `Ctrl-a d` then `t`, and captures the pane with its colors
   (`tmux capture-pane -e`).
4. **Render.**
   - The 16 basic colors are rewritten to the tokyonight-night palette, as RGB.
   - `freeze` draws the text as a PNG, without a window frame.
   - ffmpeg crops the extra space freeze leaves at the bottom, and scales the
     image to 1400 px wide.

The capture and its plain-text version stay in `target/screenshots/<name>/`
(`<name>.ans`, `<name>.txt`), handy to check a scenario without opening the
image.

## Your own setup is never touched

- maxicom runs with a `HOME` under `target/`, so your config files, saved links
  and session backups aren't read or written. The script refuses to run if that
  folder isn't under `target/`.
- The tmux server is a separate one: your tmux sessions can't receive its keys.
- Recordings made by a scenario go to `~/captures` of the throwaway home.

## The scenarios

| Name | Shows | Used in |
|---|---|---|
| `overview` | Terminal mode on a board's shell | Introduction |
| `help` | The help popup | Introduction |
| `settings-popup` | The settings popup | Popups |
| `directory` | The directory after a test | The directory |
| `line-settings` | The line settings popup | Links |
| `line-mode` | An ESP32 boot log with timestamps, a line being typed | Line mode |
| `terminal-mode` | A full-screen process viewer | Terminal mode |
| `copy-mode` | A selection in copy mode | Copy mode and mouse |
| `search` | A search and its matches | Search |
| `raw-side-panel` | The hex panel next to the text, at 130 columns | Raw side panel |
| `command-line` | The completion list | Command line |
| `triggers` | The trigger browser | Triggers |
| `record-dialog` | The recording dialog | Capture |
| `restore` | A restored session and its separator | Capture |
| `session-browser` | The session browser with past sessions | Capture |
| `status-bar` | The status bar alone, with a recording and a trigger armed | Status bar |

## Adding a scenario

Add an entry to `SCENARIOS` at the end of `shots.py`:

```python
"my-shot": lambda: scenario(
    "my-shot", [esp()], [fed(), press("C-a", "o"), wait(0.5)],
    config=LINE, links=esp_link),
```

- The devices: `esp()` for the boot log, `Pty(lines, every)` for your own feed
  (each line is sent with CR LF; `bytes` are sent as they are).
- The steps run in order: `fed()` waits for every device to finish its feed,
  `press(...)` sends tmux key names (`C-a`, `Enter`, `PPage`), `typed("text")`
  types text, and `wait(seconds)` waits.
- The options: `config` sets keys of `config.toml` as
  `{(section, key): "toml value"}`, with `None` for the top level. There are
  also `links`, `triggers`, `fixtures` (past sessions), `size`, `rows` (a slice
  of the screen's rows, like `(-1, None)` for the bottom row only), and
  `restore=True` to let maxicom restore.

Then reference the image in the guide:

```markdown
![What it shows, in a sentence](images/my-shot.png)
```

## Good to know

- **The colors.** tmux writes the basic colors as 256-color codes, which freeze
  draws with dull xterm colors, and freeze draws the default text and
  background with its theme's dim colors. That's why the script rewrites every
  basic color, the default text and the default background as RGB before
  rendering. The palette is tokyonight-night's, from the
  [tokyonight.nvim](https://github.com/folke/tokyonight.nvim) terminal themes.
- **Long paths.** Popups that show a path in full (the session browser's
  folder) would show `target/screenshots/...`. The script shows it as
  `/home/me/...` in the image, with the same width so the popup's border stays.
- **The fake ports are ptys.** Where maxicom shows a port's path or name (the
  directory, the session browser, the restore separator), it reads
  `/dev/pts/N` or `N`, where a board would give `ttyUSB0`.

## Note

omg Claude thanks for this -- Pierre
