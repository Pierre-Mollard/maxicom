# Changelog

What changes in each version of maxicom. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - Unreleased

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
