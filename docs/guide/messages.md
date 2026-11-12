# Messages

This page lists the warnings and errors maxicom shows, what each one means,
and what to do about it. Search it for the words of a message you got. A
part in `<angle brackets>` is filled in: a port, a file, a reason.

Messages show up in three places:

- **on the shell**, when maxicom stops before its screen opens;
- **in a popup**, for a failure you need to see;
- **in a [notification](notifications.md)**, in the top right corner.
  Errors stay 5 seconds, warnings 4.

A toggle also says its new state in a notification, like `Add CR ON` or
`Display Mode is hex`. Those aren't listed here.

## At startup, on the shell

maxicom prints these and exits.

| Message | Means | To do |
| --- | --- | --- |
| `Argument error: Unknown argument '<arg>'.` | An option maxicom doesn't know | `maxicom --help` lists them |
| `Argument error: Missing value for option '<option>'.` | An option that takes a value was last | Give its value: `-b 115200` |
| `Argument error: Invalid baud rate '<value>'`, `Invalid delay '<value>'` | A value that isn't a number | |
| `maxicom: No device: give one …` | No device given, none in `[link]`, no saved link, and no serial port found | Plug the board in, or give a device. See [Links](links.md#with-no-device-given). |
| `maxicom: could not open the device: <reason>` | The port given exists but doesn't open | See [Opening a link](#opening-a-link) below |
| `maxicom: could not open replay file <path>: not a recording (a .raw, its .meta or its folder)` | The path of `--replay` is a `.log` or another file | Give the recording's folder, `.raw` or `.meta`. See [Replay](capture.md#replay). |
| `maxicom: could not open replay file <path>: <reason>` | The file doesn't open | The reason comes from the system |
| `maxicom: invalid settings syntax in '<file>': …`, `invalid theme in …`, `invalid keybinding in …`, `invalid trigger in …`, `invalid plot in …` | A config file doesn't load. The message names the file, and the line or the culprit. | Fix it. See [Settings](settings.md#errors). |
| `maxicom: no cache folder on this system` | `--cache-dir` found no cache folder | |

A port that doesn't exist at startup doesn't stop maxicom: it waits for
the port, behind a "Connection Lost" popup.

## Opening a link

These come back as a notification, in the [directory](directory.md)'s
test column, or on the shell at startup.

| Message | Means | To do |
| --- | --- | --- |
| `permission denied opening serial port '<port>'` | Your user can't open the port | On Linux, join the `dialout` group (`uucp` on Arch). See [Troubleshooting](troubleshooting.md#permission-denied-linux). |
| `serial port '<port>' is held by <holder>` | Another program has the port | Close it, or detach it there. See [Port locks](locks.md#when-a-port-is-already-held). |
| `serial port '<port>' is busy (already in use by another application)` | The system refused the port as busy, without naming the holder | Find the program that has it open |
| `failed to open serial port '<port>' at <baud> baud: <reason>` | Any other open failure | The reason comes from the system |
| `could not connect to '<address>': <reason>` | A TCP host refused, or can't be reached | Check the host and port. See [TCP links](tcp-links.md). |
| `<label> is open in <holder> too` | A TCP link another maxicom also has open (warning only) | Nothing, if the server takes several clients |
| `Failed to reattach: <reason>` | `Ctrl-a D` couldn't open the port again; the link stays detached | Free the port, then `Ctrl-a D` again |

## While a link runs

| Message | Means | To do |
| --- | --- | --- |
| `Serial port lost`, `TCP connection lost` | The device went away; the Connection Lost popup shows | Nothing: it reopens when the device comes back |
| `Serial port detached` | `Ctrl-a D` closed it | `Ctrl-a D` opens it again |
| `Device idle for <n>s: port detached` | Nothing came in for `inactivity_detach_timeout` | `Ctrl-a D`. See [Idle links](links.md#idle-links). |
| `Serial port read error` | Reading failed; the link is treated as lost | |
| `break: <reason>` | The port refused the break | |
| `The port is detached: it opens with these settings` | New line settings while detached; they apply at the next attach | |
| Popup "Serial Conf Rejected" | The driver refused the new line settings, and the port kept its old ones. The details name the fields. | Pick settings the adapter supports |
| `A file transfer is running: cancel stops it` | A key typed during an `upload`, which owns the link; said once per upload | Wait for the end, or `cancel`. See [While it runs](file-transfers.md#while-it-runs). |

## Commands

A command that fails says why, prefixed with its name, and drops the rest
of its line: `(<n> more not run)` says how many commands were dropped.
See [Command line](command-line.md).

| Message | Means |
| --- | --- |
| `unknown command '<name>'` | No command has this name. Tab completes the names. |
| `<command>: missing argument <name>` | An argument is missing |
| `<command> takes no argument`, `takes <n> argument(s) at most` | Too many arguments |
| `<command>: bad argument <n>: <reason>` | An argument doesn't parse, like `'x' is not on/true/1 or off/false/0` or `odd number of hex digits` |
| `unclosed quote`, `bad escape '<text>'` | The line itself doesn't parse. See [Writing arguments](command-line.md#writing-arguments). |
| `a replay has no link to send to` | A command that sends, during a replay |
| `the port isn't open (detached or reconnecting)` | A command that needs the open port |
| `a TCP link has no <settings>, its bridge sets them` | `baud`, `dtr`, `break` and the like on a TCP link |
| `hardware flow control drives RTS (flow none first)` | `rts` with `flow rtscts` |
| `a file is being sent already (cancel stops it)` | A second `send-file` or `upload` |
| `a file transfer is running (cancel stops it)` | `send`, `send-hex` or `mash` during an `upload`. See [While it runs](file-transfers.md#while-it-runs). |
| `a mash runs (cancel stops it)` | `upload` while a `mash` runs |
| `a mash runs already (cancel stops it)` | A second `mash` |
| `mash: '<text>' didn't come in` | The mash timed out |
| `Mash stopped: <why>` | A key was typed, the link closed, or a replay started |
| `Stopped sending <file> at <n>%: <why>` | The write failed, the link closed, a replay started, or an upload failed. See [When an upload fails](file-transfers.md#when-an-upload-fails). |
| `no trigger named '<name>'`, `triggers.toml has no trigger` | `trigger` names no trigger. See [Triggers](triggers.md). |
| `no plot named '<name>'`, `plots.toml has no plot` | `plot` names no plot, or the file has none. See [Plots](plots.md). |
| `No plot shown` | `plot-freeze` with no plot shown |
| `screenshots are in terminal mode` | `screenshot` in line mode |
| `no replay runs`, `the replay isn't paused`, `the replay is finished …` | A replay command with nothing to act on |
| `a replay can't open a link (replay-stop ends it)` | `open` during a replay |

## Config files

| Message | Means | To do |
| --- | --- | --- |
| Popup "Config Reload Failed" | A file doesn't load; the running config stays. The details name the file and the line. | Fix it, then reload |
| `Config reloaded; restart to apply: <settings>` | Those settings are only read at startup | Restart maxicom. See [Reload](settings.md#reload). |
| `Keymap: unreachable <keys>` | Keys in `[line_mode]` or `[terminal_mode]` that the prefix or `[direct_actions]` take first | Bind them to other keys. See [Keybindings](keybindings.md#sections). |
| `Unknown keys in <file>: <keys>` | Keys of `config.toml`, `theme.toml` or `plots.toml` the app doesn't read, such as a typo: each takes its default | Fix or delete them, see [Unknown keys](settings.md#unknown-keys) |
| `invalid link in '<file>': link #<n> '<name>': <problem>`, `invalid syntax in '<file>'` | `links.toml` doesn't load; maxicom runs without saved links | Fix the file. See [Saved links](saved-links.md). |
| `changed in the file meanwhile: not saved` | `links.toml` changed since the directory read it | Open the directory again and redo it |
| `No plot named '<name>' in plots.toml: plot off` | `--plot` or `plot` in `config.toml` names no plot of the file | Fix the name. See [Plots](plots.md#showing-a-plot). |
| `Plot <name> is gone from plots.toml: plot off` | A reload removed the plot shown | Pick another one with `Ctrl-a g` |

## Recordings and sessions

| Message | Means | To do |
| --- | --- | --- |
| Popup "Capture Failed" | The recording's file or folder can't be created | Check the path and its rights |
| `Full recording stopped: the device changed` | Another port, line settings, encoding, or a replay | Start a new recording. See [Full recordings](capture.md#full-recordings). |
| `Session backup off: <reason>` | This session isn't backed up, usually because the cache folder can't be written | Check the folder: `maxicom --cache-dir` |
| `Session not restored: too much data came in while asking` | More than 4 MiB arrived while the restore question was up | See [The question](capture.md#the-question) |
| `Session not restored: <reason>` | The last session couldn't be read back | |
| `No session folder` | The session browser found no cache folder | |
| `A running session can't be replayed` | In the session browser, on a `●` session | |
| `Session not deleted: <reason>` | The session browser couldn't remove its folder | |
| `Failed to save screenshot: <reason>` | The screenshot's file can't be written | Check the path |

## The screen

| Message | Means | To do |
| --- | --- | --- |
| `Terminal window too small (<c>x<r>). Minimum is 30x8` | The window is under 30 columns or 8 rows | Make it bigger |
| `Line wrap: line mode only` | `Ctrl-a w` in terminal mode, where the screen always wraps | See [Terminal mode](terminal-mode.md#screen-size) |
| `Raw SideView is Off` | Resizing the side panel while it's hidden | `Ctrl-a B` shows it |
| `Clipboard error: <reason>` | The copy couldn't be sent to your terminal | See [Troubleshooting](troubleshooting.md#a-copy-doesnt-reach-the-clipboard) |
