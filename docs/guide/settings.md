# Settings

## The files

maxicom's configuration is split into a few TOML files, all in the config
folder: `~/.config/maxicom/` on Linux, see
[Install](install.md#where-its-files-are) for the other systems.

| File | Holds | Chapter |
| --- | --- | --- |
| `config.toml` | The settings | This chapter |
| `keymap.toml` | The keys | [Keybindings](keybindings.md) |
| `theme.toml` | The colors | [Theme](theme.md) |
| `highlights.toml` | Words colored in the received text | [Highlights](highlights.md) |
| `triggers.toml` | Answers to boot prompts | [Triggers](triggers.md) |
| `links.toml` | The saved links, once one is saved | [Saved links](saved-links.md) |

`theme_path`, `keybindings_path`, `highlights_path`, `triggers_path` and
`links_path` in `config.toml` put a file elsewhere. `~` is your home
folder.

### Another settings file

`maxicom --config <file>` reads the settings from that file instead of
`config.toml`, for this run. It's handy for a setup per board or per use:
one file for an ESP32 at 115200 with the triggers armed, another for logs
in line mode.

```bash
maxicom --config ~/maxicom/esp32.toml
```

The other files still come from the config folder, unless the file's own
`*_path` settings point elsewhere, so a settings file can bring its own theme
or keys. When the file doesn't exist yet, it's created with the defaults,
ready to edit. `Ctrl-a r` reloads the same file, and the settings popup lists
it next to the config folder.

### Defaults

On the first start, maxicom writes each file with its defaults, except
`links.toml`, which is created on the first save.

A setting left out of a file takes its default, so a file can hold only what
you change. Deleting a file brings back its defaults at the next start.

### Errors

A file that doesn't load (a TOML error, an unknown value, a bad time format)
stops maxicom at startup, with the file and the reason.

During a reload, the running config stays, and a popup says what went
wrong.

## The settings popup

`Ctrl-a o` or `Ctrl-a s` (or `≡` in the status bar) shows the settings you
change most, with their value and their key.

Each key changes its setting at once, for this session only: `config.toml`
isn't written. A click on a setting does the same.

| Key | Setting | Key | Setting |
| --- | --- | --- | --- |
| `M` | Mode (line / terminal) | `E` | Local echo |
| `X` | Text encoding | `T` | Timestamps |
| `U` | Mouse | `B` | Raw side panel view |
| `R` | Add CR | `F` | Add LF |
| `N` | Line Enter (`tx_eol`) | `W` | Line wrap |
| `I` | Wrap indent | `P` | Row marks (`prefix_place`) |
| `D` | Display mode | `S` | Dimension mode |
| `A` | Answer queries | | |

### Setting a value directly

A key flips its setting, so you need to know where it stands. The `set`
command gives the value you want instead, and does nothing when the setting
is already so. That suits a [key bound to a command line](command-line.md#binding-a-command-line-to-a-key):

```text
set local-echo on
set timestamps off
```

`set` takes `local-echo`, `timestamps`, `mouse`, `answer-queries`, `add-cr`
and `add-lf`. For the encoding, use `encoding utf8|ascii|cp437|latin1`. The
other settings, with more than two values, cycle with their keys.

### Config paths

The popup also shows the config folder, and any file moved elsewhere.

- `C`, or a click on the folder, copies its path.
- A file's number (`1`, `2`...), or a click on it, copies the file's path.

### Layout

When the screen has room, the popup shows the settings in two columns:
General and Newlines, then Line Mode and Terminal Mode. On a narrower
screen it uses one column, and it scrolls when the screen is too short.

## Help

`Ctrl-a ?` or `Ctrl-a z` (or `?` in the status bar) lists the keys, in tabs:

| Tab | Lists |
| --- | --- |
| Prefixed | The keys after `Ctrl-a` |
| Directs | The keys without prefix |
| Commands | The [command line](command-line.md)'s commands, and the keys bound to a line |
| Copy | Copy mode's keys |
| Search | The search bar's keys |
| Line | Line mode's keys |
| Terminal | Terminal mode's keys |
| Dialogs | The [popup keys](popups.md) |

It shows the keymap in use, your changes included. `c` copies the config
folder's path.

## Reload

`Ctrl-a r` reads all the files again and applies them. Edit a file, reload,
and see the change without quitting.

The changes made in the settings popup are lost then: the files' values
win.

### Settings that need a restart

A few settings are read at startup only. A reload tells you which of them
changed:

- `line_mode.history_size`, `line_mode.send_history_size`;
- `terminal_mode.history_size`, `terminal_mode.rows`, `terminal_mode.cols`;
- `sessions.enabled`, `sessions.keep`, `sessions.max_size`.

A running session backup also keeps its text settings until the next
session.

## config.toml

The default file, section by section, with a link to where each setting is
explained.

Durations are written `"500ms"`, `"30s"`, `"4h"` or `"1day"`.

### Top level

| Setting | Default | |
| --- | --- | --- |
| `mode` | `"terminal"` | The mode at startup, `line` or `terminal`: [Introduction](introduction.md#two-modes) |
| `local_echo` | `false` | Show the lines sent: [Line mode](line-mode.md#local-echo) |
| `add_cr`, `add_lf` | `false` | Read a lone LF / CR as CR LF: [Encoding and newlines](encoding-and-newlines.md#received-newlines) |
| `encoding` | `"utf8"` | `utf8`, `ascii`, `cp437`, `latin1`: [Encoding and newlines](encoding-and-newlines.md#text-encoding) |
| `dual_mode_buffering` | `"always_both"` | `always_both` or `active_only`: [Introduction](introduction.md#two-modes) |
| `rx_buffer_size` | `4096` | Bytes read from the link at once, 512 to 65536 |
| `show_timestamps` | `false` | Times before the lines / rows: [Line mode](line-mode.md#timestamps) |
| `timestamp_format` | `"[%H:%M:%S%.3f]"` | strftime format of those times |
| `inactivity_detach_timeout` | `"4h"` | Detach a link idle that long, `"0s"` never: [Links](links.md#idle-links) |
| `theme_path`, `keybindings_path`, `highlights_path`, `triggers_path`, `links_path` | none | Another place for that file |

### `[line_mode]`

See [Line mode](line-mode.md).

| Setting | Default | |
| --- | --- | --- |
| `tx_eol` | `"lf"` | What Enter adds: `lf`, `cr`, `crlf` |
| `display_mode` | `"pretty"` | `pretty`, `clean`, `raw`, `hex`, `octal`, `binary` |
| `wrap_mode` | `"word"` | `word`, `char`, `truncate` |
| `wrap_indent` | `"timestamp"` | `timestamp`, `prefix`, `none` |
| `history_size` | `250000` | Lines kept |
| `send_history_size` | `1000` | Sent lines kept for Up / Down |
| `show_partial_rx` | `true` | Show the line still coming in |
| `entry_spacing` | `0` | Blank rows between lines |
| `prefix_place` | `"border"` | Row marks: `border`, `inset`, `none` |
| `inset_prefix` | `{ line = "> ", wrap = "  ", partial = "… " }` | The marks of `inset` |
| `unfocused_cursor_glyph` | `"█"` | The input's cursor while unfocused |
| `prompt_prefix`, `prompt_vi_prefix` | `"> "`, `"< "` | The prompt, typing / in vi normal mode |
| `prompt_hidden_left`, `prompt_hidden_right` | `"◀"`, `"▶"` | The marks of text hidden past the input's edges |

### `[terminal_mode]`

See [Terminal mode](terminal-mode.md).

| Setting | Default | |
| --- | --- | --- |
| `dimension_mode` | `"grow_only"` | `grow_only`, `match_view`, `fixed_cols`, `fixed` |
| `rows`, `cols` | `24`, `80` | The size of `fixed` (`cols` for `fixed_cols`) |
| `history_size` | `100000` | Scrollback lines kept |
| `empty_fill_char` | `"░"` | Fills the view outside the emulated screen |
| `unfocused_cursor_glyph` | `"█"` | The cursor while unfocused |
| `answer_queries` | `true` | Answer the device's terminal queries |
| `answer_groups` | `["status", "identity", "size", "modes"]` | Which queries |
| `answerback` | `""` | Sent back on ENQ, nothing when empty |

### `[link]`

The device and line settings used when none are given. See
[Links](links.md#default-link).

| Setting | Default |
| --- | --- |
| `device` | `""` |
| `baud` | `115200` |
| `bits` | `8` |
| `parity` | `"none"` |
| `stop_bits` | `1` |
| `flow` | `"none"` |

### `[capture]`, `[sessions]`, `[screenshot]`

Recordings, session backups and their restore, and screenshots. See
[Capture](capture.md#settings), which lists them with their defaults.

### `[ui]`

| Setting | Default | |
| --- | --- | --- |
| `show_borders` | `true` | The boxes' borders |
| `show_scrollbar` | `true` | The history's scrollbar |
| `show_status_bar` | `true` | The status bar |
| `title_details` | `false` | More in the history's title, see below |
| `mouse` | `true` | Mouse support: [Copy mode and mouse](copy-mode-and-mouse.md#mouse) |
| `middle_click_paste` | `true` | A middle click pastes the last copy, see [Middle-click paste](copy-mode-and-mouse.md#middle-click-paste) |
| `focus_effects` | `true` | The gray border and cursor while the window isn't focused, see [Cursor](terminal-mode.md#cursor) |
| `hide_borders_below` | `[60, 16]` | See below |
| `hide_scrollbar_below` | `[40, 10]` | See below |
| `hide_status_bar_below` | `[35, 6]` | See below |

`[ui.status_bar]` (`left`, `center`, `right`, `time_format`, `hover`,
`layout`) has its own chapter: [Status bar](status-bar.md).

#### Title details

By default the history box's title only shows ` maxicom LINE ` or
` maxicom TERMINAL `.

With `title_details = true`, it also shows the alerts (`!`, `● REC`), the
emulator and view sizes, the scroll, the pan and the prefix. What doesn't
fit in the border is left out whole, never cut.

A click on `SCROLL +n` goes back to the live output, and a click on
`PAN +n` back to the left edge.

#### Small windows

`hide_borders_below`, `hide_scrollbar_below` and `hide_status_bar_below` each
take a `[cols, rows]` size. While the window is narrower or shorter than
that, the part isn't drawn, to leave the room to the content.

```toml
hide_borders_below = [60, 16]     # 80x24 keeps them
hide_scrollbar_below = [40, 10]
hide_status_bar_below = [35, 6]   # a row of its own once the borders go
```

The defaults hide the borders first and the status bar last. `0` turns a
side off.

The settings themselves don't change: a bigger window brings the parts back.

While the prefix is active, a status bar hidden this way shows over the
view's last row, so you can see the prefix and the link. The view keeps its
size. `show_status_bar = false` keeps it off for good.

### `[ui.marks]`

How the history's [marks](marks.md#how-marks-show) show.

| Setting | Default | |
| --- | --- | --- |
| `letters` | `true` | A mark's letter in the history's left column |
| `scrollbar` | `true` | The marks' letters on the scrollbar |
| `flash` | `true` | The line a jump lands on flashes |

### `[raw_side]`

See [Raw side panel](raw-side-panel.md#size-and-position).

| Setting | Default | |
| --- | --- | --- |
| `view` | `"off"` | `off`, `hex`, `octal`, `binary` |
| `orientation` | `"right"` | `right` or `bottom` |
| `split_percent` | `40` | 10 to 85 |

### `[command_line]`

See [Command line](command-line.md#look).

| Setting | Default | |
| --- | --- | --- |
| `layout` | `"popup"` | `popup` or `status_bar` |
| `history_size` | `100` | Lines kept |
