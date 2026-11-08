# Changelog

What changes in each version of maxicom. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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

[Unreleased]: https://github.com/Pierre-Mollard/maxicom/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Pierre-Mollard/maxicom/releases/tag/v0.1.0
