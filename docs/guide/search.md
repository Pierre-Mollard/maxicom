# Search

`Ctrl-a /` (or Search in the right-click menu) opens the search bar over the
bottom of the history. It works in both modes.

## Searching

Type the text: every match lights up as you type, and the current one
stands out. The bar shows the current match's number and the count. Up to
1000 matches are found.

| Key | Does |
|---|---|
| Ctrl-n / Ctrl-p | Next / previous match |
| Enter | Close the bar and enter [copy mode](copy-mode-and-mouse.md#copy-mode) on the current match |
| Esc | Close the bar where you are |
| Tab | Switch between a text search and a byte search |

## Text search

A text search ignores the case of ASCII letters: `error` finds `ERROR`.
Other characters must match exactly.

It searches what is on display: in line mode, the lines in their current
display mode; in terminal mode, the screen and the scrollback.

## Byte search

A byte search looks for bytes in the received data. Write them the way the
[raw side panel](raw-side-panel.md) shows them:

| Panel view | Examples |
|---|---|
| hex (default) | `0d0a`, `0d 0a`, `0x0d 0x0a`, `\x0d\x0a` |
| octal | `015 012` |
| binary | the bits of each byte |

## From the command line

`search <text>` on the [command line](command-line.md) opens the bar on that
text. Bind it to a key for a search you use often.

## Colors

The `[search]` section of `theme.toml`: `search_match` for the matches,
`search_match_active` for the current one, and `search_indicator` for the
bar's label.
