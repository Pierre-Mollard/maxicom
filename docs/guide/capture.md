# Capture

maxicom can keep what the device sends in several ways:

- **recordings** that you start and stop;
- a **session backup** of everything, made on its own, which comes back at
  the next launch;
- **replays** of both;
- **screenshots** of the terminal.

## What is what

| Kind | Made | Holds | Replayable |
|---|---|---|---|
| Session backup | On its own, every session | A folder of files: raw bytes, times, info, text | Yes |
| Full recording | By you | The same files as a session, in a folder you choose | Yes |
| Text recording | By you | A `.log` file of decoded lines, to read | No |

### The files of a session

A session, or a full recording, is a folder `<name>/` holding:

| File | Holds |
|---|---|
| `<name>.raw` | The bytes exactly as received |
| `<name>.times` | When each chunk of bytes came |
| `<name>.meta` | What they were received on: the device, the line settings, the encoding, the terminal's size |
| `<name>.log` | The text, for you to read |

The `.raw`, `.times` and `.meta` together let a session replay and restore
as it was: both modes are rebuilt from the `.raw`. The `.log` is never read
back.

A text recording is only the `.log`: decoded, split into lines, with times
and marks added. That's why it can't be replayed.

## Recordings

### Starting a recording

`Ctrl-a l` (or Record… in the right-click menu) opens the recording dialog.
Its stream follows the mode you're in: text in line mode, full in terminal
mode.

The name is filled in for you, without an extension. The stream adds it,
shown after the name: `.log` for text, `/` for a full recording's folder.

On top of the [popup keys](popups.md):

| Key | Does |
|---|---|
| Enter | Starts recording |
| Tab | Switches between text and full |
| Shift-Tab | Includes everything since the session started |

With the session included, the recording first gets everything the session
backup holds, then goes on recording.

### Stopping

While a recording runs, `● REC` shows in the status bar. `Ctrl-a l` again,
or a click on `● REC`, stops it.

Only one recording runs at a time. A full recording includes the text, so
you don't need both.

### Text recordings

A text recording writes the received lines, decoded, one per line. With
`timestamp` on, each line starts with its time in `timestamp_format`.

In line mode, the lines you send are written too, after `tx_mark`.

If the device changes during the recording (another port, baud rate,
framing or encoding), it goes on, with a line marking the change:
`── device: ttyUSB1, 115200 8N1, UTF8 ──`.

### Full recordings

A full recording writes a folder like a session's: the received bytes, their
times, the text, and a `.meta`. You can replay it with
`maxicom --replay <folder>`, and the `.meta` makes the replay decode with
the right encoding and show the line settings it was recorded with.

During a replay, a full recording takes the replayed session's `.meta`. A
replayed file without one gives a recording without one.

With the session included, every file starts with the session's content,
and the `.meta` is the session's.

A full recording stops, with a notice, when the device changes: another
port, baud rate, framing or encoding, or a replay starting or ending.

## Session backup

Every session is recorded, raw and text, without you doing anything. Each
one gets its own folder:

```text
~/.cache/maxicom/sessions/<date>_<time>_<port>/
```

The time is to the millisecond. See [Install](install.md#where-its-files-are)
for the folder on other systems.

The backup is flushed every 250 ms, so a crash or a `kill` loses almost
nothing.

It doesn't light `● REC`, and it doesn't run during a replay. Your own
status bar can show it with the `session` condition, for example
`[◆](session?muted:hide)`. The default bar doesn't.

### Its text

The `.log` is written with `timestamp`, `timestamp_format`, `record_tx` and
`tx_mark` as they were when the session started. A reload changes these for
the recordings, never for a running backup.

### Cleaning up old sessions

When a session starts, maxicom removes:

1. the sessions that received nothing;
2. then the oldest ones, until at most `sessions.keep` are left and all of
   them together fit in `sessions.max_size`.

The current session, and any still running in another maxicom, count in the
size but are never removed.

### Recovering a session by hand

Open its `.log` to read it, or replay it:

```bash
maxicom --replay ~/.cache/maxicom/sessions/20261028_141500123_ttyUSB0
```

Its `.meta` gives the replay the encoding and the line settings.

## Restore

When you open a device, its last session can come back in the history. This
happens at startup and on each switch to another device (through the
directory, `open`, or a saved link).

Line mode gets the lines back, and terminal mode the screen and scrollback.
A separator line follows, before the live data:

```text
── restored: ttyUSB0, 2026-10-28 14:15 → 16:40 ──
```

The terminal replays the session at the size it ended with, so wide lines
come back whole. Then `dimension_mode` sizes it to the view.

### Turning it on or off

`restore` in `[sessions]`:

| Value | Does |
|---|---|
| `always` (default) | Restores |
| `ask` | Asks each time |
| `never` | Doesn't restore |

`--restore` (restore without asking) and `--no-restore` decide for the whole
run, switches included.

Nothing is restored during a replay.

### Which session comes back

The last session of the same device: the same machine (hostname), port,
baud rate, data bits, parity, stop bits and encoding. It must also:

- have received something;
- not be running anymore;
- have ended within `restore_max_age` (`"0s"` means no limit).

Sessions from before the restore feature have no `.meta` and never come
back.

### The question

With `restore = "ask"`, Enter or `y` restores, and Esc or `n` skips. A click
outside the question doesn't answer it.

What arrives in the meantime waits, and shows after the restored part. If
more than 4 MiB arrives, the restore is skipped.

### What comes back

The last 1 MiB of the `.raw`, in both modes, cut into lines the way line
mode did live. Each line and row gets its time from the `.times`; a session
without one shows a blank time column.

The lines you sent in line mode were never received, so they don't come
back.

After the terminal part, any mode a full-screen program may have left on
(alternate screen, cursor keys, mouse reports, scroll region) is reset.

The restored part isn't backed up again or counted in RX. So the next launch
restores this session's own data, or the same old one again if this session
received nothing.

### Switching devices

A switch clears both modes and the RX / TX counts once the new device is
open, then restores as at startup. When you come back to the first device,
its own session comes back.

A device that fails to open leaves the screen as it was. New line settings
or a new encoding on the same port keep the history; only a new backup
starts.

## Session browser

`Ctrl-a j` (or Sessions… in the right-click menu) lists every backup on the
left, for any device. The right side shows the selected one: its device,
format, times, size and last lines.

<!-- screenshot: the session browser with a few sessions, one restorable -->

### Marks

| Mark | Means |
|---|---|
| `●` | Running; it can't be deleted |
| `»` | What this device would restore |
| `?` | No `.meta`: from before the restore feature |

### Keys

On top of the [popup keys](popups.md):

| Key | Does |
|---|---|
| `d`, `Del` | Delete the selected session, after a question |
| `D` | Delete all but the running ones, after a question |
| `s` | Sort by date (newest first) or by device |

## Replay

A replay plays a recording as if a device sent it. It takes:

- a recording's folder, its `.raw` or its `.meta`;
- any `.raw` file, even another tool's capture, played without a meta.

A `.log` is refused: its text isn't what was received.

### Starting one

| From | Write |
|---|---|
| The shell | `maxicom --replay <path>` |
| The command line | `replay <path>` or `open file://<path>` |

By default it plays as fast as it can, one buffer after the other.
`--delay <ms>`, or `replay <path> <ms>`, waits that long between two
buffers, to watch a boot scroll by.

### Controlling it

| Command | Does |
|---|---|
| `replay-pause` / `replay-resume` | Pauses / resumes |
| `replay-speed <ms>` | Changes the delay |
| `replay-restart` | Plays it again from the top, on a clean screen |
| `replay-stop` | Goes back to the live link |

Run them from the [command line](command-line.md), or bind them to keys.

While it plays, the status bar shows `▷ RUN 42%` (then `PAUSE` or `END`),
from `{replay_state}` and `{replay_percent}`.

### Back to the live link

A replay started from the app takes the live link's place, but the link
stays open behind it. To go back to it, use `Ctrl-a L`, a click on the
status bar's `« live`, or Back to live in the right-click menu.

Nothing is sent to a replay: commands that send are refused, and triggers
don't fire.

## Screenshots

Screenshots save the terminal's screen, in terminal mode only. In line mode,
the keys just say so.

| Key | The dialog opens on |
|---|---|
| `Ctrl-a k` (or Screenshot in the right-click menu) | `ansi`: `.ans`, with the colors |
| `Ctrl-a K` | `text`: `.txt`, the characters only |

The screen is taken at once, in both formats. A dialog then asks where to
save it: Enter saves to the path shown, Esc drops it, and Tab switches the
format.

The `screenshot <name>` command saves without the dialog, see
[Command line](command-line.md#search-and-screenshots).

## Paths and names

These rules apply to recordings and screenshots:

- a leading `~` is your home folder;
- missing folders are created;
- an empty `dir` is the current folder;
- names are strftime templates, filled in when the dialog opens.

## Settings

All in `config.toml`, shown with their defaults:

- `[capture]` sets the recordings; its text settings also apply to the
  session backup.
- `[sessions]` sets the session backup and the restore.
- `[screenshot]` sets the screenshots.

```toml
[capture]
dir = ""                                 # recordings folder
name = "maxicom_%Y%m%d_%H%M%S"           # no extension: .log, or a folder
timestamp = true                         # time on each text line
record_tx = true                         # sent lines in the text files (line mode)
tx_mark = "> "                           # before each sent line, "" for none

[sessions]
enabled = true                           # session backup (read at startup)
keep = 200                               # sessions kept
max_size = "200M"                        # all sessions, K/M/G/T, "0": no limit
restore = "always"                       # always, ask or never
restore_max_age = "1day"                 # since it ended, "0s": no limit

[screenshot]
dir = ""
name = "screen_%Y%m%d_%H%M%S"            # the format adds .txt or .ans
```
