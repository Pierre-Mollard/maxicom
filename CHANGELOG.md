# Changelog

What changes in each version of maxicom. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before 1.0, the
version number only tells how big a release is: the last number for a regular
release, the middle one for a big step. A release that needs something from
you says so in an "Upgrading" section.

## [Unreleased]

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
- A [progress row](https://pierre-mollard.github.io/maxicom/file-transfers.html#the-progress-row)
  over the status bar while a file is sent (`send-file` or `upload`): a
  gauge, the bytes, the rate, the time left, the retries, and a `✕` that
  stops it. `ui.transfer_row` turns it off; the status bar has
  `{send_rate}` and `{send_eta}`.
- [Plots](https://pierre-mollard.github.io/maxicom/plots.html): the numbers
  your device prints (`temp=23.4 hum=51`, the Arduino plotter's
  `temp:23.4,hum:51` or bare columns) charted live in the raw side panel's
  place, with a legend, autoscale and a time or sample window. The plots are
  defined in the new `plots.toml`, which comes with four examples. `Ctrl-a g`
  (`plot`) goes from one to the next, `Ctrl-a G` (`plot-freeze`) freezes the
  chart while the data goes on, and `--plot <name>` or `plot = "<name>"` in
  `config.toml` shows one at startup. The status bar shows `∿ <name>`
  (`{plot}`), and the settings popup has a `V` row.
- [Highlight groups](https://pierre-mollard.github.io/maxicom/highlights.html#the-file):
  a table with a `words` list gives all of them one style, as
  `[errors] words = ["ERROR", "FAIL"]`. `ignore_case` and `whole_word` set
  how a group or a word matches, and the entries apply in the file's order,
  a later one drawn over an earlier one.

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

### Fixed

- In terminal mode, the raw side panel can be panned during a search: it no
  longer comes back to the match on every frame.
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

[Unreleased]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Pierre-Mollard/maxicom/releases/tag/v0.1.0
