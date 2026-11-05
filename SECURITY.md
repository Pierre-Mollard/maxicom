# Security

## Supported versions

maxicom is young: only the latest release gets security fixes.

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

## Reporting a vulnerability

Please report it privately, not in a public issue: on GitHub, open the
repository's **Security** tab, then **Report a vulnerability**
([direct link](https://github.com/Pierre-Mollard/maxicom/security/advisories/new)).
Only the maintainers see the report.

Say what you found, how to reproduce it (the maxicom version, the OS, the
device or the bytes involved), and what an attacker could do with it. The fix
ships in a new release, and the advisory is published with it, crediting you
unless you prefer otherwise.

## What is in scope

maxicom reads untrusted data from a device and writes files that other local
users may see. A report about any of these is welcome:

- **The bytes a device sends.** They are shown through maxicom's own
  terminal emulator and never reach your terminal as they are. A device
  shouldn't be able to crash or hang maxicom, change your terminal, or make
  maxicom send anything beyond the
  [answers to its queries](https://pierre-mollard.github.io/maxicom/terminal-mode.html#answering-the-devices-queries).
  Questions about the window title and the clipboard are never answered.
- **The lock files**, in folders shared by every local user:
  `/run/lock/maxicom` (or `/tmp/maxicom-locks`) on Linux, and
  `C:\ProgramData\maxicom\locks` on Windows. The folder is made open to all
  like `/tmp`, a symlink in its place is refused, and lock files are opened
  without following links.
- **The config files**, the saved links and the triggers, read from your
  config folder.
- **Session backups and recordings**, written to your cache folder and where
  you choose.

## Known limits

These are how maxicom works, not vulnerabilities:

- **TCP links aren't encrypted or authenticated.** Anyone on the network path
  can read the traffic or inject bytes. Use them on a trusted network, or
  through an SSH tunnel, see
  [TCP links](https://pierre-mollard.github.io/maxicom/tcp-links.html).
- **Session backups hold everything the device sent**, which can include
  passwords or keys it printed. They are written with your default file
  permissions. Delete them from the session browser (`Ctrl-a j`), or turn
  them off with `enabled = false` in `[sessions]`.
- **Lock files name their holder.** Each lock's `.info` file, readable by
  every local user, holds the process ID, the user, the host, the port and
  the time it was taken, so another program can say who holds a port.
- **maxicom trusts the device on the port.** It can't tell a real board
  from anything else wired or bridged to it.
