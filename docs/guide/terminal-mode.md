# Terminal mode

Terminal mode is a terminal emulator (VT100 / xterm, through the `vt100`
crate). The device's colors, cursor moves and full-screen programs (vim,
less, top, menuconfig) show as they would in a local terminal.

Every key goes to the device as you type it. Enter sends CR, and the arrows
and function keys send their escape sequences, in the cursor key mode the
program asked for.

Terminal mode is the default (`mode = "terminal"`). `Ctrl-a m` switches to
line mode and back.

![Terminal mode running a full-screen process viewer on a board](images/terminal-mode.png)

## Screen size

The emulated screen has a size of its own, in rows and columns, which can
differ from the view: it's the size a program on the device draws for.

The device doesn't learn this size by itself, because a serial line has no
resize signal (see [Serial shells](#serial-shells) for how to tell it). So
changing it only changes what maxicom can show, and an automatic resize
never loses data.

The default, `grow_only`, suits most uses: the screen follows the window
when it grows, and keeps its content when it shrinks.

### Dimension mode

`dimension_mode` in `[terminal_mode]` decides how the screen is sized. `S`
in the settings popup cycles it.

| Mode | Size |
| --- | --- |
| `grow_only` (default) | Grows with the view, never shrinks. When the window gets smaller, the view follows the cursor. |
| `match_view` | The view's size, but never smaller than the content below the cursor or the widest row |
| `fixed_cols` | The view's rows, but never fewer than the content below the cursor; `cols` columns |
| `fixed` | `rows` x `cols` |

`rows` and `cols` in `[terminal_mode]` (default `24` x `80`) give the size
of `fixed`, and the columns of `fixed_cols`. They are read at startup.

### Setting the size by hand

`Ctrl-a W` (or Terminal size in the right-click menu) opens a popup with the
rows and the columns. Type a number, or step it by one with Left / Right.
Enter applies it. A size set this way applies exactly as asked.

`Ctrl-a E` gives the emulated screen the view's size at once.

### A screen wider than the view

You pan sideways with `Ctrl-a Left` / `Ctrl-a Right`, or with the status
bar's pan arrows (click them or use the wheel over them).

The view around the emulated screen is filled with `empty_fill_char`
(default `░`), colored by `term_outside` in the theme's `[terminal]`.

## Scrollback

`history_size` in `[terminal_mode]` (default `2500`) sets how many lines are
kept above the screen. It is read at startup.

The scrollback is maxicom's biggest use of memory: about
`history_size x cols x 32` bytes. 2500 lines of 80 columns take 6.4 MB; of
250 columns, 20 MB.

What the device sends never moves a view you scrolled up, so you can read
in peace. What you send (a key in terminal mode, a line in line mode) brings
the view back to the live screen. See
[Copy mode and mouse](copy-mode-and-mouse.md#scrolling) for the keys.

## Timestamps

`Ctrl-a t` adds a column on the left with the time each row was written, in
`timestamp_format`, followed by a `│`. Line mode shares the setting.

## Cursor

While maxicom's window doesn't have the focus, the cursor is drawn as
`unfocused_cursor_glyph` (`█`).

## Answering the device's queries

A program on the device can't see its terminal. To learn something about it,
it writes a query, an escape sequence, and reads the reply as if you had
typed it. Without a reply, it waits for a timeout, then guesses. Over a
serial line, that's how `resize` learns the size, and why vim or a shell can
pause for a second when nothing answers.

maxicom answers the common queries for the terminal it emulates.

### Turning it on or off

`answer_queries` in `[terminal_mode]` (default `true`) turns the answers on.
`A` in the settings popup toggles it, and `set answer-queries on|off` sets
it.

Only live data gets an answer, and only while you're in terminal mode. A
query inside a restored session or a replay is old: it wasn't sent to this
run, so it isn't answered.

### The groups

The queries come in four groups, and `answer_groups` lists the ones to
answer, all of them by default:

```toml
[terminal_mode]
answer_groups = ["status", "identity", "size", "modes"]
```

The tables below write `ESC [` as you'd see it in a hex dump: the escape
byte (`0x1b`) then `[`. Programs and specifications call it CSI, `ESC P` is
DCS, and `ESC \` ends a DCS reply.

#### `status`

| Query | Asks | maxicom answers |
| --- | --- | --- |
| `ESC [ 5 n` | Are you working? | `ESC [ 0 n`: ready |
| `ESC [ 6 n` | Where is the cursor? | `ESC [ row ; col R`, counted from 1 at the top left |
| `ESC [ ? 6 n` | The same, with the page | `ESC [ ? row ; col ; 1 R` |
| `ESC [ ? 15 n` | Is there a printer? | `ESC [ ? 13 n`: no printer |

The cursor query is the one that matters most. `resize` moves the cursor to
the bottom right corner and asks where it ended, which gives the size (see
[Serial shells](#serial-shells)). Shells and readline ask it too, to know
where the prompt ends.

#### `identity`

| Query | Asks | maxicom answers |
| --- | --- | --- |
| `ESC [ c` | What kind of terminal are you? | `ESC [ ? 62 ; 22 c`: VT220 class, with ANSI colors |
| `ESC Z` | The same, in its VT100 form | The same |
| `ESC [ > c` | Which model and version? | `ESC [ > 0 ; 0 ; 0 c`: no known model |
| `ESC [ = c` | Your unit ID? | `ESC P ! \| 00000000 ESC \`: a zero ID |
| `ESC [ > q` | Your name and version? | `ESC P > \| maxicom <version> ESC \` |
| `ESC [ x` | Your line settings? | `ESC [ 2 ; 1 ; 1 ; 112 ; 112 ; 1 ; 0 x`: fixed values (8 bits, no parity, 9600 baud) |
| ENQ, the byte `0x05` | Who are you? | The `answerback` text, nothing by default |

Every terminal answers `ESC [ c`, so programs send it last, after a query
that may go unanswered: when its reply comes first, they know the other
query isn't supported.

maxicom doesn't claim a model in `ESC [ > c`, because vim turns on xterm
features when it reads an xterm version there.

`answerback` in `[terminal_mode]` sets the text sent on ENQ. It's empty by
default because `0x05` also shows up in binary data.

#### `size`

| Query | Asks | maxicom answers |
| --- | --- | --- |
| `ESC [ 18 t` | Text area, in cells? | `ESC [ 8 ; rows ; cols t` |
| `ESC [ 19 t` | Screen, in cells? | `ESC [ 9 ; rows ; cols t`, the same size |
| `ESC [ 14 t` | Text area, in pixels? | `ESC [ 4 ; height ; width t` |
| `ESC [ 16 t` | One cell, in pixels? | `ESC [ 6 ; height ; width t` |
| `ESC [ 11 t` | Is the window minimized? | `ESC [ 1 t`: no |

The size is the emulated screen's, where its lines wrap (see
[Screen size](#screen-size)), not your window's.

The pixel sizes are only answered when your own terminal gives its size in
pixels. Image tools use them to size their pictures.

#### `modes`

| Query | Asks | maxicom answers |
| --- | --- | --- |
| `ESC [ ? mode $ p` | Is this mode on? | `ESC [ ? mode ; state $ y`: `1` on, `2` off, `0` unknown |
| `ESC [ mode $ p` | The same, for a standard mode | `ESC [ mode ; 0 $ y`: unknown |

The modes maxicom knows:

| Mode | Is |
| --- | --- |
| `1` | Cursor keys in application mode |
| `25` | The cursor shown |
| `47`, `1049` | The alternate screen, the one full-screen programs draw on |
| `9`, `1000`, `1002`, `1003` | Mouse reports: on press, on press and release, while dragging, on any move |
| `1005`, `1006` | Mouse reports in UTF-8, in SGR form |
| `2004` | Bracketed paste |

Programs like neovim and fzf ask before using a mode. For any other mode the
answer is `0`, which they read as "not supported".

### Never answered

| Query | Why |
| --- | --- |
| The colors: `ESC ] 10 ; ?`, `ESC ] 11 ; ?`, `ESC ] 12 ; ?`, `ESC ] 4 ; n ; ?`, and `ESC [ ? 996 n` (dark or light?) | maxicom draws with your terminal's colors and doesn't know their values. vim and the like keep their own light or dark guess. |
| The window title: `ESC [ 20 t`, `ESC [ 21 t` | Safety: the device could set a title holding a command, then have it typed back |
| The clipboard: `ESC ] 52 ; c ; ?` | Safety: the device would read your clipboard |
| Moving or resizing the window: `ESC [ 3 t`, `ESC [ 8 t`, `ESC [ 13 t` | Your window isn't the device's to move |
| A setting's value, a terminfo entry: `ESC P $ q …`, `ESC P + q …` | Not supported |
| Richer key reports: `ESC [ ? u` (kitty keyboard), `ESC [ ? 4 m` | Not supported |

### When to turn answers off

An answer is input to the device, as if you typed it. That's what a shell
expects, but not every program on a serial line is a shell. Turn
`answer_queries` off, or drop a group from `answer_groups`, when:

- the device sends binary data, where a few bytes can read like a query;
- the program reading the line takes any input as a command, like a
  bootloader waiting for an upload.

## Serial shells

Over ssh or in a local terminal, a shell learns its size from the terminal,
and again on each resize (`SIGWINCH`). A serial line carries bytes only. The
device's tty keeps the size its getty set, often `0 x 0` or `24 x 80`, and
never hears about your window.

As a result, programs wrap long lines at the wrong column, and full-screen
ones (vim, less, top, menuconfig) draw at the wrong size.

### Fixing the size with `resize`

maxicom never types a command into the device by itself. Instead, the device
can fix its own size with `resize`:

1. `resize` moves the cursor to the bottom right corner.
2. It asks the terminal where the cursor is.
3. maxicom answers with the emulator's size (where its lines wrap, which
   `dimension_mode` decides).
4. `resize` sets the tty to that size.

`resize` comes with BusyBox, on most embedded Linux boards, and with xterm
(the `xterm` or `x11-utils` package on desktop distributions). It also
prints the matching `COLUMNS` and `LINES` assignments, so
`eval "$(resize)"` updates the current shell's variables too. BusyBox prints
them only when built with `FEATURE_RESIZE_PRINT`.

### Running it automatically

To fix the size at login and after each command, on serial ttys only, add
this to the device's `~/.zshrc`:

```zsh
if [[ $TTY == /dev/tty(S|AMA|USB|ACM|mxc|GS)* ]] &&
   (( $+commands[resize] )); then
  _serial_resize() { eval "$(resize)" }
  precmd_functions+=(_serial_resize)
fi
```

For bash, in `~/.bashrc`, use `PROMPT_COMMAND='eval "$(resize)"'` under the
same kind of `tty` test.

Each run costs a round trip on the line, a few bytes each way. On a very
slow link, call `_serial_resize` once instead of the `precmd_functions`
line, to fix the size at login only.

### Sending the size by hand

Alt-r sends `stty rows R cols C; export LINES=R COLUMNS=C` with the
emulator's size, the same size `resize` would get. In line mode, Alt-r sends
the columns only.

Without `answer_queries`, or on a device without `resize`, the size stays
wrong until you press Alt-r.
