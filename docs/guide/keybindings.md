# Keybindings

Every key of maxicom is defined in `keymap.toml`, in the config folder (see
[Settings](settings.md)).

- The help (`Ctrl-a z`) lists the keys in use, your changes included.
- `Ctrl-a r` applies a change without quitting.
- Deleting the file brings back the defaults at the next start.

## Actions and commands

A key runs an action or a command, and the two don't behave the same way.

An **action** is what a key does in its mode, and it happens at once:
`toggle-local-echo`, copy mode's `up`, the search bar's `next`. Copy mode,
the search bar, line mode, terminal mode and the popups each have actions of
their own.

A **command** is what you type on the [command line](command-line.md). It
can take arguments, like `send AT\r` or `baud 9600`, and it waits its turn:
it runs after a line still running, in order, and `cancel` stops it.

The two meet in a few places:

- Every action of `[prefix_actions]` and `[direct_actions]` is also a
  command, so you can type it on the command line. There it waits its turn
  like any other command.
- A command that runs without arguments, like `break`, `plot` or `mark`, can
  go on a key as it is. The help lists it with the actions, but it still runs
  as a command: `Ctrl-a f` pressed during a `sleep 5000` waits for it to end.
- A [named command](#named-commands) gives a name to a command line, so that
  a key can run it.

## The file

```toml
prefix = "ctrl-a"

[prefix_actions]      # after the prefix: Ctrl-a, then the key
d = "directory"
F6 = "bootloader"

[direct_actions]      # without the prefix, in both modes
ctrl-q = "quit"

[commands]            # command lines under a name
bootloader = { run = "dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off", help = "Into the bootloader" }
```

### The prefix

`prefix` is the key that starts maxicom's keys. Change it when `Ctrl-a` is
taken, by tmux or screen, or by a device that needs it:

```toml
prefix = "ctrl-b"
```

Pressing the prefix twice sends the prefix key itself to the device.

### Writing a key

A key is written with its modifiers, joined by `-`: `ctrl-a`, `alt-r`,
`shift-pageup`, `ctrl-shift-left`.

The named keys are `enter`, `esc`, `tab`, `backtab` (Shift-Tab),
`backspace`, `del`, `ins`, `space`, `up`, `down`, `left`, `right`, `pageup`,
`pagedown`, `home`, `end`, and `f1` to `f12`.

Any other key is its character: `d`, `/`, `[`. An uppercase letter means
that letter with Shift, so `D` is Shift-d.

Quote the keys TOML doesn't accept bare: `"/" = "enter-search"`,
`"?" = "enter-search-backward"`.

### What a key does

A value is a name: an action of its section, from the lists below, or a
[named command](#named-commands). In `[prefix_actions]` and
`[direct_actions]`, a command that runs without arguments works too, like
`break` or `plot` (see [Actions](#actions)).

A command line doesn't go on a key. Give it a name in `[commands]`, then bind
the name:

```toml
[prefix_actions]
h = "board-reset"

[commands]
board-reset = { run = "dtr off; sleep 100; dtr on", help = "Pulse DTR, which resets most boards" }
```

### Changing a section

A section you write in the file replaces the default section entirely. A
section you leave out keeps its defaults.

So to add one key to `[prefix_actions]`, copy the whole section from the
default file and add your line to it.

A section in your file also doesn't get the default keys of a later version.
[`maxicom --write-config`](settings.md#updating-the-files) adds them.

### Errors

A key or an action that doesn't parse stops the file from loading, naming
the culprit. At startup maxicom doesn't start; on a reload, the keymap in
use stays.

A command line on a key is an error too, and the message says where it goes:

```text
in [prefix_actions]: h: a command line goes in [commands] under a name, then bind the name
```

## Sections

| Section | When |
| --- | --- |
| `[prefix_actions]` | After the prefix, in both modes |
| `[direct_actions]` | Without the prefix, in both modes: these keys never reach the device |
| `[line_mode]` | In line mode, while typing in the input box |
| `[terminal_mode]` | In terminal mode, before the key goes to the device |
| `[copy_mode]` | In copy mode |
| `[search_mode]` | While the search bar is open |
| `[dialog_actions]` | In the popups |
| `[commands]` | Not keys: the [named commands](#named-commands) the keys run |

The keys of `[prefix_actions]` and `[direct_actions]` do nothing in copy mode
and search. A `[direct_actions]` key bound to a named command is the
exception: it runs in copy mode too, unless a `[copy_mode]` key takes it.

Some popups have keys of their own on top of `[dialog_actions]`, like the
directory's `n`, `e` and `s`. Those are fixed, and each chapter lists them.

## Named commands

`[commands]` gives a name to a command line. A key runs it by its name, and
so does the command line: type `board-reset` there, with its help shown in
the completion.

```toml
[commands]
board-reset = { run = "dtr off; sleep 100; dtr on", help = "Pulse DTR, which resets most boards" }
u-boot-toggle = { run = "trigger toggle u-boot", help = "Arm / disarm the u-boot trigger" }
```

These two are the defaults, on `Ctrl-a h` and `Ctrl-a U`.

| Key | |
| --- | --- |
| `run` | The command line: commands chained with `;`, as on the [command line](command-line.md) |
| `help` | What the help and the completion show. Optional: without it, they show the line. |

A named command runs like a line you type. It waits its turn behind a line
still running, and it doesn't go in the command line's history.

The help's Commands tab lists the named commands with the line each one
runs.

### Binding a named command

A named command goes on a key of `[prefix_actions]`, `[direct_actions]`,
`[line_mode]`, `[terminal_mode]` or `[copy_mode]`:

```toml
[direct_actions]
F5 = "ctrl-c"

[line_mode]
F6 = "ctrl-c"

[commands]
ctrl-c = { run = "send-hex 03", help = "Send Ctrl-c" }
```

- A key of `[line_mode]`, `[terminal_mode]` or `[copy_mode]` runs it in that
  mode only.
- A `[direct_actions]` key runs it in both modes, and in copy mode too,
  unless a `[copy_mode]` key takes the same key.
- The search bar and the popups don't take named commands: there, the keys
  type text.

The help lists the key in its section's tab, with the command's help.

### What a named command can be

A named command that breaks one of these rules stops the file from loading,
naming it:

- Its name is one word: letters, digits, `-` and `_`.
- No built-in command and no action has its name.
- Its line parses, and runs something.
- It runs built-in commands only, not another named command.
- It takes no arguments: a key bound to `board-reset 3` is a command line,
  refused.

Like the other sections, a `[commands]` you write replaces the default one.
[`maxicom --write-config`](settings.md#updating-the-files) adds the default
commands your file lacks.

## Default keys

### `[prefix_actions]`

| Key | Action | |
| --- | --- | --- |
| `Ctrl-a $` | `pan-end` | Pan to the line end |
| `Ctrl-a '` | `marks` | Open the marks |
| `Ctrl-a (` / `Ctrl-a )` | `prev-mark` / `next-mark` | Go to the mark above / below |
| `Ctrl-a ,` | `mark` | Mark a line, waits for its letter |
| `Ctrl-a /` | `enter-search` | Search the history |
| `Ctrl-a 0` | `pan-home` | Pan to the line start |
| `Ctrl-a :` | `command-line` | Open the command line |
| `Ctrl-a <` | `side-shrink-5` | Shrink the side view by 5 % |
| `Ctrl-a >` | `side-grow-5` | Grow the side view by 5 % |
| `Ctrl-a ?` | `enter-search-backward` | Search the history from the end |
| `Ctrl-a B` | `toggle-display-raw` | Toggle the raw side view |
| `Ctrl-a C` | `clear-history` | Clear the history |
| `Ctrl-a D` | `toggle-attach` | Detach / attach the link |
| `Ctrl-a E` | `auto-resize-emulator` | Resize the emulator to the view |
| `Ctrl-a G` | `plot-freeze` | Freeze / unfreeze the plot |
| `Ctrl-a K` | `screenshot-text` | Screenshot the terminal as plain text |
| `Ctrl-a L` | `replay-stop` | Stop the replay, back to the link it replaced |
| `Ctrl-a M` | `toggle-mouse` | Toggle mouse support |
| `Ctrl-a N` | `dismiss-notifications` | Close all the notifications |
| `Ctrl-a O` | `toggle-text-encoding` | Cycle the text encoding |
| `Ctrl-a T` | `triggers` | Open the triggers |
| `Ctrl-a U` | `u-boot-toggle` | A named command: arm / disarm the `u-boot` trigger |
| `Ctrl-a W` | `resize-dialog` | Set the terminal size |
| `Ctrl-a [`, `Ctrl-a b` | `enter-copy-mode` | Enter copy mode |
| `Ctrl-a a` | `toggle-add-lf` | Read a received CR as CR LF |
| `Ctrl-a c` | `clear` | Clear the view, the history a scroll up |
| `Ctrl-a d` | `directory` | Open the directory: the serial ports to switch to |
| `Ctrl-a Down` | `scroll-down` | Scroll down one line |
| `Ctrl-a e` | `toggle-local-echo` | Toggle local echo |
| `Ctrl-a f` | `break` | Hold the line in break, 250 ms by default |
| `Ctrl-a g` | `plot` | Show the next plot, off after the last |
| `Ctrl-a h` | `board-reset` | A named command: pulse DTR, which resets most boards |
| `Ctrl-a j` | `sessions` | Open the session backups |
| `Ctrl-a k` | `screenshot-bytes` | Screenshot the terminal with its colors |
| `Ctrl-a l` | `record` | Start / stop recording (text or full) |
| `Ctrl-a Left` | `pan-left` | Pan left |
| `Ctrl-a m` | `toggle-input-mode` | Switch line / terminal mode |
| `Ctrl-a n` | `dismiss-notification` | Close the top notification |
| `Ctrl-a o`, `Ctrl-a s` | `settings` | Open the settings |
| `Ctrl-a p` | `link-settings` | Open the link settings |
| `Ctrl-a PageDown` | `page-down` | Scroll down one page |
| `Ctrl-a PageUp` | `page-up` | Scroll up one page |
| `Ctrl-a q`, `Ctrl-a x` | `quit` | Quit maxicom |
| `Ctrl-a r` | `reload-config` | Reload the config files |
| `Ctrl-a Right` | `pan-right` | Pan right |
| `Ctrl-a Shift-Left` | `pan-side-left` | Pan the side view left |
| `Ctrl-a Shift-Right` | `pan-side-right` | Pan the side view right |
| `Ctrl-a t` | `toggle-display-timestamp` | Toggle timestamps |
| `Ctrl-a u` | `toggle-add-cr` | Read a received LF as CR LF |
| `Ctrl-a Up` | `scroll-up` | Scroll up one line |
| `Ctrl-a w` | `toggle-line-wrap` | Toggle line wrap |
| `Ctrl-a z` | `help` | Open the help |

`Ctrl-a Ctrl-a` sends `Ctrl-a` to the device.

### `[direct_actions]`

| Key | Action | |
| --- | --- | --- |
| `Ctrl-q` | `quit` | Quit maxicom |
| `Shift-End` | `scroll-to-bottom` | Scroll to the bottom |
| `Shift-Home` | `scroll-to-top` | Scroll to the top |
| `Shift-PageDown` | `page-down` | Scroll down one page |
| `Shift-PageUp` | `page-up` | Scroll up one page |

### `[line_mode]`

| Key | Action | |
| --- | --- | --- |
| `Enter` | `submit` | Send the line |
| `Up`, `Ctrl-p` / `Down`, `Ctrl-n` | `history-prev` / `history-next` | Walk the lines sent |
| `Ctrl-u` | `clear-line` | Clear the input |
| `Ctrl-c` | `interrupt` | Send `0x03` |
| `PageUp` / `PageDown` | `page-up` / `page-down` | Scroll the history |
| `Alt-r` | `auto-resize-remote-line` | Send the view's width to the device |

The input box's editing keys (Ctrl-w, Ctrl-k, Alt-b, the vi keys after Esc)
are built in: see [Line mode](line-mode.md#editing-the-line).

### `[terminal_mode]`

| Key | Action | |
| --- | --- | --- |
| `Alt-r` | `auto-resize-remote` | Send the emulator's size to the device |

### `[copy_mode]`

| Key | Action | |
| --- | --- | --- |
| `h` `j` `k` `l`, arrows | `left` `down` `up` `right` | Move the cursor |
| `0` / `$` | `start-of-line` / `end-of-line` | Start / end of the line |
| `v`, `Space` | `select` | Start a selection |
| `V` | `select-line` | Select whole lines |
| `y`, `Enter` | `yank` | Copy and leave |
| `Y` | `yank-side-inspector` | Copy the bytes as the raw panel shows them |
| `Tab` | `toggle-side` | Cursor to the raw panel and back |
| `Ctrl-y` / `Ctrl-e` | `scroll-up` / `scroll-down` | Scroll the view a line |
| `PageUp` / `PageDown` | `page-up` / `page-down` | A page |
| `,` | `mark` | Mark the cursor's line, waits for its letter |
| `'` | `marks` | Open the marks |
| `(` / `)` | `prev-mark` / `next-mark` | Cursor to the mark above / below |
| `/` / `?` | `search` / `search-backward` | Search from the cursor, forward / backward |
| `n` / `N` | `search-next` / `search-prev` | Next match of the last search, in its direction / the other way |
| `*` | `search-word` | Search the word under the cursor, forward |
| | `search-word-backward` | The same, backward (vim's `#`, no key by default) |
| `q`, `Esc` | `exit` | Leave |

The vi motions (`w`, `b`, `gg`, `G`, `iw`...) are built in: see
[Copy mode and mouse](copy-mode-and-mouse.md#copy-mode).

### `[search_mode]`

| Key | Action | |
| --- | --- | --- |
| `Ctrl-n` / `Ctrl-p` | `next` / `previous` | Next / previous match |
| `Enter` | `confirm` | Close the bar, the cursor on the match |
| `Esc` | `cancel` | Close the bar, back where the search started |
| `Tab` | `search-kind` | Text or bytes |
| `Up` / `Down` | `history-prev` / `history-next` | Walk the searches run before |

### `[dialog_actions]`

| Key | Action | |
| --- | --- | --- |
| `Up` `k` / `Down` `j` | `up` / `down` | Move |
| `Left` `h` / `Right` `l` | `left` / `right` | Change a value, switch a tab |
| `Home` `g` / `End` `G` | `home` / `end` | First / last |
| `PageUp` `Ctrl-u` / `PageDown` `Ctrl-d` | `page-up` / `page-down` | A page |
| `Tab` `Space` `Ctrl-n` / `Shift-Tab` `Ctrl-p` | `cycle-next` / `cycle-prev` | Next / previous field or value |
| `Enter` | `confirm` | Apply, open |
| `Esc` `q` | `close` | Close |

In a popup with a text field (a name, a path), only Enter, Esc and Tab /
Shift-Tab act; the other keys edit the text. See [Popups](popups.md).

## Actions

The actions you can bind in `[prefix_actions]` and `[direct_actions]`. Each
one is also a command of the [command line](command-line.md), which adds
commands with arguments (`send`, `open`, `baud`, `replay`...).

The commands that run without arguments go on these keys too: `mark`,
`jump-mark`, `break`, `plot`, `link-save`, `session-name`, `replay-stop`,
`replay-pause`, `replay-resume` and `replay-restart`. Each one does what it
does on the command line with no arguments, and waits its turn like a
command: see [Actions and commands](#actions-and-commands).

| Action | | Default key |
| --- | --- | --- |
| `quit` | Quit maxicom | `Ctrl-q`, `Ctrl-a q`, `Ctrl-a x` |
| `toggle-input-mode` | Switch line / terminal mode | `Ctrl-a m` |
| `toggle-local-echo` | Toggle local echo | `Ctrl-a e` |
| `toggle-line-wrap` | Toggle line wrap | `Ctrl-a w` |
| `toggle-add-cr` | Read a received LF as CR LF | `Ctrl-a u` |
| `toggle-add-lf` | Read a received CR as CR LF | `Ctrl-a a` |
| `toggle-display-timestamp` | Toggle timestamps | `Ctrl-a t` |
| `toggle-text-encoding` | Cycle the text encoding | `Ctrl-a O` |
| `toggle-indent-wrap` | Toggle indented wrap | — |
| `toggle-display-mode` | Cycle the display mode | — |
| `toggle-display-raw` | Toggle the raw side view | `Ctrl-a B` |
| `screenshot-bytes` | Screenshot the terminal with its colors | `Ctrl-a k` |
| `screenshot-text` | Screenshot the terminal as plain text | `Ctrl-a K` |
| `help` | Open the help | `Ctrl-a z` |
| `settings` | Open the settings | `Ctrl-a o`, `Ctrl-a s` |
| `record` | Start / stop recording (text or full) | `Ctrl-a l` |
| `resize-dialog` | Set the terminal size | `Ctrl-a W` |
| `auto-resize-remote` | Resize the remote to the emulator | `Alt-r` (terminal mode) |
| `auto-resize-remote-line` | Resize the remote to the line view width | `Alt-r` (line mode) |
| `auto-resize-emulator` | Resize the emulator to the view | `Ctrl-a E` |
| `link-settings` | Open the link settings | `Ctrl-a p` |
| `enter-copy-mode` | Enter copy mode | `Ctrl-a [`, `Ctrl-a b` |
| `enter-search` | Search the history | `Ctrl-a /` |
| `enter-search-backward` | Search the history from the end | `Ctrl-a ?` |
| `scroll-up` | Scroll up one line | `Ctrl-a Up` |
| `scroll-down` | Scroll down one line | `Ctrl-a Down` |
| `scroll-to-bottom` | Scroll to the bottom | `Shift-End` |
| `scroll-to-top` | Scroll to the top | `Shift-Home` |
| `page-up` | Scroll up one page | `Shift-PageUp`, `Ctrl-a PageUp` |
| `page-down` | Scroll down one page | `Shift-PageDown`, `Ctrl-a PageDown` |
| `pan-right` | Pan right | `Ctrl-a Right` |
| `pan-left` | Pan left | `Ctrl-a Left` |
| `pan-home` | Pan to the line start | `Ctrl-a 0` |
| `pan-end` | Pan to the line end | `Ctrl-a $` |
| `pan-side-right` | Pan the side view right | `Ctrl-a Shift-Right` |
| `pan-side-left` | Pan the side view left | `Ctrl-a Shift-Left` |
| `side-grow` | Grow the side view by 1 % | — |
| `side-shrink` | Shrink the side view by 1 % | — |
| `side-grow-5` | Grow the side view by 5 % | `Ctrl-a >` |
| `side-shrink-5` | Shrink the side view by 5 % | `Ctrl-a <` |
| `clear` | Clear the view, the history a scroll up | `Ctrl-a c` |
| `clear-history` | Clear the history | `Ctrl-a C` |
| `sessions` | Open the session backups | `Ctrl-a j` |
| `directory` | Open the directory: the serial ports to switch to | `Ctrl-a d` |
| `exit-prefix` | Leave the prefix | — |
| `toggle-attach` | Detach / attach the link | `Ctrl-a D` |
| `reload-config` | Reload the config files | `Ctrl-a r` |
| `toggle-mouse` | Toggle mouse support | `Ctrl-a M` |
| `command-line` | Open the command line | `Ctrl-a :` |
| `cancel` | Stop the file transfer and queued commands | — |
| `toggle-triggers` | Arm / disarm the triggers | — |
| `triggers` | Open the triggers | `Ctrl-a T` |
| `plot-freeze` | Freeze / unfreeze the plot, its data still collected | `Ctrl-a G` |
| `plot-next` | Show the next plot of plots.toml, the first after the last | — |
| `plot-prev` | Show the previous plot of plots.toml, the last before the first | — |
| `dismiss-notification` | Close the top notification | `Ctrl-a n` |
| `dismiss-notifications` | Close all the notifications | `Ctrl-a N` |
| `marks` | Open the marks: a letter jumps to its mark | `Ctrl-a '` |
| `prev-mark` | Go to the mark above | `Ctrl-a (` |
| `next-mark` | Go to the mark below | `Ctrl-a )` |
