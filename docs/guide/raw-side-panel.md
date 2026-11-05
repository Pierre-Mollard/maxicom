# Raw side panel

The raw side panel shows the received bytes as they came off the wire, next
to the text. It reveals what the decoding, the colors and the terminal
emulator hide: a stray `\r`, a `0x00`, a framing problem.

<!-- screenshot: line mode with the raw panel in hex on the right -->

## Showing it

`Ctrl-a B`, or `B` in the settings popup, cycles `view` in `[raw_side]`:

| View | Shows each byte as |
|---|---|
| `off` (default) | The panel is hidden |
| `hex` | Hexadecimal |
| `octal` | Octal |
| `binary` | Bits |

`-H` at startup opens it in hex.

## Reading it

The panel follows the history: its rows are the same lines, so a line and
its bytes stay side by side as you scroll.

Bytes are colored by kind, from `[raw_inspector]` in the theme:

| Entry | Bytes |
|---|---|
| `byte_ascii` | Printable ASCII |
| `byte_whitespace` | `\n`, `\r`, `\t` |
| `byte_null` | `0x00` |
| `byte_ones` | `0xFF` |
| `byte_sync` | The sync bytes `0x55`, `0xAA`, `0x7E` |
| `byte_other` | Everything else |

When its rows are wider than the panel, `Ctrl-a Shift-Left` /
`Ctrl-a Shift-Right` pan it.

## Copying and searching bytes

- In [copy mode](copy-mode-and-mouse.md#copy-mode), Tab moves the cursor
  into the panel to select bytes.
- `Y` copies a selection's bytes as the panel shows them.
- A [byte search](search.md#byte-search) takes bytes written in the panel's
  current view.

## Size and position

| Setting in `[raw_side]` | Default | Does |
|---|---|---|
| `split_percent` | `40` | The share of the room it takes, 10 to 85 |
| `orientation` | `"right"` | Where it sits: `right` or `bottom` |

To resize it while maxicom runs:

- drag its border with the mouse;
- `Ctrl-a >` / `Ctrl-a <` grow or shrink it by 5 %;
- the actions `side-grow` / `side-shrink` do it by 1 %, for a key you bind.

The new size lasts until you quit; `config.toml` keeps its value.
