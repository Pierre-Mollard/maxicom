# TCP links

A TCP link reaches the device's UART through a bridge: a program or a box
that copies bytes between a TCP connection and the UART.

```bash
maxicom tcp://192.168.1.50:4000
maxicom telnet://192.168.1.50:2000
maxicom rfc2217://192.168.1.50:3335
```

## Picking the scheme

The scheme at the start of the address says what the bridge speaks. Pick
the one its server is set to.

| Scheme | Speaks | Line settings and control lines |
| --- | --- | --- |
| `tcp://` | Raw bytes, as they are | Set by the bridge |
| `telnet://` | Telnet: the device's bytes, plus the server's option negotiation | Set by the bridge |
| `rfc2217://` | Telnet with RFC 2217, which lets the client set the server's serial port | Set by maxicom, as on a serial port |

A server in telnet mode reached with `tcp://` shows a few unreadable bytes
when you connect, such as `ÿû`: its option negotiation. Use `telnet://` for
it instead.

`--tcp 192.168.1.50:4000` on the command line is the same as
`tcp://192.168.1.50:4000`.

## What can serve one

| Bridge | Scheme | Notes |
| --- | --- | --- |
| A TCP-UART bridge in the device's firmware | `tcp://` | ESP32: ESP-Link, the ESPHome stream server, or your own. No cable, and no USB forwarding from WSL2. |
| ser2net | Any | On the machine the board is plugged into: a `tcp` accepter for `tcp://`, `telnet` for `telnet://`, `telnet(rfc2217)` for `rfc2217://`. |
| socat | `tcp://` | Shares a port on the network, raw. |
| esptool's `esp_rfc2217_server` | `rfc2217://` | Shares an ESP32's port, reset and bootloader keys included. |
| pyserial's `rfc2217_server.py` | `rfc2217://` | A small example server, on any machine with Python. |
| A serial device server | Any | Moxa, Lantronix, Digi: raw TCP, telnet or RFC 2217 mode. |
| A console server | `telnet://` | Opengear, Cisco reverse telnet. |
| An emulator | `tcp://` | QEMU with `-serial tcp::4444,server,nowait`, Renode. |
| A debug probe's RTT logs | `tcp://` | J-Link on `localhost:19021`, OpenOCD's `rtt server`. |

### Sharing a port from another machine

On the machine with the board, the repository's script starts a raw bridge:

```bash
./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200
```

To set the line from maxicom, share the port with RFC 2217 instead. With
ser2net, in its YAML file:

```yaml
connection: &board
  accepter: telnet(rfc2217),tcp,3335
  connector: serialdev,/dev/ttyUSB0,115200n81,local
```

With esptool, for an ESP32 on a Windows machine, while maxicom runs in WSL2
without USB forwarding:

```bash
esp_rfc2217_server -p 4000 COM3
maxicom rfc2217://172.29.64.1:4000
```

In WSL2's default NAT mode, Windows is the address of WSL's default route,
given by `ip route show default`, and it changes after a reboot. With
`networkingMode=mirrored` in `.wslconfig`, `localhost` works instead. The
Windows firewall must let Python accept connections on public networks.

esptool's server resets the board each time a client connects or
disconnects, before maxicom sends anything: a detach, an attach or a restart
of maxicom resets it. It also runs the reset and bootloader sequences
itself, from the first DTR or RTS change of the profile's keys, and doesn't
answer those changes.

### Through an SSH tunnel

maxicom has no SSH or TLS of its own. To reach a bridge on another machine
over an encrypted link, forward its port with `ssh -L`. The login and the
encryption come from your own `ssh`, and this works for the three schemes:

```bash
ssh -L 4000:localhost:3335 user@lab-pc
maxicom rfc2217://localhost:4000
```

Or run maxicom on that machine, over SSH:

```bash
ssh -t user@lab-pc maxicom /dev/ttyUSB0
```

## How it differs from a serial port

### Telnet

On a `telnet://` or `rfc2217://` link, maxicom asks the server for binary
mode both ways, so bytes go through as they are. It refuses the other
telnet options, such as echo, the window size or the terminal type: they're
for a login shell, and the device is a serial console.

The telnet commands never reach the screen. The recordings, the session's
`.raw` file and the [raw side panel](raw-side-panel.md) all get the bytes
the device sent.

### Line settings on a raw or telnet link

The bridge sets the baud rate and the framing, so maxicom has none to set:

- the status bar shows `raw TCP` or `telnet TCP` in their place (the
  `line_settings` condition hides them);
- the line settings popup only shows the address;
- `--baudrate`, `--bits`, `--parity`, `--stop-bits`, `--flow`, `--dtr` and
  `--rts` are refused;
- `baud`, `framing`, `flow`, `dtr`, `rts` and `break` are refused.

### Line settings on an RFC 2217 link

An `rfc2217://` link works like a serial port. The status bar shows its baud
rate and framing, and the [line settings](links.md#line-settings) popup, the
options and the commands all work. The control lines work too, so a
[profile](profiles.md) that resets the board on open, or on a key, does it
on a remote board.

maxicom sends the line settings, then
[DTR and RTS on open](links.md#dtr-and-rts-on-open), as soon as the server
agrees to RFC 2217. It sends them again on every reconnect.

maxicom doesn't wait for the server's answers, so the screen never freezes.
When the server answers with a different value than the one asked, a
warning says so, such as `the server set 9600 baud, asked 115200 baud`.

A server that doesn't speak RFC 2217, such as a plain telnet port, refuses
it when you connect. A warning says `the server refuses RFC 2217: no line
settings`. The bytes still flow, but the line settings and the control lines
are refused until you open the link again.

### Connecting and reconnecting

The UI never waits for the network. While it connects, it shows
"Connecting to …".

A host that is down, or not listening yet, shows "Connection Lost", and
maxicom retries every 500 ms, as for an unplugged port. Detach and attach
work the same as on a serial port.

A link that dies silently, because the device was powered off or the Wi-Fi
dropped, is detected by TCP keepalive after about 11 seconds, then retried.

### Timing

Typed keys are sent at once. Timestamps record when the bytes arrived from
the network, not when they crossed the wire on the device's side.

On an RFC 2217 link, the network sits between each DTR or RTS change and
the next. On a local network this adds little to a reset sequence with
`sleep` steps of 50 ms or more, such as the ESP32 profile's; a slow or distant
link can stretch it.

### Sessions

Session backups are kept per address. A session is restored only on the
same address, and its files are named after it: `192.168.1.50_4000`. An
RFC 2217 session also records its line settings, as a serial one does.

## Tips

Prefer an IP address to a `.local` name: mDNS names may not resolve in WSL2.
