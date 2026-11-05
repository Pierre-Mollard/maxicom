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
`Ctrl-a e` (line mode).

### The device wants Enter as CR LF

In line mode, set `tx_eol = "crlf"`, or press `N` in the settings popup.
Terminal mode sends CR, as terminals do.

### `Ctrl-a` is taken

tmux, screen, or the device itself may need `Ctrl-a`. Press it twice to send
it, or change `prefix` in `keymap.toml` (`prefix = "ctrl-b"`). See
[Keybindings](keybindings.md#the-prefix).

### A key I bound doesn't work

The help (`Ctrl-a ?`) shows the keymap in use. If the file didn't load,
maxicom named it at startup, or the reload's popup did.

### vim or top draws at the wrong size, the shell wraps at the wrong column

The device doesn't know the window's size. See
[Serial shells](terminal-mode.md#serial-shells).

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
