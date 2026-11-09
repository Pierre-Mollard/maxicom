# Quick start

A tour of the ten things you'll do most, with a board plugged in over USB.

## 1. Open the port

```bash
maxicom /dev/ttyUSB0             # COM3 on Windows
maxicom /dev/ttyUSB0 -b 921600   # another baud rate
```

The default is 115200 8N1. With no port given, maxicom opens on the
directory: pick a port with the arrows and press Enter.

## 2. Type

You start in terminal mode: your keys go to the device, as in any terminal.
The status bar shows the mode (`TERM`), the port, the baud rate and the
RX / TX counts.

## 3. Switch to line mode

Press `Ctrl-a m`. The output now shows as lines, with an input box at the
bottom. Type a line and press Enter to send it, followed by LF (`tx_eol`
changes that). Up and Down walk through the lines you sent. `Ctrl-a m` again
goes back to terminal mode.

## 4. Scroll back

Use the mouse wheel or `Shift-PageUp`. New data doesn't move a view you
scrolled. `Shift-End`, or typing, takes you back to the live output.

## 5. Copy

Drag with the mouse: the text is copied when you release the button.

Or press `Ctrl-a [` for copy mode: move with the arrows or `h j k l`, press
`v` to start a selection, `y` to copy it, Esc to leave.

A right click where you type pastes the copy. See
[Copy mode and mouse](copy-mode-and-mouse.md).

## 6. Search

Press `Ctrl-a ?` and type: the matches light up, the newest one first.
Ctrl-n and Ctrl-p jump from one to the next. Enter leaves you in copy mode on
the match, where `n` and `N` go on through the matches. `Ctrl-a /` searches
from the oldest line instead.

## 7. Record

Press `Ctrl-a l`, then Enter to start a recording under the suggested name.
`● REC` shows in the status bar; `Ctrl-a l` again stops it.

You don't have to remember: every session is also backed up on its own, and
comes back at the next launch on the same port. See [Capture](capture.md).

## 8. Reset the board

`Ctrl-a h` pulses DTR, which resets most boards. `Ctrl-a f` sends a break.

## 9. Save the link

Press `Ctrl-a d` to open the directory, then `s` to save the port in use with
its line settings under a name. Next time, `maxicom <name>` opens it. See
[Saved links](saved-links.md).

## 10. Quit

Press `Ctrl-a q` (or `Ctrl-q`), then `y`. Pressing `Ctrl-q` twice also
quits.

## Lost?

`Ctrl-a z` lists every key, and a right click opens a menu of the main
actions.

## Next steps

| If | Read |
| --- | --- |
| The text looks wrong: staircase lines, lines written over each other, odd characters | [Encoding and newlines](encoding-and-newlines.md) |
| A shell on the device wraps at the wrong column, or vim draws at the wrong size | [Serial shells](terminal-mode.md#serial-shells) |
| You want one key to put an ESP32 in its bootloader, or to stop U-Boot's autoboot | [Command line](command-line.md) and [Triggers](triggers.md) |
| You want other keys, colors or status bar | [Keybindings](keybindings.md), [Theme](theme.md), [Status bar](status-bar.md) |
