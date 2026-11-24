# Profiles

A profile is a file of keys, named commands, highlights, triggers and plots
that maxicom adds to its config while a link uses it. Use one to give a
board its reset keys and its log colors, or a project its plots, without
changing anything for your other devices.

A link without a profile works exactly as before. Profiles only add to the
config, and only while a link that names them is open.

## The profiles folder

Profiles live in the `profiles` folder next to `config.toml`, one file per
profile: the profile `board` is `profiles/board.toml`. With
`--config <file>`, the folder next to that file is used.

The profiles popup (see [below](#the-profiles-popup)) shows the folder's
path, and `c` copies it.

## What a profile holds

Every part is optional. A profile holds only what it writes, and each part
uses the format of its own config file:

```toml
description = "My board: reset keys, log colors, a crash notice"
enabled = true
on_load = "mode line"
on_open = "board-reset"

[prefixed]
h = "board-reset"
H = "bootloader"

[commands]
board-reset = { run = "dtr off; rts on; sleep 100; rts off", help = "Reset through RTS" }
bootloader = { run = "dtr off; rts on; sleep 100; dtr on; rts off", help = "Start the bootloader" }

[highlights.error-lines]
line_start = ["ERROR:"]
fg = "Red"

[triggers.crash]
match = "Fatal exception"
run = 'notify error "the board crashed"'
rearm = true
armed = true

[plots.heap]
separator = "="
keys = ["heap"]
```

| Part | Format | Chapter |
| --- | --- | --- |
| `description` | One line, shown in the profiles popup | |
| `enabled` | `false` turns the profile off, see [Turning one off](#turning-one-off) | |
| `on_load` | A command line run when the profile is laid, see [Running commands](#running-commands) | [Command line](command-line.md) |
| `on_open` | A command line run when the link opens, see [Running commands](#running-commands) | [Command line](command-line.md) |
| `[prefixed]`, `[direct]`, `[line_mode]`, `[terminal_mode]`, `[search_mode]`, `[copy_mode]`, `[dialog]` | The sections of `keymap.toml` | [Keybindings](keybindings.md) |
| `[commands]` | Named commands, as in `keymap.toml` | [Named commands](keybindings.md#named-commands) |
| `[highlights.<name>]`, or words under `[highlights]` | The entries of `highlights.toml` | [Highlights](highlights.md) |
| `[triggers.<name>]` | The rules of `triggers.toml` | [Triggers](triggers.md) |
| `[plots.<name>]` | The plots of `plots.toml` | [Plots](plots.md) |

A profile has no line settings: the baud rate and the framing belong to the
link, in [`links.toml`](saved-links.md) or on the command line. A `[link]`
table in a profile is ignored, with a warning.

## Running commands

A profile can run two [command lines](command-line.md) of its own, at two
different moments.

`on_load` runs when the profile is laid: at start, when a link that names it
opens, and on `profile load`. It's the place for settings, such as the mode
the board's output reads best in:

```toml
on_load = "mode line; set timestamps on"
```

A profile doesn't hold settings itself: `on_load` sets them as if you typed
the line, and they stay as they are when the profile is taken off. Like a
trigger's, these lines can't open a popup or wait for a key, see
[Triggers](triggers.md#the-file). It runs
only for a profile new to the stack, so laying another profile on top
doesn't undo what you changed since.

`on_open` runs once the link opens: at start, on an attach and on `open`. It
doesn't run when maxicom reopens a lost port on its own, since a board that
resets on `on_open` and drops its USB port would loop. Use it to reset the
board, so its whole boot log comes in:

```toml
on_open = "board-reset"
```

When both are due, `on_load` runs first, then `on_open`, before any command
still waiting. With several profiles, each new one runs its `on_load` in
stack order, and only the last `on_open` of the stack runs. A line that
doesn't parse is said in a warning, `on_load: ...` or `on_open: ...`.

## How a profile is added

A profile adds to the config files and wins where they meet:

| Part | What happens |
| --- | --- |
| Keys | A key replaces the same key of its section, however it's written (`F6` and `f6` are the same) |
| Named commands | One of the same name replaces the config's, the others are added |
| Highlights | Added after the config's, so they're drawn over them. One of the same name replaces it |
| Triggers | One of the same name replaces the config's, the others are added |
| Plots | One of the same name replaces the config's in its place, the others are added at the end |

The config files themselves never change: maxicom rebuilds its config from
them and the profiles each time the profiles change.

A profile is checked like the config files: its triggers must compile, its
plots must read, and its keys must name actions or commands. An error names
the profile and its file.

## Giving a link its profiles

A saved link names its profiles in `links.toml`:

```toml
[[link]]
name = "board"
address = "/dev/ttyUSB0"
profiles = ["board", "weather"]
```

The link form has a Profiles field for them, comma separated (see
[Saved links](saved-links.md#editing-in-the-app)). Saving a link with a
profile that has no file warns you, and saves it anyway.

`--profile <name>` gives profiles to any link, saved or not, after the saved
link's own. Give it several times for several profiles:

```bash
maxicom /dev/ttyUSB0 --profile board --profile weather
```

At startup, a profile that doesn't load stops maxicom with its error.

## Several profiles

A link can have several profiles, a board's and a project's for example.
They're added in order, so on a clash the later one wins: with
`profiles = ["board", "weather"]`, a key both of them set does what
`weather` says.

## When the link changes

The profiles go with the link. When you open another link, from the
directory or with `open`, the new one brings its own profiles and the old
ones are taken off. A link that isn't saved has none.

The change happens once the new link is open: if it fails to open, maxicom
goes back to the old link with its profiles. A profile that doesn't load
then opens an error popup, and the profiles in use stay.

A reload (`Ctrl-a r`) reads the config files again and adds the same
profiles over them.

## Changing them while running

| Command | Does |
| --- | --- |
| `profile load <name>` | Adds the profile on top of the link's. The names complete. |
| `profile unload <name>` | Takes it off |
| `profile` | Says the profiles in use |
| `profiles` | Opens the profiles popup |

A change lasts until the link changes. It isn't written to `links.toml`:
edit the link to keep it.

## The profiles popup

`Ctrl-a P` (or Profiles in the right-click menu) lists the profiles of the
folder with their descriptions. The ones in use have `◉` and their place in
the order they're added: the higher number wins a clash. The others have
`○`. A profile that's off, or whose file doesn't read, is dimmed and says
why.

On top of the [popup keys](popups.md):

| Key | Does |
| --- | --- |
| Enter, Tab, Space, a click | Add the selected profile, or take it off |
| `l`, Right | Add it |
| `h`, Left | Take it off |
| `K` | Move it up: added later, it wins over more of the others |
| `J` | Move it down |
| `c` | Copy the folder's path |

## In the status bar

`{profiles}` shows the profiles in use, and the `profiles` condition holds
while there are some. Neither is in the default bar: see
[Status bar](status-bar.md) to add them.

## Replays

A session's backup remembers the profiles in use, so its replay adds them
again: the same highlights and plots as when it was recorded. A profile
that isn't in the folder anymore is skipped with a warning, and the replay
runs without it. Stopping the replay gives the live link its profiles back.

`--replay <path> --profile <name>` adds profiles on top of the session's.

## The profiles that come with maxicom

When maxicom creates the `profiles` folder, it writes two profiles in it:

| Profile | Holds |
| --- | --- |
| `esp32` | ESP-IDF's log lines colored by level (`E` red, `W` yellow, `I` blue, `D` and `V` dimmed), `Ctrl-a h` resetting the board through RTS, `Ctrl-a H` starting its bootloader, a `usb-bootloader` command for the chips with a native USB port, line mode on load (its `on_open` reset is there, commented out), and on each crash (a panic, `abort()`, a stack overflow, the task watchdog, a brownout) a notice and mark `c` on its line |
| `arduino` | Line mode on load, and the `serial-plotter` plot: `temp:23.4,hum:51` or bare numbers, as the Arduino IDE's serial plotter reads them |

They're plain files: read them, edit them, or copy one to start your own.
The `esp32` reset commands drive RTS and DTR, so keep flow control off on
its link, and edit the sequences if your board's auto-reset circuit differs.

### Getting them back

Delete a profile you don't want: once the folder exists, maxicom never
writes into it on its own, so a deleted profile stays deleted.

| To | Run |
| --- | --- |
| Write the missing ones again, the others left as they are | `maxicom --write-config` |
| Put them back as they came, the old files kept as `.bak` | `maxicom --reset-config` |

A newer maxicom doesn't change a profile already written: `--reset-config`
brings its new version.

## Turning one off

`enabled = false` in a profile turns it off without deleting it. A link
that names it opens without it, and a warning says so. Its file isn't
checked past its syntax, so a half-edited profile blocks nothing. Set it
back to `true`, or delete the line, to turn it on.
