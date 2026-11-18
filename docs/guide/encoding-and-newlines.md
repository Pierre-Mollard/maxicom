# Encoding and newlines

How the received bytes become text, in both modes.

These settings only change the text. The raw side panel, the raw recordings
and the triggers always get the bytes exactly as received.

## Text encoding

`encoding` in `config.toml` sets how the received bytes are read as
characters.

| Encoding | For |
| --- | --- |
| `utf8` (default) | Most devices today |
| `ascii` | 7-bit text only |
| `cp437` | The IBM PC character set: old BIOS, DOS and bootloader screens drawn with box characters |
| `latin1` | ISO-8859-1, the Western European 8-bit set |

To change it:

- `Ctrl-a O`, or `X` in the settings popup, cycles through them;
- `encoding cp437` on the [command line](command-line.md) sets one;
- `-R cp437` sets it at startup.

A change applies to the bytes that come next, not to what is already on
screen. A recording keeps the encoding it was made with, so its replay
decodes it the same way.

### Bytes that aren't valid text

Sometimes the device sends bytes that don't make a character in the
encoding: a binary file printed with `cat`, an uninitialized buffer, a wrong
baud rate. They show as `�`, in line mode and in terminal mode alike, so you
can see that bad data came in.

- In `utf8`, a byte that can't start a character, or a character cut short,
  shows as one `�`: `F1 80` followed by `A` is one `�`, two stray `80` bytes
  are two.
- In `ascii`, each byte from `80` up shows as `�`. The bytes below `80`
  are read as in `utf8`, so escape sequences and colors work.
- In `latin1` and `cp437`, every byte is a character, so nothing shows as
  `�`.

A `�` the device really sends (`EF BF BD` in `utf8`), like a shell echoing a
pasted `�`, shows too, in both modes.

A copy and a search give the `�` too. The
[raw side panel](raw-side-panel.md), and a raw copy (`Y` in copy mode), show
the bytes as they were received, `F1 80` here.

In terminal mode, a character still incomplete at the end of what has
arrived waits for the next byte, which says whether it is complete or
broken.

## Received newlines

Devices don't all end their lines the same way. A line break is normally
CR LF: CR brings the cursor back to the left, LF moves it down.

### Lines going down like stairs

The device ends its lines with LF only. In terminal mode, each line starts
where the previous one ended.

Turn on **add CR** (`add_cr`): a lone LF is read as CR LF.

### Lines written over each other

The device ends its lines with CR only, so each line overwrites the
previous one, in both modes.

Turn on **add LF** (`add_lf`): a lone CR is read as CR LF. In line mode, a
lone CR then ends its line instead of starting it over. This is the only one
of the two that changes anything in line mode, where an LF already ends a
line.

### Turning them on

| | Add CR | Add LF |
| --- | --- | --- |
| Key | `Ctrl-a u` | `Ctrl-a a` |
| Settings popup | `R` | `F` |
| Command line | `set add-cr on\|off` | `set add-lf on\|off` |
| `config.toml` | `add_cr = true` | `add_lf = true` |

Both are off by default. While one is on, the status bar shows `+CR` or
`+LF`; a click on it turns it off.

The device's own CR LF stays a single line break.

They apply to the bytes that come next, in both modes and in the text
recordings, a restored session included.

## Sent newlines

In terminal mode, Enter sends CR, as a terminal does.

In line mode, Enter sends `tx_eol` from `[line_mode]`: `lf` (default), `cr`
or `crlf`. `N` (Line Enter) in the settings popup cycles it. See
[Line mode](line-mode.md#line-ending).
