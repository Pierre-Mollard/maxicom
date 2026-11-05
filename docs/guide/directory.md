# The directory

The directory lists everything you can open: your saved links, and the
serial ports the system sees. Open it with `Ctrl-a d`, the `directory`
command, or Directory in the right-click menu.

<!-- screenshot: the directory on the Saved links tab, a test result in the State column -->

## The two tabs

| Tab | Columns |
|---|---|
| Saved links | Name, Address, Line (the line settings), State |
| Serial ports | Port (its path), Device, ID, State |

Switch tabs with Tab, Left / Right, or a click on the tab.

The directory opens on Saved links when you have at least one, with the link
in use selected. `●` marks the link in use.

On Linux, a USB port's Device is its maker and product
(`Silicon Labs CP2102 USB to UART Bridge Controller`), and its ID the
`vid:pid` (`10c4:ea60`), both read from sysfs. On-board UARTs and other
systems leave them empty.

The State column shows `in use`, `opening…`, or the result of a
[test](#testing-the-ports-and-links).

A column that is empty on every row is hidden. On a narrow screen, the
address or the device column is shortened.

## Opening a link

Select a row and press Enter:

- a saved link opens with its own line settings;
- a serial port opens with the current line settings, like the `open`
  command.

The directory stays open, showing `opening…` on the row, until the link is
open. If it fails, the row says why, and the link you were using comes back
with its line settings.

## Keys

On top of the [popup keys](popups.md):

| Tab | Key | Does |
|---|---|---|
| Saved links | `n` | Create a new saved link |
| Saved links | `e` | Edit the selected link |
| Saved links | `s` | Save the link in use |
| Saved links | `x`, `Del` | Delete the selected link, after a question |
| Serial ports | `s` | Save the selected port as a link |
| Serial ports | `r` | List the ports again (USB ports come and go) |
| Both | `t` | Test the ports and the saved links |

`n`, `e` and `s` open the saved link form, see
[Saved links](saved-links.md#editing-in-the-app).

## Testing the ports and links

`t` tests every saved link and every serial port, and writes the result in
the State column. Each address is tested once: a saved link on a listed port
shares the port's result. The link in use isn't tested.

A test that gets no answer within 2 seconds shows `no answer`.

### Serial ports

| Result | Means |
|---|---|
| `free` | Nothing holds the port |
| `busy` | Another program holds it, named when known: `busy: picocom (1234)`, `busy: maxicom 4242 (pierre)` |
| `in use` | This maxicom has it open, under another name of the same device |
| `no access: join dialout` | You lack the permission; the group named is the one that owns the port |
| `not found` | The port is gone (unplugged) |
| `no lock` | macOS only: no lock holds it (see below) |

### TCP links

| Result | Means |
|---|---|
| `reachable` | The connection was accepted |
| `refused` | Nothing listens on that port |
| `unreachable` | The host can't be reached |
| `unknown host` | The name doesn't resolve |

The test connects once and closes at once. Keep in mind that the device sees
a connection, and a bridge that takes a single client may drop the one it
has.

### How a port is tested

Opening a port raises DTR and RTS, which resets boards with an auto-reset
circuit (ESP32, Arduino). So on Linux and macOS, the test never opens a
port. It looks, in order, at:

1. maxicom's own [locks](locks.md);
2. the UUCP lock files that minicom and lockdev write, minicom's naming
   included (`LCK..pts_3`);
3. `/proc/locks`, for the `flock` taken by picocom, tio, pyserial and
   maxicom;
4. the open files of the processes in `/proc`, for your own processes only;
5. the port's permissions.

The first three show any user's program. A port held by another user's
program without a lock (screen, cu) can't be seen without root, and shows
`free`.

macOS has no `/proc`, so a port that no lock holds shows `no lock` rather
than `free`.

On Windows, only opening the port tells whether it's free. The test opens it
with its settings unchanged, which may reset a board. A port held by another
maxicom is found by its lock instead, without opening it.
