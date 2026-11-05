# Theme

`theme.toml` holds every color of maxicom's interface. It lives in the
config folder, or where `theme_path` in `config.toml` points. `Ctrl-a r`
applies a change.

The theme doesn't color the device's output:

- the device's own colors, in terminal mode and in line mode's `pretty`
  display, come from the device and your terminal's palette;
- [highlights](highlights.md) add colors to words of the received text.

## Writing an entry

An entry is either a color, which sets the text color, or a table of style
fields:

```toml
[popup]
border = "LightBlue"      # a color alone

[ui.selection]            # a table
fg = "Black"
bg = "#FFB86C"
```

| Field | Takes |
|---|---|
| `fg`, `bg`, `underline_color` | A color |
| `bold`, `dim`, `italic`, `underlined`, `reversed`, `crossed` | `true` or `false` |

A few entries take a color only, not a table: those of `[palette]`,
`[raw_inspector]` and `[session_browser]`, and the terminal's flash colors.

An entry left out takes its default. A value that doesn't parse stops the
file from loading: maxicom doesn't start, and a reload keeps the running
theme.

## Colors

| Write | For |
|---|---|
| `Black`, `Red`, `Green`, `Yellow`, `Blue`, `Magenta`, `Cyan`, `Gray`, `DarkGray`, `LightRed`, `LightGreen`, `LightYellow`, `LightBlue`, `LightMagenta`, `LightCyan`, `White` | The 16 terminal colors; your terminal's palette decides how they look |
| `Reset` | The terminal's default color |
| `"#RRGGBB"` | An exact color |
| `"75"` | An index of the 256-color palette, as a string |

## Entries

### `[palette]`

The base colors the rest builds on. The [status bar](status-bar.md) uses
these names too (`main`, `secondary`...).

| Entry | Default | |
|---|---|---|
| `main` | `Cyan` | The accent (`primary` in the status bar): the mode badge, the baud rate |
| `secondary` | `Yellow` | The second accent: the time, running tasks (mash, file sent, replay) |
| `ternary` | `Green` | A third one |
| `text` | `White` | Text |
| `text_muted` | `#8C929E` | Secondary text: separators, counts |
| `text_warn` | `Red` | Alerts (`warn`): `● REC`, `+CR`, armed triggers |
| `text_good` | `Green` | Good states |
| `unselected` | `White` | Unselected items |
| `links` | 8 indexes | The colors given to links, one per port and the same at every launch (status bar `link`, session browser) |

### `[ui]`

| Entry | Default | |
|---|---|---|
| `background` | `Reset` on `Reset` | The app's background |
| `border_panel` / `border_panel_focused` | `DarkGray` / `Cyan` | A box's border, the focused box's |
| `border_app_unfocused` | `DarkGray` | The borders while the window isn't focused |
| `cursor_unfocused` | `DarkGray` | The cursor while the window isn't focused |
| `cursor_ghost` | `White` on `#4A4A5A` | The copy cursor's place in the other panel |
| `selection` / `selection_raw` | black on `Magenta` / on `Red` | The copy mode selection, in the text / in the raw panel |
| `hover` | reversed | What's under the mouse |
| `status_bar` | `White` on `#1E232D` | The status bar's text; its background fills the `band` layout |
| `scrollbar_thumb` / `scrollbar_track` | `DarkGray` / `#303038` | The scrollbar |

The selection colors match the copy mode badge: magenta in the text, red
while the cursor is in the raw side panel.

### `[line_mode]`

| Entry | Default | |
|---|---|---|
| `line_prefix`, `line_indent_prefix`, `line_partial_prefix` | `DarkGray` | The row marks: a line, a wrapped row, the line coming in |
| `timestamp` | `Yellow` | The times |
| `prompt` / `history` | `Reset` bg | The input box, the history |

### `[terminal]`

| Entry | Default | |
|---|---|---|
| `term_inside` / `term_outside` | `Reset` / `DarkGray` | The emulated screen, the view around it (`empty_fill_char`) |
| `timestamp` | `Yellow` | The times column |
| `timestamp_del`, `timestamp_del_empty` | `Yellow`, `DarkGray` | The `│` after a time, after an empty one |
| `term_flash_fg_color`, `term_flash_bg_color` | `DarkGray`, `White` | The flash when a screenshot is taken |

### `[search]`

See [Search](search.md).

| Entry | Default | |
|---|---|---|
| `search_match` | black on `Yellow` | The matches |
| `search_match_active` | black on `#FF8C00` | The current match |
| `search_indicator` | black on `Green`, bold | The bar's label |

### `[raw_inspector]`

The byte kinds of the [raw side panel](raw-side-panel.md#reading-it).

| Entry | Default |
|---|---|
| `byte_any` | `White` |
| `byte_null` | `DarkGray` |
| `byte_ascii` | `Cyan` |
| `byte_whitespace` | `Yellow` |
| `byte_sync` | `LightRed` |
| `byte_ones` | `Red` |
| `byte_other` | `LightBlue` |

### `[popup]`

| Entry | Default |
|---|---|
| `border` | `Cyan` |
| `background` | `White` on `Reset` |
| `title` | `Cyan`, bold |

### `[notification]`

| Entry | Default | |
|---|---|---|
| `border` | `DarkGray` | The frame |
| `info` | `White` | |
| `success` | `Green` | |
| `warn` | `Yellow` | |
| `error` | `Red` | |

### `[command_line]`

| Entry | |
|---|---|
| `border` | The popup's and the completion list's frame (default `LightGreen`) |
| `prompt` | The `:` |
| `text` | The typed text |
| `selected` | The completion picked in the list |

### `[session_browser]`

The colors of the [session browser](capture.md#session-browser), in the
spirit of `lsd -l`. The defaults are 256-color indexes.

| Entries | Color |
|---|---|
| `date_hour`, `date_day`, `date_week`, `date_month`, `date_older` | The date, fading with the age of the last write |
| `size_empty`, `size_small`, `size_medium`, `size_large`, `size_huge`, `size_giant` | The size, warming as it grows: `size_empty` for an empty session, then steps at 100 KiB, 1 MiB, 100 MiB and 1 GiB |
| `mark_running`, `mark_restorable`, `mark_no_meta` | The marks |
