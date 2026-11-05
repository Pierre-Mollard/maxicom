# Port locks

While a link is open, maxicom holds locks that every user's programs can
see. This keeps two programs from sharing a port by mistake, which would
split the device's output between them.

## The locks maxicom takes

### On a serial port

- **A `flock` on the device** (Linux, macOS). picocom, tio and pyserial take
  the same kind of lock.
- **A UUCP lock file**, `LCK..ttyUSB0`, holding maxicom's PID, as the
  programs using lock files expect. It is only written where the system lets
  users write one: `/run/lock` on Debian and Ubuntu. Fedora doesn't allow
  it, so it is skipped there.

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

## TCP links

A TCP link that another maxicom has open still opens, because some servers
accept several clients. maxicom warns you and names the other one.

## After a crash

A crash leaves nothing in the way:

- the system drops the `flock` and the shared folder's lock along with the
  process;
- a UUCP lock file whose process is gone is stale, and the next program
  replaces it.

On a normal close, maxicom removes its locks itself.
