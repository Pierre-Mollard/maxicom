# Changelog

What changes in each version of maxicom. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before 1.0, the
version number only tells how big a release is: the last number for a regular
release, the middle one for a big step. A release that needs something from
you says so in an "Upgrading" section.

## [Unreleased]

### Upgrading

In `[terminal_mode]`, `answer_queries` is gone: an empty `answer_groups = []`
answers no query (`A` in the settings popup still switches the answers for
the session), and `answerback` is now `enq_answer`. maxicom warns about the
old keys until you remove or rename them.

Run [`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files)
to get the new keys that scroll the raw side panel in your `keymap.toml`:
`Ctrl-a Shift-Up` / `Ctrl-a Shift-Down` and `Ctrl-a Shift-PageUp` /
`Ctrl-a Shift-PageDown`, and the redraw keys `Ctrl-a R` / `Ctrl-a Ctrl-l`. It
also swaps `Ctrl-a <` and `Ctrl-a >`, now grow and shrink: a `keymap.toml`
written before keeps them the old way round.

In `theme.toml`, `[raw_inspector]` drops `byte_sync` and `byte_ones`, and
`byte_other` is now `byte_control`: maxicom warns about these keys until you
remove or rename them. `byte_text` and `byte_invalid` are new.
`[terminal]` drops `timestamp_del_empty`: `timestamp_del` styles the `│`
after the times on every row, `DarkGray` like line mode's.

A new `focus` group in `answer_groups`: add it to a list you wrote yourself
to send the focus reports.

### Added

- In line mode, a notification tells you once when the device looks like a
  shell, whose history and Tab completion only work in terminal mode: at once
  when it sends a terminal query, on Up or Down after it turned bracketed
  paste on or sent prompt marks, and on Tab. `shell_hint = false` in
  `[line_mode]` turns it off. See
  [The shell hint](https://pierre-mollard.github.io/maxicom/line-mode.html#the-shell-hint).
- Links from the device (OSC 8, as `ls --hyperlink` and gcc send) are
  underlined in terminal mode, with the new `link` entry of the theme's
  `[terminal]`. A click on one, or `gx` on it in copy mode, copies its
  address with a notification: maxicom never opens it.
- Focus reports: vim, neovim and tmux can ask to hear when your window gains
  or loses focus (`ESC [ ? 1004 h`), and maxicom tells them (`ESC [ I`,
  `ESC [ O`), live in terminal mode. They're in the new `focus` group of
  `answer_groups`, on by default.
- `mark_unhandled` in `[terminal_mode]`, with the same groups as
  `notify_unhandled`, puts a marker where something wasn't handled: a control
  byte's control picture (`␀`, `␡`), `◊` for a sequence. Empty by default.
- `notify_unhandled` in `[terminal_mode]` lists groups (`controls`, `c1`,
  `escape`, `csi`, `osc`, `strings`, `shell`) to count in a notification what
  the terminal received but didn't handle: `Unhandled: NUL ×9, CSI ?2027h`.
  Empty by default.
- The window title a program on the device sets shows in the history box's
  title, after the name; `show_device_title = false` in `[ui]` leaves it out.
  Its icon name too with `show_device_icon_name = true`. The status bar can
  show them with `{device_title}` and `{device_icon_name}`.
- `device_clipboard = true` in `[terminal_mode]` lets a program on the device
  copy to your clipboard (OSC 52), with a notification each time. Off by
  default.
- `Ctrl-a R` and `Ctrl-a Ctrl-l` (the `redraw` command) clear your terminal
  and draw the whole screen again, for leftovers your terminal kept.
- `disabled_mode = "line" | "terminal"` in `config.toml` turns a mode off:
  switching to it only warns, and it receives nothing. `"none"` by default.
- `notify_copy = false` in `[ui]` turns off the notification after each copy;
  a clipboard error still shows.
- `backspace = "del" | "bs"` in `[terminal_mode]`, and `O` in the settings
  popup: what the Backspace key sends, DEL as before or BS as minicom does,
  for a device that only erases on BS.
- In terminal mode, the byte trace: maxicom remembers which received bytes
  drew which cell of the screen. In copy mode, the raw side panel shows the
  bytes behind the screen's cursor (the char the cell shows, and the earlier
  ones written there, as a progress bar's), and the screen shows the cell the
  dump cursor's byte drew, written over or not. Each panel keeps the other's
  ghost in its middle, and Tab lands on it. A selection on one side shows on
  the other: on the screen, the dump shows the bytes that make it, colors and
  line ends included, and `Y` copies them, the bytes received rather than
  the ones the screen rebuilds (still copied, with a warning, without a
  trace). A new theme color, `cursor_ghost_old`, for the earlier and written
  over ones. `byte_trace` in `[terminal_mode]` (`Y` in the settings popup,
  `set byte-trace`) turns it off, for a fast link. See
  [The byte trace](https://pierre-mollard.github.io/maxicom/raw-side-panel.html#the-byte-trace).
- `Ctrl-a Shift-Up` / `Ctrl-a Shift-Down` (`scroll-side-up` /
  `scroll-side-down`) scroll the raw side panel a line, and
  `Ctrl-a Shift-PageUp` / `Ctrl-a Shift-PageDown` (`page-side-up` /
  `page-side-down`) a page: in terminal mode the dump on its own, in line mode
  the history with it. The prefix stays between them, as after `Ctrl-a Up`. In
  copy mode with the cursor in terminal mode's dump, `Ctrl-y` / `Ctrl-e`
  scroll the dump. See
  [In terminal mode](https://pierre-mollard.github.io/maxicom/raw-side-panel.html#in-terminal-mode).
- A `byte_escape` color in the theme's `[raw_inspector]`, for the ESC byte
  (`0x1B`) in the raw side panel.
- In line mode, copy mode and the mouse select in the line still coming in,
  before its line end, in the text and the raw side panel. A selection on it
  stays when the rest of the line arrives. See
  [The line still coming in](https://pierre-mollard.github.io/maxicom/copy-mode-and-mouse.html#the-line-still-coming-in).
- The `notifications` command opens the notifications said so far, to read
  again one that went by too fast, and copy its text. Each message is kept
  once, not each repeat, and also goes to a `.notices` file in the session's
  folder. No key opens it by default. See
  [Reading them again](https://pierre-mollard.github.io/maxicom/notifications.html#reading-them-again).

### Changed

- A [group replay](https://pierre-mollard.github.io/maxicom/capture.html#replaying-a-group)
  shows a popup while it loads, with a gauge, the speed and the time left, and
  Esc stops it and goes back to the live link. maxicom froze until the whole
  group was in.
- A group replay loads faster: each mode reads the lines its history holds,
  counted from the newest session back, instead of a fixed number of bytes
  per line. A group of short lines opens two to three times faster.
- Line mode redraws a long history much faster when something changes how
  its lines are laid out: a window resize, a side panel opened or resized,
  timestamps turned on or off, another display or wrap mode, and the first
  look after a group replay. A line that fits on one row is no longer laid
  out to count its rows: on a million lines, about 15 ms instead of almost
  2 s (100 ms for colored lines). Lines with non-ASCII characters are still
  measured one by one, now over several frames: maxicom keeps answering, and
  a small box top left says `Counting lines` until the scrollbar is right.
  See [Counting the rows](https://pierre-mollard.github.io/maxicom/line-mode.html#counting-the-rows).
- A `clear` in a restored session or in a group replay clears the screen but
  no longer erases the terminal's history: one session's `clear` wiped the
  sessions before it in terminal mode. Live, `clear` works as before. See
  [A clear in a session](https://pierre-mollard.github.io/maxicom/capture.html#a-clear-in-a-session).
- In copy mode with the raw side panel shown, the panel without the cursor
  shows the selection in a dimmer version of its color, the new
  `selection_ghost` and `selection_raw_ghost` in the theme's `[ui]`: the
  color tells which side the selection was made on, the brightness where
  the cursor is. It showed in the same color in both panels.
- In terminal mode, the raw side panel shows every byte received, in a dump
  like `xxd`'s: offsets, the bytes and their text, the same number of bytes
  on every line. It showed each screen row rebuilt by the emulator, so line
  ends, cursor moves, erases and text written over never showed, and colors
  came back in the emulator's own codes. The dump scrolls on its own, with
  its own scrollbar, instead of following the screen, and goes back live
  when the screen does; the history's scrollbar moves next to the history.
  Copy mode, the mouse and a byte search work in it. See
  [In terminal mode](https://pierre-mollard.github.io/maxicom/raw-side-panel.html#in-terminal-mode).
- From the live view, line mode's copy mode starts on the line still coming
  in, when there is one.
- In Hex, Octal and Binary, the line still coming in shows its last blanks
  and a carriage return waiting for its line feed, as the lines before it
  show their line end.
- The raw side panel colors bytes by what they are in the encoding, a char
  read whole even when a row cuts it: printable ASCII in white (it was
  cyan, and `U` and `~` showed red as sync bytes), the bytes of other chars
  in light blue, bytes that make no char (a broken UTF-8 char, `0xFF`) in
  light red, the other control bytes in gray. Line mode's Hex, Octal and
  Binary displays color them the same way. See
  [Byte colors](https://pierre-mollard.github.io/maxicom/raw-side-panel.html#byte-colors).
- `Ctrl-a <` grows the side panel and `Ctrl-a >` shrinks it, the way the
  border moves when the panel is on the right. In terminal mode, with the raw
  panel on the right, they size it by the dump's lines: growing, to just fit
  8 more bytes a line; shrinking, to just fit 8 bytes less, with no blank
  columns left. Past the last group of 8, by 1 %.
- In terminal mode, the raw side panel first opens at the size that just
  fits 8 bytes a line in the dump, instead of `split_percent`.
- Durations in `config.toml` and `plots.toml` read the same forms as before
  (`"500ms"`, `"2h 30m"`, `"1day"`, `"1week"`), a day being 24 hours, but
  months and years are refused: they have no fixed length. maxicom writes
  them its new way, `"1d"` rather than `"1day"`.
- In the strftime formats (`timestamp_format`, `time_format`, file names),
  `%Z` gives the time zone's abbreviation (`CET`) instead of its offset (use
  `%:z` for `+01:00`), and `%c` and `%x` write the date another way. The
  other specifiers write what they did.

### Fixed

- In terminal mode, the `│` after the timestamps is `DarkGray` on every row,
  as in line mode. It was yellow next to a time, gray next to an empty row.
- In copy mode, the cursor's ghost in the other panel mixes its color with
  the selection's under it, when both are RGB colors as the defaults are: the
  selection's last char looked unselected. In terminal mode, the screen
  cells of a dump selection written over since stay in the selection's
  color, dimmed. They turned gray.
- The plot panel's `◀` and `▶` stay in place from one plot to the next: the
  name is centered in the width of the longest one in `plots.toml`.
- In terminal mode, a program asking whether autowrap is on
  (`ESC [ ? 7 $ p`) hears "always on": maxicom can't turn it off, and the
  program can cut its lines itself. It heard "unknown".
- Local echo (`Ctrl-a e`) works in terminal mode: what you type, paste and
  send shows on the screen, as in minicom. It only worked in line mode.
- In terminal mode, invisible characters (soft hyphens, zero-width spaces,
  emoji joiners and selectors) no longer shift the rest of the row or leave
  stray characters on the screen: maxicom doesn't send them to your terminal,
  which placed them its own way. Copy and search still have them. An emoji
  with a selector shows in its plain form, a joined emoji as its parts;
  `draw_zero_width = true` in `[ui]` sends them as before, for a terminal
  that counts them as maxicom does. Line mode doesn't send them either now.
- Characters that terminals count at different widths no longer shift the
  rest of their row or leave pieces of earlier lines after a scroll (a `cat`
  of a binary file did it), in both modes: they show as a dimmed `▯`, a wide
  one followed by a blank. Copy and search still have them.
  `draw_unsure_width = true` in `[ui]` draws them as received, for a terminal
  that counts them as maxicom does. A new guide page,
  [Characters drawn differently](https://pierre-mollard.github.io/maxicom/drawn-characters.html),
  lists what is drawn differently and why.
- In terminal mode, a wide character panned into the view's last column shows
  as a blank: it spilled over the right border.
- In terminal mode, a `�` the device sends shows: it disappeared, so a `�`
  pasted to a shell never showed in its echo.
- The line still coming in is decoded with the `encoding` setting, as the
  lines before it: it was always read as UTF-8.
- In the raw side panel, the line still coming in wraps with its text: it
  was one row, out of step with the text's rows once the line wrapped.
- Jumping to a mark put on the line still coming in, by a trigger's `mark`,
  no longer crashes maxicom.
- In terminal mode, bytes that aren't valid text (invalid UTF-8, or from
  `80` up in `ascii`) show as `�`, one per broken character as in line mode,
  instead of disappearing. The raw side panel and a raw copy show them as
  received. See
  [Bytes that aren't valid text](https://pierre-mollard.github.io/maxicom/encoding-and-newlines.html#bytes-that-arent-valid-text).
- With the `ascii` encoding, line mode keeps the escape sequences and their
  colors, as terminal mode does: it showed each control byte, `ESC`
  included, as `�`.

## [0.1.3] - 2026-11-16

### Upgrading

Run [`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files)
to get `Ctrl-a P`, the new profiles popup, in your `keymap.toml`.

Three sections of your `keymap.toml` are renamed, after the help's tabs:
`[prefix_actions]` is now `[prefixed]`, `[direct_actions]` is `[direct]` and
`[dialog_actions]` is `[dialog]`. Rename them in your file: under the old
names, their keys aren't read, and maxicom says so in a warning.

A key can't run a command line any more: maxicom doesn't start, and the
message names the key. Give the line a name in the new `[commands]` section,
then bind the name, as in
[Named commands](https://pierre-mollard.github.io/maxicom/keybindings.html#named-commands).
The default `Ctrl-a h` and `Ctrl-a U` lines are now the named commands
`board-reset` and `u-boot-toggle`: write `h = "board-reset"` and
`U = "u-boot-toggle"`.

Do both before
[`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files):
it needs the file to load, and it drops the sections under the old names
(the old file stays next to it, as `.bak`).

A profile already in your `profiles` folder isn't updated by a new
version. To get the new `esp32` and `arduino` (line mode on load, the crash
triggers, the `board-reset` fix), delete them and run
[`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files).

A `center` status bar template saved in your `config.toml` keeps the armed
triggers in red: write `armed?primary:hide` in its `◎ {trigger_count}` block,
as in the [default](https://pierre-mollard.github.io/maxicom/status-bar.html#the-default).

### Added

- [Named commands](https://pierre-mollard.github.io/maxicom/keybindings.html#named-commands):
  `[commands]` in `keymap.toml` gives a command line a name and a help. A key
  runs it by its name, in both modes, line mode, terminal mode and copy mode,
  and so does the command line, where the name completes with its help.
  The defaults: `board-reset` (`Ctrl-a h`), `u-boot-toggle` (`Ctrl-a U`),
  and `ctrl-c` and `ctrl-d`, with no key.
- The help lists every key with what its action does, dimmed, in every tab,
  and the named commands with the line each one runs.
- The commands that run without arguments (`mark`, `plot`, `break`,
  `replay-stop`...) go on a key like an action, and the help lists them with
  the actions.
- Profiles: a file in the `profiles` folder next to `config.toml` adds keys,
  named commands, highlights, triggers and plots while a link uses it. A saved
  link names its profiles in `links.toml` or in the link form, and
  `--profile` gives them to any link; several stack, the later one winning.
  `profile load` and `profile unload` lay one or take it off while running,
  `profile` says them, and the profiles popup (`Ctrl-a P`) lists them all to
  add, take off or reorder. See
  [Profiles](https://pierre-mollard.github.io/maxicom/profiles.html). `{profiles}` shows them in the status bar, and a
  session's replay uses the profiles it was recorded with.
- Two profiles come with maxicom, written with the `profiles` folder:
  `esp32` (ESP-IDF log levels in color, an RTS reset on `Ctrl-a h`, the
  bootloader on `Ctrl-a H`, a crash notice) and `arduino` (the serial plotter
  plot). Delete one you don't want: it stays deleted. `--write-config` writes
  a missing one again, `--reset-config` writes them back. `enabled = false`
  in a profile turns it off without deleting it.
- A profile's `on_load` runs a command line when the profile is laid, for
  settings such as `mode line`, and `on_open` one when the link opens, such
  as a board reset. See
  [Running commands](https://pierre-mollard.github.io/maxicom/profiles.html#running-commands).
  The `esp32` and `arduino` profiles switch to line mode on load; `esp32`'s
  `on_open` reset is there, commented out.
- `mode line|terminal` switches the mode, and does nothing when it's already
  so.
- [DTR and RTS on open](https://pierre-mollard.github.io/maxicom/links.html#dtr-and-rts-on-open):
  `dtr` and `rts` in `[link]` and in saved links, `--dtr` and `--rts`, and two
  rows in the line settings popup and the link form. `keep` (the default)
  leaves each line as the system puts it, `on` and `off` set it on each open.
  On Linux and macOS both lines move together, so a board's auto-reset
  circuit doesn't see one line alone.
- A trigger's `mark = "c"` puts that mark on the line its text came in on, so
  `Ctrl-a '` then `c` goes back to it. See
  [Marking the line](https://pierre-mollard.github.io/maxicom/triggers.html#marking-the-line).
- Each trigger counts how many times it fired: the trigger browser shows it,
  `trigger reset [name]` sets it back to zero, and a switch to another device
  starts it again.
- The `esp32` profile notices and marks `c` on a panic, `abort()`, a stack
  overflow, the task watchdog and a brownout.

### Changed

- A key binds a name only: an action or a named command (see Upgrading).
- `keymap.toml`'s sections `[prefix_actions]`, `[direct_actions]` and
  `[dialog_actions]` are renamed `[prefixed]`, `[direct]` and `[dialog]`, and
  each one's comment says what its keys take. An unknown key in the file is
  now warned about, like in `config.toml`.
- In copy mode, `,` is copy mode's own `mark` action: it marks at once instead
  of waiting behind a line still running.
- The help popup grows up to 100 columns, for the actions' descriptions.
- The default `plots.toml` no longer has `serial-plotter`: it's the `arduino`
  profile's now. A `plots.toml` already written keeps it.
- The armed triggers in the status bar are in the main color, cyan by
  default, instead of red.
- In a status bar template, `line` and `term` are the mode shown, also while
  the prefix, copy mode, the search or a popup has the keys.
- The `esp32` profile colors ESP-IDF's info lines (`I (`) blue.

### Fixed

- In terminal mode, the cursor keys mode (`NOR`, `APP`) no longer disappears
  from the status bar while the prefix, copy mode, the search or a popup has
  the keys.
- The `esp32` profile's `board-reset` (`Ctrl-a h`) turns DTR off first: after
  an open both lines are on, and the reset only worked after a flash had
  left DTR off.

## [0.1.2] - 2026-11-14

### Upgrading

Run [`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files)
to get the new keys in your `keymap.toml`: `/`, `?`, `n`, `N` and `*` in copy
mode, Up / Down in the search bar, `Ctrl-a 0` / `Ctrl-a $`, and
`Ctrl-a g` / `Ctrl-a G` for the plots. A `center` status bar template saved
in your `settings.toml` doesn't show the plot: add the `∿ {plot}` block of the
[default](https://pierre-mollard.github.io/maxicom/status-bar.html#the-default).
`Ctrl-a ?` now searches backward and the help stays on `Ctrl-a z`; your file
keeps `"?" = "help"` until you change it to `"?" = "enter-search-backward"`.
A `left` status bar template saved in your `settings.toml` keeps the copy
color while you search: move its `search?(...)` before `copy_raw?` and
`copy?`, as in the [default](https://pierre-mollard.github.io/maxicom/status-bar.html#the-default).
Your `highlights.toml` keeps working as it is: the new groups are only in a
new file. To get them, delete it and maxicom writes the new default, or copy
the [default](https://pierre-mollard.github.io/maxicom/highlights.html#the-file).
Your `triggers.toml` keeps its rules: to get the new `panic` example, add it
from the [default](https://pierre-mollard.github.io/maxicom/triggers.html#the-file).
A `center` status bar template saved in your `settings.toml` keeps its
`⇡ {send_file} {send_percent}%` block, which the new progress row repeats:
remove it, or turn the row off with `ui.transfer_row = false`.

### Added

- [File transfers](https://pierre-mollard.github.io/maxicom/file-transfers.html)
  with XMODEM, XMODEM-1K and YMODEM, built in (no `sz` needed):
  `upload ymodem <file>` sends to U-Boot's `loady` / `loadx`, `rx`, `rb` and
  most bootloaders, the receiver started before or after. `download ymodem`
  receives what `sb` or `sx` sends, into the new `[transfers] dir` or a folder
  given: names, sizes and dates kept, a whole batch at once, nothing
  overwritten, a stopped file kept as `.part`. While a transfer runs the link
  is its own: keys and `send` wait, the protocol's bytes stay off the screen.
  ZMODEM and Kermit go through a detach and your own tools.
- [`maxicom --reset-config`](https://pierre-mollard.github.io/maxicom/settings.html#resetting-the-files)
  puts the config files back to the defaults, after a confirmation, each old
  file kept as `.bak`; `links.toml` is left alone. It works when a file
  doesn't load.
- `notify <info|success|warn|error> <text>` shows a
  [notification of yours](https://pierre-mollard.github.io/maxicom/notifications.html#your-own-notifications),
  from the command line, a key bound to a command line or a
  [trigger](https://pierre-mollard.github.io/maxicom/triggers.html#raising-a-notification)
  (`run = 'notify error "the board panicked"'`). It sends nothing to the
  device. A trigger with `silent = true` fires without its own notice, so its
  `notify` is the only one; the default `triggers.toml` has such a `panic`
  trigger, disarmed.
- A [progress row](https://pierre-mollard.github.io/maxicom/file-transfers.html#the-progress-row)
  over the status bar while a file is sent (`send-file` or `upload`): a
  gauge, the bytes, the rate, the time left, the retries, and a `✕` that
  stops it. `ui.transfer_row` turns it off; the status bar has
  `{send_rate}` and `{send_eta}`.
- [Plots](https://pierre-mollard.github.io/maxicom/plots.html): the numbers
  your device prints (`temp=23.4 hum=51`, the serial plotter format
  `temp:23.4,hum:51` or bare columns) charted live in a panel at the bottom
  by default (`[plot_side]`), with the raw side panel if it's on, with a
  legend, autoscale and a time or sample window. The plots are defined in
  the new `plots.toml`, which comes with four examples. `Ctrl-a g`
  (`plot`) goes from one to the next, `Ctrl-a G` (`plot-freeze`) freezes the
  chart while the data goes on, and `--plot <name>` or `plot = "<name>"` in
  `config.toml` shows one at startup. The status bar shows `∿ <name>`
  (`{plot}`), and the settings popup has a `V` row. The panel has its own
  [place and size](https://pierre-mollard.github.io/maxicom/plots.html#size-and-position),
  `[plot_side]`, and a `✕` and a right-click "Close plot" to hide it.
- A right click on the raw side panel offers "Close raw panel", first in the
  menu.
- [Highlight groups](https://pierre-mollard.github.io/maxicom/highlights.html#the-file):
  a table with a `words` list gives all of them one style, as
  `[errors] words = ["ERROR", "FAIL"]`. `ignore_case` and `whole_word` set
  how a group or a word matches, and the entries apply in the file's order,
  a later one drawn over an earlier one.
- [Whole-line highlights](https://pierre-mollard.github.io/maxicom/highlights.html#whole-lines):
  a table with `line_start = ["ERROR:", "[E]"]` in place of `words` colors
  every line that starts with one of them, on all its rows, for logs that
  begin with their level. The words are drawn over the colored line. The
  default file only shows an example in its header comment.

- [Search backward](https://pierre-mollard.github.io/maxicom/search.html#starting-a-search)
  with `Ctrl-a ?` (`enter-search-backward`): from the live view, the newest
  match first.
- Search from [copy mode](https://pierre-mollard.github.io/maxicom/search.html#starting-a-search):
  `/` and `?` search forward and backward from the cursor, which follows the
  match as you type; Esc puts it back, a selection kept.
- `n` / `N` in copy mode go to the
  [next match](https://pierre-mollard.github.io/maxicom/search.html#going-through-the-matches)
  of the last search, in its direction or the other way, a notification when
  they go around the history's end. After Enter the matches stay lit and the
  search bar stays in the bottom border with its count, until you leave copy
  mode.
- `*` in copy mode
  [searches the word under the cursor](https://pierre-mollard.github.io/maxicom/search.html#searching-the-word-under-the-cursor)
  (`search-word`, and `search-word-backward` with no key).
- Up / Down in the search bar walk the searches you ran, among those
  starting with what you typed (`history-prev` / `history-next` in
  `[search_mode]`).
- `Ctrl-a 0` and `Ctrl-a $` pan to the line start and end
  ([`pan-home`, and the new `pan-end`](https://pierre-mollard.github.io/maxicom/keybindings.html#actions)).

### Changed

- The default `highlights.toml` has three groups, case ignored and whole
  words only: errors (`ERROR`, `FAIL`, `FAILED`...) in white on red,
  warnings (`WARN`, `WARNING`) in yellow, and `OK`, `SUCCESS`, `PASS` in
  green. Two single words show the other forms: `INFO` in cyan, `TIMEOUT`
  in bold magenta.
- A search always runs in copy mode: `Ctrl-a /` and `Ctrl-a ?` enter it at
  the view, and Esc goes back to the live view.
- `Ctrl-a ?` searches backward; the help is on `Ctrl-a z`.
- The search bar is shorter: `/error (3/12)`, `?` for a search backward,
  `[bytes]` dimmed before a byte search only, `1000+` past the match cap, and
  `Enter Go` as Enter's hint. The count and the keys sit at the bar's right
  end, so they don't move as you type.
- The status bar reads `SRCH` while you type a search, on a light blue badge
  (`search_indicator`, green before, like the marks' badge); `COPY` again
  once Enter takes you to the match.
- The search bar opens empty, the last search dimmed in it: Enter searches it
  again, and Esc keeps it for `n` and `N`. Typing no longer adds to the last
  search.
- The default status bar no longer shows the file being sent: the new
  progress row does.
- [Notifications](https://pierre-mollard.github.io/maxicom/notifications.html)
  stay the time to read them, from 2 seconds for a short one to 10 for a
  long one, an error at least 6. A file transfer's messages follow each other
  in one notification, and so do the link's (connecting, connected, lost,
  reconnected), instead of piling up. A reconnect says "Reconnected to" as an
  attach does. While the progress row shows, no notification repeats that a
  transfer waits for the other side; a download names its files and gives the
  folder once, at the end. A full stack drops infos before warnings and
  errors, a message said again counts in its title (`×2`), the time starts
  when a notification first shows, and a `cancel` says `Cancelled: …` where a
  failure says `Stopped …: why`.
- Cycling a setting (the raw side panel's view, the encoding, Add CR...) or
  the plot, or copying again, updates one notification instead of adding one
  per press.
- The settings popup lists Mouse before Byte Inspect, so the raw side
  panel's row and the plot's sit together.
- The raw side panel's border joins the window's border (`┬` `┴` on the
  right, `├` at the bottom).

### Fixed

- Highlights are matched in each received line instead of each row of the
  screen: a word wrapped over two rows is lit on both, a word cut by the
  pan is lit on its visible part, and the part of a longer word left at an
  edge (`ERROR` of `NOERROR`) no longer lights up. In terminal mode the rows
  the device wrapped are joined. In line mode, search matches and the copy
  selection show over a highlight's background, and the time column is no
  longer matched. In both modes a line ends at its last character: a rule
  ending in a space doesn't match the blank end of a terminal row.
- In terminal mode, the raw side panel can be panned during a search: it no
  longer comes back to the match on every frame.
- Without borders, the history's scrollbar is in the history's last column:
  it no longer covers the raw side panel's.
- Leaving copy mode gives back the pan you had when you entered it, instead
  of leaving the view panned where the cursor took it.
- The pan arrows of the history's bottom border no longer draw over the
  search bar.
- Without borders, the search bar takes a row of its own under the history
  instead of covering its last row, which could hide the cursor or a match.
  In terminal mode the remote isn't resized: the view scrolls one row.
- In terminal mode with the status bar in the bottom border (the `border`
  layout), the search bar no longer hides it: it takes a row of its own
  above the border.
- In terminal mode, copy mode no longer starts on an empty row when the
  output ended with a newline (the bottom left of a full screen): it starts
  on the last line with text.
- In terminal mode, the matches of a search follow the screen when the device
  redraws it (a clear, a full-screen app): the old ones go, the new ones are
  lit.
- In a full-screen app (the alternate screen), a search no longer finds a
  copy of the screen's top line for each line of the history.
- A text search ignores the case of every letter, not only ASCII ones:
  `été` finds `ÉTÉ`.
- A search from copy mode that found its match around the history's end
  says so when you press Enter, as `n` and `N` do.
- On a view with an even number of rows, terminal mode puts a mark or a
  search match on the middle row, as line mode does, not one row above.

## [0.1.1] - 2026-11-08

### Upgrading

Coming from 0.1.0, your config files keep what 0.1.0 wrote in them. Run
[`maxicom --write-config`](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files)
to update them: it adds the new settings, the comments and the mark keys, and
saves the old files as `.bak`. It keeps your values, so a few things are left
to you:

- The history and session folder sizes stay at their old values, see Changed.
  Delete those lines to get the new defaults.
- The status bar stays as it was. Delete `left`, `center` and `right` from
  `[ui.status_bar]` to get the new one.
- In `theme.toml`, `mark_restorable` is now `mark_restored`, and
  `term_flash_fg_color` and `term_flash_bg_color` are now `flash` in `[ui]`.
  Set your colors again under the new names if you had changed them.

### Added

- The generated config files have
  [a few comments](https://pierre-mollard.github.io/maxicom/settings.html#defaults):
  a link to the guide above each section, and the values a key takes at the end
  of its line.
- `--write-config` [updates the config files](https://pierre-mollard.github.io/maxicom/settings.html#updating-the-files)
  to the current version and keeps your values.
- A warning names the [unknown keys](https://pierre-mollard.github.io/maxicom/settings.html#unknown-keys)
  of `config.toml` and `theme.toml`, such as a typo, which were silently ignored.
- [Size reports](https://pierre-mollard.github.io/maxicom/terminal-mode.html#size-reports-from-programs)
  for the programs that ask for them (mode 2048): maxicom sends the
  terminal's size on each change, so neovim 0.12 fits the screen over a
  serial line with no `resize` to run. They're part of the `size` group of
  the query answers.
- [Middle-click paste](https://pierre-mollard.github.io/maxicom/copy-mode-and-mouse.html#middle-click-paste)
  of the last copy, anywhere in the window (`middle_click_paste` in `[ui]`
  turns it off).
- `--config <file>` reads the settings from
  [another file](https://pierre-mollard.github.io/maxicom/settings.html#another-settings-file)
  than `config.toml`, for a setup per board.
- `focus_effects` in `[ui]`: off, the window keeps its focused look when it
  loses the focus.
- The status bar shows the cursor keys mode in terminal mode, `NOR` or `APP`
  as in minicom: `{cursor_keys}` and the `app_cursor` condition.
- [Marks](https://pierre-mollard.github.io/maxicom/marks.html) in the
  history, `a` to `z` as in vi: `Ctrl-a ,` then a letter marks a line,
  `Ctrl-a '` lists them and a letter jumps back, `Ctrl-a (` / `)` walk them.
  The same keys in copy mode, Add mark and Marks… in the right-click menu,
  the letters in the history's left column and on the scrollbar, the line
  flashing after a jump; `[ui.marks]` turns each part off. Commands `mark`,
  `jump-mark`, `delete-mark`, `marks`, `prev-mark`, `next-mark`. Marks
  aren't saved across runs, see
  [what maxicom doesn't do](https://pierre-mollard.github.io/maxicom/limits.html#marks).
- Keys in `[copy_mode]` can run a
  [command line](https://pierre-mollard.github.io/maxicom/command-line.html#binding-a-command-line-to-a-key),
  like the prefix's.
- The [session browser](https://pierre-mollard.github.io/maxicom/capture.html#session-browser)
  shows the sessions in a tree, by device, month, ISO week and day
  (`group_by` in `[sessions]`), and opens on the running session. `s` switches
  to the list of all the sessions.
- [Replay a whole group](https://pierre-mollard.github.io/maxicom/capture.html#replaying-a-group)
  of sessions, a day or a week, to scroll and search them together: `p` on a
  group. A notification says what doesn't fit in the histories. A group also
  has a preview, and `d` deletes it after a question.
- [Name a session](https://pierre-mollard.github.io/maxicom/capture.html#naming-a-session)
  with `n` in the session browser, or `session-name` for the running one. The
  name shows in the browser and in the restore and replay separators.
- A [saved link's color](https://pierre-mollard.github.io/maxicom/saved-links.html#its-color),
  `color` in `links.toml` or in the link's form. It shows in the directory, on
  its sessions in the session browser, and in the status bar's `link`.

### Changed

- Much bigger defaults, which you can lower on an older computer: line mode
  keeps 250,000 lines (`line_mode.history_size`, was 1,000), terminal mode
  100,000 rows (`terminal_mode.history_size`, was 2,500), and the session
  folder takes 5 GiB (`sessions.max_size`, was 200 MiB). A full
  [history](https://pierre-mollard.github.io/maxicom/line-mode.html#history-size)
  uses far more memory. A config file written by an earlier version keeps its
  old values.
- The size alone limits the session folder by default: `sessions.keep` is 0, no
  limit (was 200). A notification says which sessions a cleanup removed, and
  the session browser shows how full the folder is.
- The session browser's `●` mark takes the theme's main color, and `○` replaces
  `»`: it marks the session really restored into the history, not the one that
  would be. The theme entry `mark_restorable` is now `mark_restored`.
- The session browser's selected row is one bar across the list.
- The directory's selected row is one color, its `●` and state included, and a
  hovered row in the popups' lists too.
- `d` in the session browser asks before deleting a session.
- `s` in the session browser no longer sorts by device, as the tree groups by
  device.
- The status bar's mode badge reads `COPY` in copy mode, instead of `TERM`
  or `LINE` in the copy colors.
- The session browser's preview shows the lines in their colors, the
  timestamps dimmed.
- The theme's flash colors moved from `[terminal]` (`term_flash_fg_color`,
  `term_flash_bg_color`) to one `flash` style in `[ui]`, shared by the
  screenshot and the mark jumps.

## [0.1.0] - 2026-11-06

The first release. Everything below is described in the
[user guide](https://pierre-mollard.github.io/maxicom/).

### Added

- Serial ports on Linux, macOS and Windows, and devices over raw TCP
  (ser2net, ESP32 bridges, QEMU), with
  [port locks](https://pierre-mollard.github.io/maxicom/locks.html) shared
  with minicom and picocom.
- Two views of the same data: a
  [terminal emulator](https://pierre-mollard.github.io/maxicom/terminal-mode.html)
  for shells and full-screen programs, which answers the device's queries,
  and a [line mode](https://pierre-mollard.github.io/maxicom/line-mode.html)
  for logs, with timestamps and an input box.
- A [directory](https://pierre-mollard.github.io/maxicom/directory.html) of
  serial ports and
  [saved links](https://pierre-mollard.github.io/maxicom/saved-links.html),
  which tests who holds each port without opening it.
- Scrollback,
  [copy mode](https://pierre-mollard.github.io/maxicom/copy-mode-and-mouse.html)
  with vi keys, mouse selection, and
  [search](https://pierre-mollard.github.io/maxicom/search.html) by text or
  bytes.
- A [raw side panel](https://pierre-mollard.github.io/maxicom/raw-side-panel.html)
  with the received bytes in hex, octal or binary.
- A backup of every session, restored at the next launch, recordings,
  replays and screenshots, see
  [Capture](https://pierre-mollard.github.io/maxicom/capture.html).
- A [command line](https://pierre-mollard.github.io/maxicom/command-line.html)
  (`send`, `dtr`, `break`, `mash`...), keys bound to command lines, and
  [triggers](https://pierre-mollard.github.io/maxicom/triggers.html) that
  answer boot prompts.
- minicom's keys and options: `maxicom -D /dev/ttyUSB0 -b 115200`, then
  `Ctrl-a z` for the help, see
  [Coming from minicom](https://pierre-mollard.github.io/maxicom/coming-from-minicom.html).
- Configurable
  [settings](https://pierre-mollard.github.io/maxicom/settings.html),
  [keys](https://pierre-mollard.github.io/maxicom/keybindings.html),
  [colors](https://pierre-mollard.github.io/maxicom/theme.html) and
  [status bar](https://pierre-mollard.github.io/maxicom/status-bar.html),
  reloaded while it runs (`Ctrl-a r`).

[Unreleased]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Pierre-Mollard/maxicom/releases/tag/v0.1.0
