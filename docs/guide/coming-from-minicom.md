# Coming from minicom

You can start maxicom the way you start minicom:
`maxicom -D /dev/ttyUSB0 -b 115200`. The prefix is `Ctrl-a`, and where
maxicom has the same feature, it uses minicom's key.

Every key can be changed in `keymap.toml`; delete the file to get these
defaults back. Note that `c` and `C` (Shift-c) are two different keys, see
[how keys are written](introduction.md#how-keys-are-written).

## Keys

| `Ctrl-a` + | minicom           | maxicom                          |
| ---------- | ----------------- | -------------------------------- |
| `z`        | help              | help                             |
| `x`, `q`   | quit              | quit                             |
| `o`        | configure         | settings (also `s`)              |
| `p`        | comm parameters   | line settings                    |
| `d`        | dialing directory | saved links and the serial ports |
| `e`        | local echo        | local echo                       |
| `w`        | line wrap         | line mode's wrap (see below)     |
| `u`        | add carriage ret. | add CR: a lone LF read as CR LF  |
| `a`        | add linefeed      | add LF: a lone CR read as CR LF  |
| `l`        | capture on/off    | record: text or full             |
| `c`        | clear screen      | clear, the history a scroll up   |
| `f`        | send break        | send break                       |
| `h`        | hangup            | DTR pulse, resets most boards    |
| `b`        | scrollback        | copy mode (also `[`)             |
| `Ctrl-a`   | send Ctrl-a       | send Ctrl-a                      |
| `t`        | terminal settings | timestamps                       |

### Keys minicom doesn't have

| `Ctrl-a` + | Does |
| --- | --- |
| `m` | Switch between line and terminal mode |
| `j` | Session backups |
| `D` | Detach / attach the link |
| `O` | Text encoding |
| `/` / `?` | Search forward / backward |
| `0` / `$` | Pan to the line start / end |
| `:` | Command line |
| `T` | Triggers |
| `U` | Toggle the u-boot trigger |
| `L` | Back to the live link from a replay |
| `W` | Terminal size |
| `E` | Fit the emulator to the view |
| `k` / `K` | Screenshot, with colors / as text |
| `B` | Raw side panel |
| `<` / `>` | Raw side panel size |
| `M` | Mouse on / off |
| `C` | Clear the history |
| `n` / `N` | Close the top notification / all of them |
| `r` | Reload the config |

`Ctrl-a z` lists them all.

## Options and files

| minicom               | maxicom                              |
| --------------------- | ------------------------------------ |
| `-D, --device`        | the same, or alone: `maxicom COM3`   |
| `-b, --baudrate`      | the same, with `--bits`, `--parity`… |
| `-C, --capturefile`   | the received text to that file       |
| `-H, --displayhex`    | the raw side panel in hex            |
| `-R, --remotecharset` | utf8, ascii, cp437 or latin1         |
| `-v, --version`       | the same                             |
| `minicom board`       | `maxicom board`, a saved link        |
| `minirc.dfl`          | `[link]` in `config.toml`            |
| `~/.dialdir`          | `links.toml` in the config folder    |

## What differs

### Flow control is off

minicom turns RTS/CTS on by default; maxicom turns flow control off, because
most USB-UART boards don't wire those lines.

### No default device

With no device given, maxicom opens on the directory. When there is no saved
link and no serial port either, it tells you how to give one.

### The dialing directory is saved links

Its entries are saved links: a port or a TCP link, with its line settings.
You edit them in the app. `links.toml` is rewritten whole on each save, so
comments in it are lost. One file can serve several machines. See
[Saved links](saved-links.md).

### The terminal always wraps

minicom doesn't wrap by default, `-w` turns it on; maxicom always wraps, as
xterm does. There is no `-w`, and `Ctrl-a w` changes the wrap of line mode
only.

### Backspace sends DEL

minicom's Backspace sends BS (`08`), maxicom's DEL (`7f`), as xterm does. For
a device that wants BS, set `backspace = "bs"` in `[terminal_mode]`, see
[Backspace](terminal-mode.md#backspace).

### Some keys send other codes

In terminal mode, maxicom sends the codes xterm sends. Most keys send the same
bytes as in minicom, except these:

| Key | minicom | maxicom |
| --- | --- | --- |
| Backspace | `08` | `7f` (see above) |
| Home | `ESC [ 1 ~` | `ESC [ H` |
| End | `ESC [ 4 ~` | `ESC [ F` |
| F5 | `ESC [ 1 6 ~` | `ESC [ 1 5 ~` |

A device that only knows minicom's Home and End codes won't see these keys.
minicom's F5 code is one that no xterm key sends.

### Received newlines

With add LF on, minicom shows a blank line after each line ending in CR LF:
it adds an LF to the device's own. maxicom keeps that CR LF as one line
break, see [Received newlines](encoding-and-newlines.md#received-newlines).

The two other line controls are read as xterm reads them: vertical tab
(`0B`) and form feed (`0C`) move down one line. minicom ignores the vertical
tab, and clears the screen on a form feed.

### Bytes that aren't valid text

maxicom shows one `�` for each broken character, and never hides the valid
text around it. minicom sometimes shows one `�` for several broken
characters in a row, drops the valid character that follows a broken one, or
puts the `�` after the next tab. See
[Bytes that aren't valid text](encoding-and-newlines.md#bytes-that-arent-valid-text).

The C1 control characters (`80` to `9F` in `latin1`, `C2 80` to `C2 9F` in
`utf8`) aren't shown: minicom draws them as characters.

### Colors and escape sequences

maxicom shows colors without any option (minicom needs `-c on`), with 256
colors, true colors, dim and italic. Sequences that minicom doesn't know
are hidden instead of printed: a window title (`ESC ] 0 ; title BEL`) or a
soft reset doesn't leave text on the screen.

### Local echo shows the text you type

With local echo on, both show what you type, Enter as a new line and
Backspace moving back. minicom also passes the arrows and function keys to
its screen, so its cursor moves; maxicom leaves them out.

### File transfers

minicom's upload (`Ctrl-a s`) and download (`Ctrl-a r`) run `sx`, `sb`, `sz`
and their receivers. maxicom has XMODEM and YMODEM built in, with a progress
bar: `upload ymodem <file>` and `download ymodem` from the command line
(`Ctrl-a :`), where `Ctrl-a s` opens the settings. There is no ZMODEM or
Kermit: detach the link and run `sz` or `rz` yourself. See
[File transfers](file-transfers.md).

### No scripts

There is no shell, no scripts and no modem dialing. maxicom never starts
another program, and sends only what you type, bind to a key or arm. For boot
prompts, use [triggers](triggers.md) and named commands on keys (`dtr`,
`send`, `break`...), see [Command line](command-line.md).
