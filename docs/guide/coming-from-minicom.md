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
| `minicom esp32`       | `maxicom esp32`, a saved link        |
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

There is no `-w`. `Ctrl-a w` changes the wrap of line mode only.

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
prompts, use [triggers](triggers.md) and command lines bound to keys (`dtr`,
`send`, `break`...), see [Command line](command-line.md).
