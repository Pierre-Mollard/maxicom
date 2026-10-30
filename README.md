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
cargo run --bin maxicom --profile profiling -- --baud 115200 --port /tmp/maxicom-shell-test
```

- Run the bin utility "fake_device" for testing dumb programs

```bash
./scripts/run_fake.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baud 115200 --port /tmp/maxicom
```

- Run a device over TCP (`tcp://` links): a shell on `127.0.0.1:4000`, one per
  connection, or a real serial port shared like ser2net

```bash
./scripts/run_tcp.sh                            # zsh on port 4000
./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200   # bridge a serial port
```

```bash
cargo run --bin maxicom -- --port tcp://127.0.0.1:4000
```

- Profile (flame chart) with sample

```bash
cargo build --bin maxicom --profile profiling
```

```bash
samply record ./target/profiling/maxicom --baud 115200 --port /tmp/maxicom-shell-test
```

- Check license compat

```bash
cargo deny check
```

## DEVICES

`maxicom [OPTIONS] [DEVICE]`, all the options with `maxicom --help`.

- A serial port: `maxicom /dev/ttyUSB0 -b 921600`, `maxicom COM3`, or with
  `--port`.
- A device over TCP: `maxicom tcp://192.168.1.50:4000`, or
  `--tcp 192.168.1.50:4000`.

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
  the serial settings popup shows the address only, and `--baud`, `--bits`,
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

### UI

- `title_details` in `[ui]` (default `false`): the history box's title shows
  only ` maxicom LINE ` or ` maxicom TERMINAL `. With `true` it also shows the
  alerts (`!`, `● REC`), the VT and view sizes, the scroll, the pan and the
  prefix. What doesn't fit in the border is left out whole, never cut. A click
  on `SCROLL +n` goes back to the live tail, on `PAN +n` to the left edge.
- `compact_cols` and `compact_rows` in `[ui]` (default `50` and `14`): while
  the screen is narrower or shorter than that, it is drawn without borders,
  status bar and scrollbar, to leave the room to the content. The settings
  stay as they are: a bigger window brings them back. `0` turns a side off.

### COMMAND LINE

- `Ctrl-A :` (`command-line`) opens a `:` prompt that runs commands: the
  names of `keymap.toml` (`toggle-local-echo`, `record-raw`, ...), chained
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
    settings, as the serial settings popup does. On a detached port they are
    kept for when it opens.

  These six need a serial port: a TCP link (its bridge owns the lines and the
  settings) and a replay refuse them.
  - `open <port>`: switches to another serial port or `tcp://host:port`, the
    settings kept (`open /dev/ttyUSB1`, `open tcp://192.168.1.50:4000`). The
    path completes from the disk. The rest of the line waits until it is
    open: `open COM4; send AT\r`. A port that fails to open brings the last
    one back. Another port starts a new session backup, as they go by port.
    `detach-serial` detaches and attaches the port, as `Ctrl-A h` does.
  - `set <setting> on|off`: `local-echo`, `timestamps`, `mouse` or
    `answer-queries`; nothing when it is so already (the settings with more
    values cycle with their keys).
  - `encoding utf8|ascii|cp437|latin1`: how the received bytes are read.
  - `search <text>`: opens the search bar on the text, its matches shown.
  - `screenshot <name>`: saves the terminal at once, without the dialog, in
    `screenshot_dir` for a bare name: `.ans` keeps the colors, else text
    (`.txt` added to a name without an extension). Terminal mode only.
  - While the port isn't open (detached, lost, reconnecting), `send`,
    `send-file` and the control lines refuse; `baud`, `framing` and `flow`
    are kept for when it opens.
  - `sleep <ms>`: the rest of the line waits that long (up to a minute), the
    screen stays live. A line run meanwhile waits behind it.
  - `cancel`: stops the file being sent and drops the commands waiting. A line
    starting with it runs it at once, not behind them: `cancel; send x`.
    An ESP32 into its bootloader, as esptool does it:
    `dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off`

  A command the link can't do (anything sent on a replay) stops the line
  there, with the error.
- The word before the cursor has its completions listed (command names with
  their help, then the values of the argument it is). Tab and Shift-Tab put
  them in its place in turn, then Up and Down too; a lone one, or a click, is
  taken with a space after it.
- Otherwise Up and Down walk the lines that ran, as in the TX input: the
  newest first, back to the line being typed. `history_size` in
  `[command_line]` (default `100`) keeps that many, for the session only; a
  reload applies at once.
- `layout` in `[command_line]` (default `"popup"`): `"popup"` draws it in a
  centered popup of one row, `"status_bar"` over the status bar's row, or in
  the popup when the status bar isn't shown.
- Its colors are `[command_line]` in `theme.toml`: `border` (the popup's and
  the list's), `prompt` (the `:`), `text` and `selected` (the completion
  picked in the list).

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

- Recordings: what is received, in any mode, to a file of your choice.
  - `text`  The received lines, decoded, one per line, after their time
            (`timestamp_format`) when `timestamp` is on. In line mode, the
            sent lines too, after `tx_mark`
  - `raw`   The received bytes as is, replayable with `--replay <file>`
  Text and raw record apart, both at once if you want. `toggle-logging`
  (`Ctrl-A l`, or a click on `● REC`) stops every recording, or opens the
  recording dialog on the mode's stream (text in line mode, raw in terminal
  mode). `record-text` and `record-raw` (no default keys, also in the
  right-click menu) start or stop one stream.
- The recording dialog: the path is prefilled, Enter starts.
  - `Tab`        Switch the stream (swaps the default path unless edited)
  - `Shift-Tab`  Include since session start: the file starts with everything
                 the session backup holds, then keeps recording
- Session backup (journal): every session is recorded, text and raw, to
  `~/.cache/maxicom/sessions/<date>_<time>_<port>` (`.log`, `.raw` and a small
  `.meta` with the device and the text format), the time to the millisecond.
  Flushed every 250 ms, so a crash or a `kill` loses almost nothing. It never
  lights `● REC`. Not in replay mode.
  - Its text is written with `timestamp`, `timestamp_format`, `record_tx` and
    `tx_mark` as they are when the session starts; a reload changes the
    recordings only, never a running backup.
  - At startup, the sessions that received nothing are removed, then the
    oldest past `journal_keep`. A session still running, here or in another
    maxicom (another port), is never removed.
  - To recover a session by hand, open its `.log`, or replay its `.raw`:
    `maxicom --replay ~/.cache/maxicom/sessions/20261028_141500123_ttyUSB0.raw`
  - A layout can show it with the `journal` condition, e.g.
    `[◆](journal?muted:hide)` (not in the default layout).
- Restore: at startup, the last session of the same device can come back in
  the history, the lines in line mode and the screen and scrollback in
  terminal mode, then a separator line
  `── restored: ttyUSB0, 2026-10-28 14:15 → 16:40 ──` before the live data.
  - `restore` in `[capture]`: `ask` (default) asks at startup, `always`
    restores, `never` doesn't. `--restore` (restore without asking) and
    `--no-restore` decide for one launch. Never in replay mode.
  - The same device: the same machine (hostname), port, baud, data bits,
    parity, stop bits and encoding. The session must have received something,
    not be running, and have ended within `restore_max_age` (`"0s"`: no
    limit).
    Sessions from before the restore have no `.meta` and never come back.
  - The question: Enter or `y` restores, Esc or `n` skips; a click outside
    doesn't answer. What arrives meanwhile waits and comes after the
    restored part (up to 4 MiB, then the restore is skipped).
  - What comes back: the last `line_mode.history_size` lines of the `.log`,
    with their time read with the session's format (a line whose time can't
    be read, or a session without times, shows a blank time column), and the
    last 1 MiB of the `.raw`. A line starting with the session's `tx_mark`
    comes back as a sent line, so a mark that device output never starts with
    is best.
  - After the terminal part, the modes a full-screen program may have left
    (alternate screen, cursor keys, mouse reports, scroll region) are reset.
  - The restored part isn't backed up again nor counted in RX, so the next
    launch restores this session's own data (or the same one again if this
    one received nothing).
- Session browser: `sessions` (`Ctrl-A S`, or Session backups in the
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
  A keymap file from before has no `S` binding: add `S = "sessions"` under
  `[prefix_actions]` in `keymap.toml`, or delete the file to get the new
  defaults.
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
- `[capture]` in `config.toml`, with the defaults:

```toml
[capture]
dir = ""                                 # recordings folder
text_name = "maxicom_%Y%m%d_%H%M%S.log"
raw_name = "maxicom_%Y%m%d_%H%M%S.raw"
timestamp = true                         # time on each text line
record_tx = true                         # sent lines in the text files (line mode)
tx_mark = "> "                           # before each sent line, "" for none
journal = true                           # session backup (read at startup)
journal_keep = 20                        # sessions kept
restore = "ask"                          # ask, always or never
restore_max_age = "24h"                  # since it ended, "0s": no limit
screenshot_dir = ""
screenshot_name = "screen_%Y%m%d_%H%M%S" # the format adds .txt or .ans
```

### THEME

#### STATUS BAR FORMAT

- Syntax:
  - `plain text`                  Base theme style (`status_bar`), on the UI
                                background unless the layout is `band`
  - `{variable}`                  Dynamic runtime value
  - `[content](style)`            Styled text span
  - `[content](cond?true:false)`  Conditional style
  - `(style1 style2)`             Group multiple styles with spaces

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
    - journal        Session backup is running (alias: backup)
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
    `toggle-logging`) when the block is clicked
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
  - {baud}, {framing}            Open the serial port settings ({port} does nothing)
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
