# Troubleshooting

This page starts from what you see. To look up the exact words of a warning
or an error, see [Messages](messages.md).

## The port doesn't open

### Permission denied (Linux)

Join the port's group, then log out and back in:

```bash
sudo usermod -aG dialout "$USER"     # uucp on Arch
```

See [Install](install.md#permissions).

### Held by another program

maxicom names the program: `held by picocom (1234)`. Close it. If it's
another maxicom, detach the port there with `Ctrl-a D`. See
[Port locks](locks.md).

On Windows, a port opens in one program only, and maxicom can't always tell
which one has it. Close the serial monitors (Arduino IDE, PuTTY) first.

### No such port

The name depends on the adapter and the system: `/dev/ttyUSB0`,
`/dev/ttyACM0`, `COM3`, `/dev/cu.usbserial-XXXX`. Open the
[directory](directory.md) (`Ctrl-a d`, or `maxicom` alone) to see the ports
the system lists. After plugging a board, `r` lists them again.

### WSL2 sees no port

Forward the adapter with usbipd-win, or use a TCP bridge. See
[Install](install.md#wsl2).

## The text is wrong

### Garbage characters

The baud rate doesn't match the device's. Try the usual ones (115200, 9600,
921600) with `Ctrl-a p`, then check the framing (almost always 8N1).

### Lines go down like stairs

Each line starts where the previous one ended: the device ends its lines
with LF only. Turn on add CR with `Ctrl-a u`. See
[Encoding and newlines](encoding-and-newlines.md#received-newlines).

### Lines overwrite each other

The device ends its lines with CR only. Turn on add LF with `Ctrl-a a`.

### Odd accented letters or boxes

The encoding doesn't match. `Ctrl-a O` cycles through utf8, ascii, cp437
(old BIOS and DOS screens) and latin1. See
[Encoding and newlines](encoding-and-newlines.md#text-encoding).

### Escape codes shown as text

You see things like `^[[32m` in line mode: the display mode is `raw`. `D`
in the settings popup cycles back to `pretty`.

### Nothing shows, but the RX count grows

- Check the mode: with `show_partial_rx` off, line mode hides a line until
  it ends.
- Check you're not scrolled up: `Shift-End` goes back to the live output.

### Nothing is received at all

- The TX and RX wires may be swapped.
- The board may wait for flow control. `flow = "none"` is the default:
  check that a saved link or `--flow` didn't turn RTS/CTS on.

## Keys and the device

### The board resets when maxicom opens the port

Opening a port raises DTR and RTS, which resets boards with an auto-reset
circuit (ESP32, Arduino). Every terminal does it, and it's often what you
want.

### What I type doesn't show

The device doesn't echo what it receives. Turn on local echo with
`Ctrl-a e`, in either mode.

### The device wants Enter as CR LF

In line mode, set `tx_eol = "crlf"`, or press `N` in the settings popup.
Terminal mode sends CR, as terminals do.

### `Ctrl-a` is taken

tmux, screen, or the device itself may need `Ctrl-a`. Press it twice to send
it, or change `prefix` in `keymap.toml` (`prefix = "ctrl-b"`). See
[Keybindings](keybindings.md#the-prefix).

### A key I bound doesn't work

The help (`Ctrl-a z`) shows the keymap in use. If the file didn't load,
maxicom named it at startup, or the reload's popup did.

### vim or top draws at the wrong size, the shell wraps at the wrong column

The device doesn't know the window's size. See
[Serial shells](terminal-mode.md#serial-shells).

## Escape sequences and binary data

maxicom reads what the device sends as a terminal does: besides text, there
are control bytes and escape sequences, and some of them are questions it
[answers](terminal-mode.md#answering-the-devices-queries). Most of the time
that's what you want. With binary data, or a program that does something
unusual, these entries help.

### A program on the device doesn't look right

A screen drawn wrong, a color or a mode that doesn't take, a program that
seems to wait for something: maxicom may not handle a sequence it sends.
List the groups to watch in `notify_unhandled`, in `[terminal_mode]`, and
reload with `Ctrl-a r`:

```toml
notify_unhandled = ["controls", "c1", "escape", "csi", "osc", "strings"]
```

A notification then names what came in and wasn't handled, with how many
times: `Unhandled: CSI ?2027h ×3, OSC 52`. Run the program again and watch
it. The groups are explained in
[What the terminal doesn't handle](terminal-mode.md#what-the-terminal-doesnt-handle),
and [What maxicom doesn't do](limits.md) lists what isn't supported on
purpose. Set it back to `[]` when you're done.

### The unhandled notification never stops

Counts like `NUL ×95526` and a long list of kinds mean the device sends
binary data, or the baud rate is wrong and the text arrives as garbage
(check it with `Ctrl-a p`). The notification is right, but it isn't useful
there: set `notify_unhandled = []`, or keep only the groups you're looking
for.

### The notification shows OSC 7, OSC 133 or ?2026 on every prompt

Those are from your shell or your programs, and harmless when ignored: the
current directory, prompt marks, synchronized drawing. They're in the
`shell` group: take it out of `notify_unhandled`.

### The notification shows nothing

- `notify_unhandled` is empty by default: nothing is counted.
- Only what the device sends counts. A restored session doesn't, nor what
  maxicom writes itself, like the local echo.
- What maxicom answers or uses isn't unhandled: the queries of the groups in
  `answer_groups`, the device's title, its clipboard copies.
- In line mode, the bytes reach the terminal only while both modes are fed,
  the default `dual_mode_buffering = "always_both"`.

### Characters appear at the device's prompt by themselves

Something like `^[[0n`, `1;1R` or `^[[?62;22c` shows at the prompt, or a
command fails on input you didn't type. Those are maxicom's answers to
queries: a program asked and quit before reading the reply, or the device
sent binary data where a few bytes read like a query.

Press `A` in the settings popup (`Ctrl-a o`) to switch the answers off for
the session, or empty `answer_groups` in `[terminal_mode]` to turn them off
for good. To keep the useful ones, drop only a group: `status` and `size`
answer the cursor and size queries `resize` needs. `enq_answer` is sent on
each `0x05` byte: leave it empty with binary data.

### Before sending binary data by hand

A file sent as it is (`send-file`), or bytes pasted for a bootloader that
waits for an upload, go to a device that may echo them back. maxicom then
reads that echo as it reads everything:

- answers to the queries it finds in it go to the device, in the middle of
  your data: switch them off with `A` in the settings popup first, and back
  on after;
- keep `notify_unhandled` empty, or it counts the whole file;
- keep `device_clipboard` off (the default), or a few bytes reading like a
  copy could replace your clipboard.

An [XMODEM or YMODEM upload](file-transfers.md#uploading-with-xmodem-or-ymodem)
doesn't need this: the answers wait while the transfer runs.

### A program waits a second before it starts

vim, a shell's line editor or `resize` ask the terminal a question and wait
for the reply, then give up after a timeout. With the answers off, or their
group left out of `answer_groups`, they wait every time. Switch them back on
with `A` in the settings popup, or put the groups back.

## Copying and the screen

### A copy doesn't reach the clipboard

maxicom copies through the terminal (OSC 52). In tmux, add
`set -g set-clipboard on`; in other terminals, check that clipboard access
is allowed. See
[Copy mode and mouse](copy-mode-and-mouse.md#where-the-copy-goes).

### I can't select with my terminal's mouse

maxicom has the mouse. Turn it off with `Ctrl-a M`, or hold Shift while
selecting, which works in most terminals.

### Borders or the status bar are gone

The window is smaller than `hide_borders_below` or `hide_status_bar_below`.
See [Small windows](settings.md#small-windows).

### Leftover characters on the screen

Pieces of old lines stay after a scroll, characters land on the borders, or a
row is shifted, most often after binary data. Press `Ctrl-a R` to draw the
whole screen again. Some characters are counted at different widths by your
terminal and maxicom, see
[Characters drawn differently](drawn-characters.md).

### Lines or glyphs drawn wrong on Windows

Use Windows Terminal.

### The history is gone after a restart

The last session only comes back on the same device with the same line
settings, and within `restore_max_age` (1 day by default). See
[Restore](capture.md#which-session-comes-back).

## Config files

### maxicom stops at startup, naming a file

That file doesn't parse, and the message says where. Fix it, or delete it to
get its defaults back.

### A change doesn't apply

`Ctrl-a r` reloads the files. A few settings need a restart, and the reload
names them. See [Reload](settings.md#reload).

### Back to the defaults

Delete the file (`keymap.toml`, `theme.toml`...). maxicom writes it again at
the next start.
