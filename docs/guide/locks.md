# Port locks

While a link is open, maxicom holds locks that every user's programs can
see. This keeps two programs from sharing a port by mistake, which would
split the device's output between them.

## The locks maxicom takes

### On a serial port

- **A UUCP lock file**, `LCK..ttyUSB0`, holding maxicom's PID, as the
  programs using lock files expect. maxicom makes it before opening the
  port, so two programs can't both take it. It is only written where the
  system lets users write one:

  | System | Folder |
  | --- | --- |
  | Debian, Ubuntu | `/run/lock` |
  | Fedora, RHEL, Rocky, Alma | `/run/lock/lockdev`, only for members of the `lock` group |
  | macOS | none |

- **A `flock` on the device** (Linux, macOS). picocom, tio and pyserial take
  the same kind of lock.
- **Exclusive mode** (Linux, macOS): the system itself refuses the port to
  any other program that tries to open it, whoever the user, with "Device or
  resource busy". This is what keeps other programs out where no lock file
  can be written, as on Fedora and RHEL.

### On any link

**A lock in a folder shared by all users**, with a `.info` file beside it
giving the PID, the user, the machine, the address and the start time:

| System | Folder |
| --- | --- |
| Linux, macOS | `/run/lock/maxicom` where the system lets users create it (Debian, Ubuntu), else `/tmp/maxicom-locks` |
| Windows | `%ProgramData%\maxicom\locks` |

This is how other maxicoms, and the [directory](directory.md)'s test, know
who holds a link.

## When a port is already held

maxicom refuses to open a port another program holds, and names the holder:

```text
held by maxicom 4242 (pierre)
held by picocom (1234)
```

The check happens before the port is opened, so the board doesn't get a DTR
pulse (and a reset) for nothing.

A port counts as held when:

- any user's program holds a lock on it;
- on Linux, one of your own programs has it open, with or without a lock.

A pseudo-terminal (pty) only counts as held through a lock, since the
program that created it (socat, for example) always has it open too.

To use a port another maxicom holds, detach it there with `Ctrl-a D`.

### Lock files

maxicom reads the lock files in every usual lock folder: `/var/lock`,
`/run/lock`, `/var/spool/lock`, `/var/spool/uucp` and a few older ones, and
the `lockdev` folder inside each, where Fedora and RHEL keep theirs.

A program names its lock file after the path it was given, which may be an
alias of the device. maxicom leads each name back to the device, so a port is
held whichever path each program uses:

| The other program opened | Its lock file | Found |
| --- | --- | --- |
| `/dev/ttyUSB0` | `LCK..ttyUSB0` | yes |
| a link in `/dev`: `/dev/board`, `/dev/serial/by-id/usb-…` | `LCK..board`, `LCK..serial_by-id_usb-…` | yes |
| a path in `/dev`, on Fedora and RHEL | also `LK.000.188.000`, the device's numbers | yes, whatever its name |
| `/dev/cu.usbserial-1` (macOS) | `LCK..cu.usbserial-1` | yes, for `/dev/tty.usbserial-1` too |
| a link outside `/dev`: `~/board` (Debian, Ubuntu) | `LCK..board` | only when maxicom opens a link of that name too |

### What can't be seen

Some holders leave nothing maxicom can find:

- another user's program that holds the port without any lock;
- a lock file named after a link outside `/dev`, as in the last row above;
- a lock file whose PID belongs to another container, which means nothing
  outside it.

Exclusive mode still keeps out every program started after maxicom, except
those run with `sudo`, which the system lets through.

## TCP links

A TCP link that another maxicom has open still opens, because some servers
accept several clients. maxicom warns you and names the other one. The
lock is the server's port: `tcp://`, `telnet://` and `rfc2217://` on the same
`host:port` count as one.

## After a crash

A crash leaves nothing in the way:

- the system drops the `flock` and the shared folder's lock along with the
  process;
- a UUCP lock file whose process is gone is stale, and the next program
  replaces it.

A lock file with no PID in it, empty or unreadable, is different: it may be
another program's lock, written at that very moment. maxicom never removes
one. It refuses the port and names the file:

```text
held by a lock with no PID, /run/lock/LCK..ttyUSB0 (remove it if stale)
```

If no program uses the port, delete that file by hand.

On a normal close, maxicom removes its locks itself.
