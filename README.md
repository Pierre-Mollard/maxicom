# MAXICOM

minicom but written in rust with my custom features.

## NOTES

- Build with cargo

```bash
cargo build
```

- Run with local bash redirect for testing interactive programs

```bash
./scripts/run_shell.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom-shell-test
```

- Run the bin utility "fake_device" for testing dumb programs

```bash
./scripts/run_fake.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom
```

- Run a device over TCP (`tcp://` links): a shell on `127.0.0.1:4000`, one per
  connection, or a real serial port shared like ser2net

```bash
./scripts/run_tcp.sh                            # zsh on port 4000
./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200   # bridge a serial port
```

```bash
cargo run --bin maxicom -- --device tcp://127.0.0.1:4000
```

- Profile (flame chart) with sample

```bash
cargo build --bin maxicom --profile profiling
```

```bash
samply record ./target/profiling/maxicom --baudrate 115200 --device /tmp/maxicom-shell-test
```

- Check license compat

```bash
cargo deny check
```

## COMING FROM MINICOM

Start it as minicom: `maxicom -D /dev/ttyUSB0 -b 115200`, the prefix is
`Ctrl-A` and its keys are minicom's where maxicom has the feature. Every key
can be changed in `keymap.toml` (delete the file to get these defaults).

| `Ctrl-A` + | minicom           | maxicom                          |
|------------|-------------------|----------------------------------|
| `z`        | help              | help (also `?`)                  |
| `x`, `q`   | quit              | quit                             |
| `o`        | configure         | settings (also `s`)              |
| `p`        | comm parameters   | line settings                    |
| `d`        | dialing directory | the serial ports, `t` tests them |
| `e`        | local echo        | local echo                       |
| `w`        | line wrap         | line mode's wrap (see below)     |
| `u`        | add carriage ret. | add CR: a lone LF read as CR LF  |
| `a`        | add linefeed      | add LF: a lone CR read as CR LF  |
| `l`        | capture on/off    | record: text or full             |
| `c`        | clear screen      | clear, the history a scroll up   |
| `f`        | send break        | send break                       |
| `h`        | hangup            | DTR pulse, resets most boards    |
| `b`        | scrollback        | copy mode (also `[`)             |
| `Ctrl-A`   | send Ctrl-A       | send Ctrl-A                      |
| `t`        | terminal settings | timestamps                       |

maxicom's own: `m` line / terminal mode, `j` session backups, `D` detach /
attach, `O` text encoding, `/` search, `:` command line, `T` triggers, `U`
the u-boot trigger, `W` terminal size, `E` emulator to the view, `k`
screenshot, `B` raw side panel, `M` mouse, `C` clear the history, `r` reload
the config. `Ctrl-A ?` lists them all.

| minicom               | maxicom                              |
|-----------------------|--------------------------------------|
| `-D, --device`        | the same, or alone: `maxicom COM3`   |
| `-b, --baudrate`      | the same, with `--bits`, `--parity`… |
| `-C, --capturefile`   | the received text to that file       |
| `-H, --displayhex`    | the raw side panel in hex            |
| `-R, --remotecharset` | utf8, ascii, cp437 or latin1         |
| `-v, --version`       | the same                             |
| `minirc.dfl`          | `[link]` in `config.toml`            |

What differs:

- Flow control is off by default (minicom: RTS/CTS on), as most USB-UART
  boards don't wire those lines.
- No default device: with none given maxicom opens on the directory of the
  serial ports, or says how to give one when there's none.
- The terminal always wraps long lines: no `-w`, and `Ctrl-A w` changes line
  mode's wrap only.
- No file transfers (X/Y/ZMODEM, kermit), shell, scripts or modem dialing:
  maxicom never starts another program, and sends only what you type, bind
  or arm. Triggers and bound command lines (`dtr`, `send`, `break`, …) cover
  the boot prompts.

## DEVICES

`maxicom [OPTIONS] [DEVICE]`, all the options with `maxicom --help`.

- A serial port: `maxicom /dev/ttyUSB0 -b 921600`, `maxicom COM3`, or with
  `-D, --device` as minicom (`maxicom -D /dev/ttyUSB0 -b 115200`). minicom's
  `-C <file>` (record the received text to that file), `-H` (the raw side
  panel in hex), `-R <encoding>` and `-v` work too, and an option's value can
  be glued to it (`--baudrate=9600`, `-b9600`).
- A device over TCP: `maxicom tcp://192.168.1.50:4000`, or
  `--tcp 192.168.1.50:4000`.
- The directory, `Ctrl-A d` (`directory`, also in the right-click menu), as
  minicom's dialing directory: the serial ports the system lists, `●` on the
  one in use. `↑` `↓` `j` `k` or a click select, Enter (or a click on the
  selected one) switches to it as `open` does, its line settings kept; `r`
  lists the ports again (USB ones come and go). Saved links will come in a
  tab of their own (MAX-185).
  - Enter keeps the directory open (`opening…` on the row) until the port is
    open; one that fails says why on its row.
  - `t` tests the ports, the one in use left out: `free`, `busy` (with the
    program holding it when known, `busy: minicom (1234)`), `no access` (on
    Linux, the user isn't in `dialout` or `uucp`) or its error; one that
    doesn't answer in 2 s says so. Opening a port raises DTR and RTS, which
    resets a board with an auto-reset (ESP32, Arduino), so on Linux the test
    doesn't open them. It reads, in order: the UUCP lock files (minicom) and
    `/proc/locks` (`flock`: picocom, tio, pyserial), which show any user's
    program; the open files of the processes in `/proc`, the user's own only;
    then the permissions. A port another user holds without a lock (screen,
    cu, another maxicom) can't be seen without root and shows `free`. macOS
    has no `/proc`: a port held without a lock file shows `free`. On Windows
    only an open tells, with the port's settings kept; a board may reset
    there.
- The port's popups when it's detached or lost have `d` for the directory,
  to switch to another one. While the link waits (detached, lost, or no
  device picked yet), its popup comes back when the popup on top of it
  closes. A reattach that fails (another program took the port) leaves it
  detached, its popup up, instead of retrying quietly.
- Without a device or line options, the `[link]` section of `config.toml` gives
  them (read at startup, the options over it):

  ```toml
  [link]
  device = ""        # empty: none, a device must be given
  baud = 115200
  bits = 8
  parity = "none"    # none, even, odd
  stop_bits = 1
  flow = "none"      # none, rtscts, xonxoff
  ```

  A port's name depends on the adapter (`/dev/ttyUSB0`, `COM3`,
  `/dev/cu.usbserial-XXXX`), so there's no default one. With no device
  anywhere maxicom starts on the directory, with nothing open until a port is
  picked (no session restored; the backup starts on the port picked); closed
  before that, a "No device" popup asks to pick one (Enter) or quit (Esc).
  With no serial port listed either, it stops and says how to give one.
  Flow control is off by default, where
  minicom has RTS/CTS on: most USB-UART boards don't wire those lines.

### TCP LINKS

The device's UART reached through a bridge, which copies the bytes between a
TCP connection and the UART:

- A TCP-UART bridge in the device's firmware (ESP32: ESP-Link, the ESPHome
  stream server, your own): no cable, and from WSL2 no USB forwarding
- ser2net or socat on the machine the board is plugged into, which shares its
  port on the network (`./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200`). To
  reach it from elsewhere, encrypted: `ssh -L 4000:localhost:4000 pi`, then
  `maxicom tcp://localhost:4000`
- Serial device servers (Moxa, Lantronix, Digi) in their raw TCP mode
- Emulators: QEMU `-serial tcp::4444,server,nowait`, Renode
- RTT logs through a debug probe: J-Link on `localhost:19021`, OpenOCD
  `rtt server`

What changes from a serial port:

- Raw bytes only, no telnet: a ser2net "telnet" port or a server in telnet
  mode shows a few unreadable bytes at connect (its option negotiation). Use
  their raw mode.
- No line settings: the bridge sets the baud and the framing. The status bar
  shows `raw TCP` in their place (the `line_settings` condition hides them),
  the serial settings popup shows the address only, and `--baudrate`, `--bits`,
  `--parity`, `--stop-bits`, `--flow` are refused.
- The UI never waits for the network: it says "Connecting to …"; a host down
  or not listening yet shows "Connection Lost" and is retried every 500 ms,
  like an unplugged port. Detach and attach work the same.
- A link that dies silently (the device powered off, the Wi-Fi dropped) is
  found by TCP keepalive after about 11 s, then retried.
- Typed keys are sent at once, and timestamps are when the bytes arrive from
  the network, not when they crossed the wire.
- Sessions are kept per address: one restores only on the same address, and
  its files are named after it (`192.168.1.50_4000`).
- Prefer an IP address: a `.local` name (mDNS) may not resolve in WSL2.

## CONFIG

- The config files are in `~/.config/maxicom/` (`config.toml`, `theme.toml`,
  `keymap.toml`, `highlights.toml`, `triggers.toml`). The settings popup
  shows that folder, and any file `theme_path`, `keybindings_path`,
  `highlights_path` or `triggers_path` puts elsewhere (`~` is the home
  folder); `C` or a click on the folder copies its path, the file's number
  (`1`, `2`, ...) or a click on it copies the file's. The popup scrolls when
  the screen is too short for it. In the help, `c` copies the folder's path.
- `add_cr` and `add_lf` (default `false`), minicom's add CR / add LF, for a
  device that ends its lines with LF only (staircase text in terminal mode)
  or CR only (lines written over each other in both modes): `add_cr` reads a
  lone LF as CR LF, `add_lf` a lone CR as CR LF. The device's own CR LF stays
  one line break. In line mode only `add_lf` changes something (an LF already
  ends a line): a lone CR ends its line instead of starting it over. They apply
  to the next bytes, in both modes and the text recordings (a session's
  restored part too); the raw recordings, the raw side panel and the triggers
  get the bytes as received. `Ctrl-A u` / `Ctrl-A a`, `set add-cr on|off`,
  `set add-lf on|off`, `R` / `F` in the settings popup; `+CR` / `+LF` in the
  status bar while on. What Enter sends in line mode is `tx_eol` in
  `[line_mode]` (`lf` default, `cr`, `crlf`); in terminal mode Enter sends CR,
  as a terminal does.

### UI

- `title_details` in `[ui]` (default `false`): the history box's title shows
  only ` maxicom LINE ` or ` maxicom TERMINAL `. With `true` it also shows the
  alerts (`!`, `● REC`), the VT and view sizes, the scroll, the pan and the
  prefix. What doesn't fit in the border is left out whole, never cut. A click
  on `SCROLL +n` goes back to the live tail, on `PAN +n` to the left edge.
- `hide_borders_below`, `hide_scrollbar_below` and `hide_status_bar_below` in
  `[ui]`, each a `[cols, rows]` size: while the screen is narrower or shorter
  than it, that part isn't drawn, to leave the room to the content. The
  defaults hide the borders first, the status bar last, as minicom always
  shows its own:

  ```toml
  hide_borders_below = [60, 16]     # 80x24 keeps them
  hide_scrollbar_below = [40, 10]
  hide_status_bar_below = [30, 6]   # a row of its own once the borders go
  ```

  The settings stay as they are: a bigger window brings the parts back. `0`
  turns a side off. While the prefix is on, a status bar hidden this way shows
  over the view's last row, as a line, so the prefix and the link can be seen;
  the view keeps its size (`show_status_bar = false` keeps it off).

### COMMAND LINE

- `Ctrl-A :` (`command-line`) opens a `:` prompt that runs commands: the
  names of `keymap.toml` (`toggle-local-echo`, `record`, ...), chained
  with `;`. Enter runs the line, Esc closes it. A line with an unknown command
  or a bad argument runs nothing: the error shows and the prompt stays.
- Commands with arguments, quoted when they have spaces (`"..."` takes the
  escapes `\r \n \t \xNN`, `'...'` is literal):
  - `send <text>`: sends the text as is, its escapes as bytes:
    `send AT\r`, `send "hello world\r\n"`
  - `send-hex <bytes>`: sends bytes written in hex, no spaces: `send-hex 0d0a`
  - `send-file <path> [line-delay-ms]`: sends a file as is, paced to the baud
    rate and framing (10 bits a byte in 8N1, 12 in 8E2), so the screen stays
    live and the file can be any size. With a delay, it waits that long
    after each line, for a device
    without flow control. The line goes on at once: `send-file a; send b`
    sends `b` first. The status bar shows `⇡ name 42%` while it runs (a click
    stops it), `cancel` stops it too. A detach or a lost link stops it.
    The path completes from the disk (`~` is the home folder).
  - `dtr on|off`, `rts on|off`: set the control lines (`true`/`false`, `1`/`0`
    work too). `rts` is refused with hardware flow control: the driver drives
    RTS then.
  - `break [ms]`: holds the line in break, 250 ms by default. The rest of the
    line waits for its end.
  - `baud <rate>`, `framing <8N1>`, `flow none|hw|sw`: change the port's
    settings, as the link settings popup does. On a detached port they are
    kept for when it opens.

  These six need a serial port: a TCP link (its bridge owns the lines and the
  settings) and a replay refuse them.
  - `open <address>`: switches to another serial port, `tcp://host:port` or a
    replay of `file://path`, the settings kept (`open /dev/ttyUSB1`,
    `open tcp://192.168.1.50:4000`). A bad address is refused before the line
    runs. It completes the schemes and the serial ports the system lists, then
    paths after `file://` or a `/`. The rest of the line waits until it is
    open: `open COM4; send AT\r`. A port that fails to open brings the last
    one back. Another port starts a new session backup, as they go by port.
    `toggle-attach` detaches and attaches the link, as `Ctrl-A D` does.
  - `set <setting> on|off`: `local-echo`, `timestamps`, `mouse` or
    `answer-queries`; nothing when it is so already (the settings with more
    values cycle with their keys).
  - `encoding utf8|ascii|cp437|latin1`: how the received bytes are read.
  - `search <text>`: opens the search bar on the text, its matches shown.
  - `screenshot <name>`: saves the terminal at once, without the dialog, in
    `screenshot.dir` for a bare name: `.ans` keeps the colors, else text
    (`.txt` added to a name without an extension). Terminal mode only.
  - While the port isn't open (detached, lost, reconnecting), `send`,
    `send-file` and the control lines refuse; `baud`, `framing` and `flow`
    are kept for when it opens.
  - `sleep <ms>`: the rest of the line waits that long (up to a minute), the
    screen stays live. A line run meanwhile waits behind it.
  - `cancel`: stops the file being sent, a `mash`, and drops the commands
    waiting. A line starting with it runs it at once, not behind them:
    `cancel; send x`.
  - `mash <text> <until> [every-ms] [timeout-ms]`: sends the text at once,
    then every 50 ms by default, until `until` comes in, 15 s at most, for a
    bootloader that only listens for a moment after a reset. The rest of the
    line waits for `until`: `mash ' ' '=> '; send 'printenv\r'`. The timeout,
    a key typed for the device, `cancel`, a detach or a lost link stop it and
    drop the rest of the line. The status bar shows `⟳ mash` while it runs (a
    click stops it). With a reset first, on one key:
    `dtr off; rts on; rts off; mash ' ' '=> '`.
  - `trigger on|off|toggle [name]`: arms, disarms or toggles the triggers, or
    the one named (see [Triggers](#triggers)); the names complete.

  A command the link can't do (anything sent on a replay) stops the line
  there, with the error.
- In `keymap.toml`, a key of `[prefix_actions]` or `[direct_actions]` takes a
  command line where an action name goes, as tmux's `bind`. An ESP32 into its
  bootloader on one key, as esptool does it, and Ctrl-C as a byte:

  ```toml
  [prefix_actions]
  F6 = "dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off"

  [direct_actions]
  F5 = "send-hex 03"
  ```

  The key runs its line as if typed: behind a line still waiting (unless it
  starts with `cancel`), and it doesn't go in the history. Like the other
  keys of these sections, it does nothing in copy mode and search. A line that
  doesn't parse stops the keymap from loading, naming its key (a reload keeps
  the running one). The default keymap has three: `Ctrl-A f` sends a break,
  `Ctrl-A h` pulses DTR (`dtr off; sleep 100; dtr on`), which resets most
  boards, and `Ctrl-A U` toggles the `u-boot` trigger. The help's Commands
  tab lists the keys bound to a line and every command.
- Typed text has the completions of the word before the cursor listed
  (command names with their help, then the values of the argument it is).
  A help too long for the list is cut, but the selected one's, which wraps.
  Tab, Shift-Tab, Up and Down put them in its place in turn; a lone one, or
  a click, is taken with a space after it. An empty line, or a line recalled
  from the history, lists nothing until Tab (which lists without picking)
  or typing.
- Without a list, Up and Down walk the lines that ran, as in the TX input:
  the newest first, back to the line being typed, only the ones starting
  with what was typed (`send` then Up walks the `send` lines).
  `history_size` in `[command_line]` (default `100`) keeps that many, for the
  session only; a reload applies at once.
- `layout` in `[command_line]` (default `"popup"`): `"popup"` draws it in a
  centered popup of one row, `"status_bar"` over the status bar's row, or in
  the popup when the status bar isn't shown.
- Its colors are `[command_line]` in `theme.toml`: `border` (the popup's and
  the list's), `prompt` (the `:`), `text` and `selected` (the completion
  picked in the list).

### TRIGGERS

A trigger runs a command line when a text comes in, as soon as it does: the
`Hit any key to stop autoboot` of U-Boot answered by a space, a login prompt
by a user name. They are in `triggers.toml`, a table each, named after it (the
default file has these two, disarmed):

```toml
[u-boot]
match = "Hit any key to stop autoboot"
run = "send ' '"

[login]
match = "login: "
run = 'send root\r'
rearm = true
```

- `match` is found in the received bytes as they come, not in whole lines
  (U-Boot redraws its countdown on the same line), even cut between two
  reads. It takes the escapes of the command line: `'\x1b[7mBoot menu'`.
- `run` is a command line (see [Command line](#command-line)), run as if
  typed, after a line still waiting.
- A trigger only fires while armed, and once: it disarms then, so the next
  boot goes on normally. `rearm = true` keeps it armed. They start disarmed,
  `armed = true` starts one armed.
- `Ctrl-A T` (`triggers`, also in the right-click menu) opens the trigger
  browser: the triggers and their state (`◎` armed, `○` not), the selected
  one's rule on the side. `j`/`k` or Up/Down move. Enter, Tab, Space, a
  click on the selected one or its button toggle it; `l`/Right arms it and
  `h`/Left disarms it. `a` toggles them all (armed when none is, else
  disarmed), `c` copies the file's path.
- `Ctrl-A U` toggles `u-boot`; `toggle-triggers` (no key by default) toggles
  them all; `trigger on|off|toggle [name]` does it from a command line or a
  key, so one key can arm and reset:
  `trigger on u-boot; dtr off; rts on; rts off`.
- The status bar shows `◎ 2` while two are armed (a click opens the
  browser). In a template, `{trigger_count}` is how many are armed,
  `{trigger_names}` their names (the first and how many more when the bar is
  short) and `armed` the condition. A notice says when one fires.
- Only live bytes fire them: never a replay, a restored session, nor what came
  in while the restore question was open.
- A rule that doesn't parse (an empty `match`, an unknown command, a misspelt
  field) stops the file from loading, naming the rule; a reload keeps the
  running ones. A reload applies the file, each trigger armed as it says.
  `triggers_path` in `config.toml` points to another file.

### TERMINAL

- `dimension_mode` in `[terminal_mode]`: the size of the emulated screen. The
  device doesn't learn it by itself (there is no resize signal on a serial
  line, see [Serial shells](#serial-shells)), so it only changes what can be
  shown, and an automatic resize never loses data:
  - `grow_only` (default)  Grows with the view, never shrinks. When the window
                         gets smaller, the view follows the cursor
  - `match_view`           The view's size, but never smaller than the content
                         below the cursor or the widest row
  - `fixed_cols`           The view's rows, but never fewer than the content
                         below the cursor; `cols` columns
  - `fixed`                `rows` x `cols`
  A size set from the resize popup applies as asked. The settings popup
  cycles the mode with `S` (Terminal Mode section).
- `answer_queries` in `[terminal_mode]` (default `true`): answer the queries a
  program on the device sends its terminal, like any terminal emulator does:
  the cursor position (how `resize` learns the size), the screen size, the
  device attributes, the mode states. Live data in terminal mode only, never a
  restored session or a replay. Colors, the title and the clipboard are never
  answered. The settings popup toggles it with `A`.
  - `answer_groups` (default all): `status`, `identity`, `size`, `modes`
  - `answerback` (default empty, no answer): the text sent back on ENQ
    (`0x05`). Off by default because `0x05` also shows up in binary data
- Scrolling: what is sent to the device (a key in terminal mode, a line on
  Enter in line mode) brings the view back to the live screen; what the device
  sends never moves a view scrolled up (`new_data` in the status bar).

#### SERIAL SHELLS

Over ssh or in a local terminal, the shell learns its size from the terminal,
and again on each resize (`SIGWINCH`). A serial line carries bytes only: the
device's tty keeps the size its getty set, often `0 x 0` or `24 x 80`, and
never hears about the window. Programs then wrap long lines at the wrong
column, and full-screen ones (vim, less, top, menuconfig) draw at the wrong
size.

maxicom never types a command into the device by itself. The device fixes its
own size with `resize`, which moves the cursor to the bottom right corner, asks
the terminal where the cursor is, and sets the tty to that size. maxicom
answers that query (`answer_queries`, on by default) with the emulator's size:
where its lines wrap, which `dimension_mode` decides.

- `resize` comes with BusyBox (most embedded Linux boards) and with xterm
  (`xterm` or `x11-utils` package on desktop distributions). It sets the tty
  and prints the matching `COLUMNS` / `LINES` assignments (BusyBox only when
  built with `FEATURE_RESIZE_PRINT`), so `eval "$(resize)"` also updates the
  current shell's variables.
- To fix the size at login and after each command, on serial ttys only (zsh,
  in the device's `~/.zshrc`):

  ```zsh
  if [[ $TTY == /dev/tty(S|AMA|USB|ACM|mxc|GS)* ]] &&
     (( $+commands[resize] )); then
    _serial_resize() { eval "$(resize)" }
    precmd_functions+=(_serial_resize)
  fi
  ```

  For bash, in `~/.bashrc`: `PROMPT_COMMAND='eval "$(resize)"'`, under the
  same kind of `tty` test. Each run costs one round trip on the line (a few
  bytes each way): on a very slow link, call `_serial_resize` once in place of
  the `precmd_functions` line, to fix the size at login only.
- By hand: Alt-R (`auto-resize-remote`) sends
  `stty rows R cols C; export LINES=R COLUMNS=C` with the emulator's size,
  the same size `resize` gets. In line mode Alt-R sends the columns only.
- With `answer_queries` off, or on a device without `resize`, the size stays
  wrong until Alt-R.

### CAPTURE

- What is what:
  - A session is a folder of its own, `<name>/<name>.raw` with its `.meta`
    and `.times`, plus a `.log` to read. The `.raw` holds the bytes as
    received, the `.times` when each chunk of them came, and the `.meta` what
    they were received on (device, line settings, encoding), so a session
    replays and restores as it was: both modes are rebuilt from the `.raw`,
    the `.log` is never read back.
  - A full recording is the same thing made by hand: the same files in a
    folder of your choice.
  - A text recording (`.log`) is for keeping and reading only: decoded,
    split in lines, times and marks added, it can't be replayed the same.
- Recordings: what is received, in any mode, to a file of your choice.
  - `text`  The received lines, decoded, one per line, after their time
            (`timestamp_format`) when `timestamp` is on. In line mode, the
            sent lines too, after `tx_mark`
  - `full`  A folder like a session's: the received bytes as is,
            replayable with `--replay <folder>`, their times, the
            text, and a `.meta` so the replay decodes with the encoding and
            shows the line settings it was recorded with. During a replay,
            the replayed session's (no `.meta` for a file without one). With
            the session included, every file starts with the session's and
            the `.meta` when the session did. It stops, with a notice, when
            the device changes (another port, baud, framing or encoding, a
            replay starting or ending)
  One recording at a time (a full one has the text too). `record`
  (`Ctrl-A l`, a click on `● REC`, or `Record…` in the right-click menu)
  stops it, or opens the recording dialog on the mode's stream (text in line
  mode, full in terminal mode).
- The recording dialog: the name is prefilled, without an extension: the
  stream sets it, shown after the name (`.log`, or `/` for the folder).
  Enter starts.
  - `Tab`        Switch the stream
  - `Shift-Tab`  Include since session start: the recording starts with
                 everything the session backup holds, then keeps recording
- Session backup: every session is recorded, raw and text, to a folder
  `~/.cache/maxicom/sessions/<date>_<time>_<port>/` (`.log`, `.raw`, `.times`
  and a small `.meta` with the device, the text format and the terminal grid's
  size, all named like the folder), the time to the millisecond.
  Flushed every 250 ms, so a crash or a `kill` loses almost nothing. It never
  lights `● REC`. Not in replay mode.
  - Its text is written with `timestamp`, `timestamp_format`, `record_tx` and
    `tx_mark` as they are when the session starts; a reload changes the
    recordings only, never a running backup.
  - When a session starts, the sessions that received nothing are removed,
    then the oldest ones until `sessions.keep` are left and all the sessions
    together (all their files) fit `sessions.max_size`. The
    current session and one still running, here or in another maxicom
    (another port), count in the size but are never removed.
  - To recover a session by hand, open its `.log`, or replay it (its
    `.meta` gives the encoding and the line settings):
    `maxicom --replay ~/.cache/maxicom/sessions/20261028_141500123_ttyUSB0`
- A replay (`--replay`, `replay`, `open file://`) takes a recording's
  folder, its `.raw` or its `.meta`, or any `.raw` (another tool's capture,
  played without a meta). A `.log` is refused: its text isn't what was
  received.
  - A layout can show it with the `session` condition, e.g.
    `[◆](session?muted:hide)` (not in the default layout).
- Restore: at startup, the last session of the same device can come back in
  the history, the lines in line mode and the screen and scrollback in
  terminal mode, then a separator line
  `── restored: ttyUSB0, 2026-10-28 14:15 → 16:40 ──` before the live data.
  The terminal replays the session at the grid size it ended on, so its wide
  lines come back whole; then `dimension_mode` sizes it to the view.
  - `restore` in `[sessions]`: `always` (default) restores, `ask` asks at
    startup, `never` doesn't. `--restore` (restore without asking) and
    `--no-restore` decide for one launch. Never in replay mode.
  - The same device: the same machine (hostname), port, baud, data bits,
    parity, stop bits and encoding. The session must have received something,
    not be running, and have ended within `restore_max_age` (`"0s"`: no
    limit).
    Sessions from before the restore have no `.meta` and never come back.
  - The question: Enter or `y` restores, Esc or `n` skips; a click outside
    doesn't answer. What arrives meanwhile waits and comes after the
    restored part (up to 4 MiB, then the restore is skipped).
  - What comes back: the last 1 MiB of the `.raw`, in both modes, cut in
    lines as line mode did live (the lines sent in line mode aren't received:
    they don't come back), each line and row at the time from the `.times`
    (a session without one shows a blank time column).
  - After the terminal part, the modes a full-screen program may have left
    (alternate screen, cursor keys, mouse reports, scroll region) are reset.
  - The restored part isn't backed up again nor counted in RX, so the next
    launch restores this session's own data (or the same one again if this
    one received nothing).
- Session browser: `sessions` (`Ctrl-A j`, or Session backups in the
  right-click menu) lists every backup on the left, whatever its device, and
  shows the selected one on the right (device, format, times, size, its last
  lines).
  - Marks: `●` running (can't be deleted), `↺` what this device would
    restore, `?` no `.meta` (from before the restore)
  - `↑` `↓` `j` `k` `PageUp` `PageDown` `Home` `End`, or a click, select
  - `d` (or `Del`)  Delete the selected session
  - `D`             Delete all but the running ones, asked first (`y` / `n`)
  - `s`             Sort by date (newest first) or by device
  - `Esc`           Close
  A keymap file from before has the old prefix keys (MAX-181 moved them to
  minicom's): delete `keymap.toml` to get the new defaults.
- Screenshots (terminal mode): `screenshot-bytes` (`Ctrl-A k`) and
  `screenshot-text` (no default key, Screenshot in the right-click menu) take
  the screen at once, in both formats, then a dialog asks where to save it.
  Enter saves to the prefilled path, Esc drops it. In line mode they only say
  so.
  - `Tab`  Switch the format: `text` (`.txt`, the characters) or `ansi`
           (`.ans`, with the colors). The key's format is selected first
- Paths: a leading `~` is the home folder, missing folders are created, an
  empty dir is the current one. Names are strftime templates, formatted when
  the dialog opens.
- In `config.toml`, with the defaults. `[capture]` is the recordings, its
  text settings also the session backup's; `[sessions]` the session backup
  and the restore; `[screenshot]` the screenshots:

```toml
[capture]
dir = ""                                 # recordings folder
name = "maxicom_%Y%m%d_%H%M%S"           # no extension: .log, or a folder
timestamp = true                         # time on each text line
record_tx = true                         # sent lines in the text files (line mode)
tx_mark = "> "                           # before each sent line, "" for none

[sessions]
enabled = true                           # session backup (read at startup)
keep = 200                               # sessions kept
max_size = "200M"                        # all sessions, K/M/G/T, "0": no limit
restore = "always"                       # always, ask or never
restore_max_age = "24h"                  # since it ended, "0s": no limit

[screenshot]
dir = ""
name = "screen_%Y%m%d_%H%M%S"            # the format adds .txt or .ans
```

### THEME

- `theme.toml` styles the UI: an entry is a color (its `fg`), or a table of
  `fg`, `bg`, `underline_color` (a name like `White`, `DarkGray`, `Reset`, or
  `#RRGGBB`) and `bold`, `dim`, `italic`, `underlined`, `reversed`, `crossed`.
  The copy mode selection, in the text and the raw side panel, is
  `[ui.selection]` (default black on `Magenta`, as the `COPY` badge);
  the cursor's ghost in the other panel is `[ui.cursor_ghost]`,
  the part under the mouse `[ui.hover]`.

#### STATUS BAR FORMAT

- Syntax:
  - `plain text`                  Base theme style (`status_bar`), on the UI
                                background unless the layout is `band`
  - `{variable}`                  Dynamic runtime value
  - `[content](style)`            Styled text span
  - `[content](cond?true:false)`  Conditional style
  - `(style1 style2)`             Group multiple styles with spaces
  - `\` at the end of a line      Joins the next line, its indentation dropped

- Several lines: a template can be written one block per line, each line but
  the last ending with `\`. In a `'''` string TOML keeps the `\` for the
  status bar; in a `"""` string TOML joins the lines itself. The default
  config file is written that way:

```toml
right = '''
[{scroll_pos_up} ](has_scroll?muted:hide prio=2)\
    [RX: {rx_bytes:4h}](muted prio=3)\
    [ | ](muted prio=3)\
    ...'''
```

  A space before the `\` stays, as between `[{time}](...)` and `[?](...)`.

- Variables:
  - {mode}           "TERM" or "LINE"
  - {port}           Device address (e.g. /dev/ttyUSB0, tcp://192.168.1.50:4000)
  - {baud}           Baud rate (e.g. 115200), raw for a raw TCP link
  - {framing}        Serial framing (e.g. 8N1), TCP for a TCP link
  - {rx_bytes}       Received bytes count
  - {tx_bytes}       Transmitted bytes count
  - {time}           Local time (via time_format)
  - {pan_left}       Left pan amount (`◀ (3)`)
  - {pan_right}      Right pan amount (`!(2) ▶`)
  - {pan_offset}     Columns panned from the left edge (`3`)
  - {pan_max}        Columns the view can pan in all (`117`)
  - {scroll_pos}     Position: "All", "Bot", "Top", "42%"
  - {scroll_pos_up}  Uppercase position: "ALL", "BOT", "TOP"
  - {scroll_percent} Numeric percent: 0..100
  - {scroll_offset}  Lines scrolled away from tail
  - {scroll_max}     Total scrollback depth
  - {send_file}      The file `send-file` sends, empty when none
  - {send_percent}   How much of it is sent: 0..100

- Formats: `{name:spec}`, spec `[<>^][0][width][h]`
  - `{rx_bytes:8}`    Pad to 8 columns (numbers right, text left)
  - `{port:<12}`      Align with `<`, `>` or `^` (center)
  - `{rx_bytes:08}`   Pad a number with zeros
  - `{rx_bytes:4h}`   Short size with a unit letter (`1.0K`, `12K`, `1.2M`), 4 columns
  A longer value is never cut; a malformed spec stays as written.

- The pan shows once: in the bar when a template has a pan variable, else as
  arrows in the history's bottom border. The default center part is
  `◀ offset/max ▶`, padded so it doesn't move while panning.

- Conditions:
  - Modes & Focus:
    - prefix         Prefix key (Ctrl-A) is active
    - rec            File capture is active (alias: logging)
    - session        Session backup is running (alias: backup)
    - line           Line input active (alias: linemode)
    - term           Terminal mode active (alias: terminal)
    - copy           Scrollback mode active (alias: copymode)
    - search         Search mode is active
    - overlay        Dialog/popup is open (alias: popup)
    - focused        Window has OS desktop focus
    - line_settings  The link has line settings: a serial port, not a TCP
                     link whose bridge sets them (alias: serial)
    - sending        A file is being sent (alias: send_file)

  - Viewport & Scroll:
    - is_live        Following live tail (alias: live, bot, bottom)
    - is_top         Scrolled to top of buffer (alias: top)
    - is_scrolled    Viewing past buffer history (alias: scrolled, paused)
    - scroll_up      History available above (alias: can_scroll_up)
    - scroll_down    History available below (alias: can_scroll_down)
    - has_scroll     Buffer exceeds visible screen height

  - Horizontal Pan:
    - panned         Viewport is panned (alias: pan, panning)
    - pan_left       Can pan left (alias: can_pan_left)
    - pan_right      Can pan right (alias: can_pan_right)
    - pan_alert      Unread content to right (alias: pan_right_alert)
    - wide           Content wider than the view (alias: pannable)
    - new_data       Data arrived while scrolled up (aliases: unread, missed)

- Styles:
  - Colors:
    black, red, green, yellow, blue, magenta, cyan, gray,
    darkgray, white, or hex (#RRGGBB)
  - Specifiers:
    `fg=<color>`, `bg=<color>`
  - Modifiers:
    bold, dim, italic, underline, reverse, crossed,
    sblink (slow blink), rblink (rapid blink), reset
  - Visibility:
    hide, hidden, none
  - Mouse:
    `click=<action>` runs a keymap action (e.g. `help`, `settings`, `quit`,
    `record`) when the block is clicked
    `hover=<style>` styles the block under the mouse pointer, e.g.
    `hover=(bg=main fg=black)`, or `hover=none`; blocks without it take
    `hover` in `[ui.status_bar]` (default "reversed", "none" for no hover)

- When it doesn't fit: `prio=N` on a block (`1` most important). The bar
  tries the compact forms (`{port}` as `ttyUSB0`, byte counts as `1.2K`, the
  pan as `◀3`), then without the blocks of the highest `prio`, the next one,
  and so on (the same `prio` goes together: give a separator its block's),
  then without the center, then the right part cut from its left. Blocks
  without `prio=` and plain text always stay.

- Clicks: a block without `click=` runs what its first variable implies:
  - {mode}                       Switch between LINE and TERM
  - {baud}, {framing}            Open the link settings ({port} does nothing)
  - {copy_side}                  Switch the copy cursor's panel
  - {pan_left}, {pan_right}      Pan left / right
  - {pan_offset}, {pan_max}      Back to the left edge
  - {send_file}, {send_percent}  Stop sending the file
  - {scroll_pos}, {scroll_pos_up}, {scroll_percent}, {scroll_offset}
                                 Back to the live tail
  A variable in plain text (outside `[...]`) is clickable on its own.
  The scroll and pan actions a `click=` can run: `scroll-up`, `scroll-down`,
  `page-up`, `page-down`, `scroll-to-top`, `scroll-to-bottom`, `pan-left`,
  `pan-right`, `pan-home` (back to the left edge). They move the view only,
  in copy mode too: the copy cursor stays. The steps (`scroll-up`,
  `scroll-down`, `page-up`, `page-down`, `pan-left`, `pan-right`) repeat while
  the button is held on their block, until the pointer leaves it. The wheel
  over a block that pans pans the history. A custom scroll block:
  `[▲](click=page-up)[{scroll_percent:>3}%](muted)[▼](click=page-down)`
  The history box's own indicators are clickable too: its bottom border's
  `◀ (n)` / `(n) ▶` pan, its title's `SCROLL +n` goes back to the live tail
  and `PAN +n` to the left edge.

- Layout: `layout` in `[ui.status_bar]`
  - `line`             A row of its own, plain text on the UI background
  - `band`             A row filled with the `status_bar` bg
  - `border` (default) In the bottom border of the last box (TX box in line
                     mode, history in terminal mode, where a search takes
                     its place while open)
  - `box`              In a box of its own, 3 rows high
  `border` and `box` need borders, else they are `line`. With borders on,
  `line` and `band` are inset one column each side.

### Author

Pierre Mollard
