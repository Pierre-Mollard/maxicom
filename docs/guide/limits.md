# What maxicom doesn't do

This page lists what maxicom leaves out on purpose, or doesn't support yet,
with what to use instead. If a feature isn't in this guide, it's most likely
not there; check here before looking for a hidden setting.

## It never acts on its own

### It never types into the device

maxicom sends only what you type, what you bind to a key, and what you arm
as a trigger. It never sends a command by itself, not even to fix the
terminal size: the device runs `resize` and asks for it. See
[Serial shells](terminal-mode.md#serial-shells).

The one exception is answering the questions a program on the device asks
its terminal, like "where is the cursor?" or "what size are you?". A program
can't see its terminal, so it writes a query and waits for the reply: that's
how `resize` learns the size. maxicom answers these queries by default
(`answer_queries = true` in `[terminal_mode]`, with every group in
`answer_groups`), only live and only in terminal mode.

A program can also ask to be told the size each time it changes, and maxicom
then sends it on every resize: see
[Size reports from programs](terminal-mode.md#size-reports-from-programs).
That's still an answer, to a program that asked, not a command typed into
the device.

You can turn the answers off, all of them or by group. See
[Answering the device's queries](terminal-mode.md#answering-the-devices-queries).

### It never starts another program

There is no shell escape, no external editor, no helper program for file
transfers, and no command run on an event. XMODEM and YMODEM are built in.
To use another program on the port, detach the link first (`Ctrl-a D`), see
[Detach and attach](links.md#detach-and-attach).

### It doesn't retry a failed reattach

When `Ctrl-a D` can't open the port again, the link stays detached until you
try again. A port that was unplugged, or a TCP host that went down, is
different: maxicom reopens those by itself.

## Links

### One link per window

A maxicom shows one link at a time, with no tabs or split views. To watch
two devices, run two maxicoms. They can't open the same port: see
[Port locks](locks.md).

### No telnet, RFC 2217 or SSH

A TCP link carries raw bytes only. There is no telnet option negotiation,
and no RFC 2217, so the baud rate of a remote port can't be set from
maxicom. To reach a device behind SSH, forward a port with `ssh -L`. See
[TCP links](tcp-links.md).

### No modem dialing

The directory opens ports and saved links. It doesn't dial phone numbers or
send modem init strings. A [named command](keybindings.md#named-commands) on
a key can send any string, see [Command line](command-line.md).

## Sending

### No ZMODEM or Kermit

`upload` and `download` move files with XMODEM, XMODEM-1K or YMODEM, which
covers U-Boot, most bootloaders and lrzsz's `sb` / `sx`. There is no ZMODEM or
Kermit: for those, detach the link and run `sz`, `rz` or `kermit` on the port.
See [File transfers](file-transfers.md#zmodem-and-kermit).

### No scripting language

There are no scripts with loops or conditions. Instead:

- [triggers](triggers.md) answer a prompt when its text arrives;
- [command lines](command-line.md) chain commands, with `sleep` between
  them, and can be bound to a key.

## The screen

### Terminal mode always wraps

The emulated screen wraps at its own width, like any terminal. Turning
wrapping off only exists in line mode (`Ctrl-a w`). For lines wider than the
view, make the screen wider and pan, see
[Screen size](terminal-mode.md#screen-size).

A device can't turn it off either: `ESC [ ? 7 l` (`tput rmam`) does nothing,
and the text past the right edge still goes on to the next row, where a
terminal would write it over the last column. A program that asks first
(`ESC [ ? 7 $ p`) gets "always on" and can cut its lines itself. Few do it
over a serial link: shells, logs and full-screen programs don't.

### Some text styles aren't shown

Blinking text, hidden text (`ESC [ 8 m`) and strikethrough show as plain
text. Bold, dim, italic, underline, reverse and the colors show as sent. The
screen alignment test of vttest (`ESC # 8`, a screen full of `E`) does
nothing.

### No images

Sixel and kitty graphics aren't drawn. Their bytes still show in line
mode's [raw side panel](raw-side-panel.md).

### Some characters aren't drawn as received

maxicom can't know how many columns your terminal gives each character. The
ones terminals disagree on show as a dimmed `▯`, invisible ones aren't sent,
and a terminal with other width tables may still shift a row: `Ctrl-a R`
draws the screen again. Copy and search always have the real characters. See
[Characters drawn differently](drawn-characters.md).

### The byte trace loses text a full-screen program moves

In terminal mode, the raw side panel shows every byte received with its own
scrolling, and in copy mode [the byte trace](raw-side-panel.md#the-byte-trace)
shows which bytes drew which cell. It follows what a program writes, but not
where the program moves it afterwards: a scroll region, inserted or deleted
lines, or the alternate screen scrolling (vi, `less`, `top`). Those cells
show no bytes, and their bytes show their old cell as written over. Bytes
older than the ones kept have no cell either.

### Some queries stay unanswered

A program on the device can ask its terminal questions. A few never get an
answer:

- **The colors.** maxicom draws with your terminal's own colors, so it
  doesn't know their values. Programs like vim then guess a light or dark
  background.
- **The window title and the clipboard**, for safety. maxicom shows the
  device's title, and can let it copy to your clipboard, but never sends
  either back: the device could read your clipboard, or set a title holding
  a command and have it typed back.
- **The kitty keyboard protocol** and other richer key reports.

See [Answering the device's queries](terminal-mode.md#answering-the-devices-queries).

## Copying

### The clipboard goes through your terminal

maxicom copies with OSC 52, an escape sequence your terminal turns into a
clipboard write. It doesn't talk to the system clipboard itself. When a copy
doesn't arrive, see
[A copy doesn't reach the clipboard](troubleshooting.md#a-copy-doesnt-reach-the-clipboard).

## Marks

### Marks aren't saved

[Marks](marks.md) last as long as maxicom runs. A restart, or a session
restored, starts without them.

## Recordings and replays

### A text recording can't be replayed

A `.log` holds decoded text, not the bytes received. Only a full recording
or a session backup can be replayed. See [Capture](capture.md#what-is-what).

### A group replay holds at most a million lines

Line mode loads 1,000,000 lines of a [group](capture.md#replaying-a-group), and
terminal mode only its `history_size` rows. A notification says what's left out.
To read more, copy the folder's path from the
[session browser](capture.md#session-browser) and open the `.log` files with
another tool.

### A replay sends nothing

Commands that send are refused during a replay, and triggers don't fire.
See [Replay](capture.md#replay).
