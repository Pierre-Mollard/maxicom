# TCP links

A TCP link reaches the device's UART through a bridge: a program or a box
that copies bytes between a TCP connection and the UART.

```bash
maxicom tcp://192.168.1.50:4000
```

## What can serve one

| Bridge | Notes |
|---|---|
| A TCP-UART bridge in the device's firmware | ESP32: ESP-Link, the ESPHome stream server, or your own. No cable, and no USB forwarding from WSL2. |
| ser2net or socat | On the machine the board is plugged into, sharing its port on the network. |
| A serial device server | Moxa, Lantronix, Digi, in their raw TCP mode. |
| An emulator | QEMU with `-serial tcp::4444,server,nowait`, Renode. |
| A debug probe's RTT logs | J-Link on `localhost:19021`, OpenOCD's `rtt server`. |

### Sharing a port from another machine

On the machine with the board, the repository's script starts a bridge:

```bash
./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200
```

To reach it from elsewhere over an encrypted tunnel:

```bash
ssh -L 4000:localhost:4000 pi
maxicom tcp://localhost:4000
```

## How it differs from a serial port

### Raw bytes only

maxicom speaks raw TCP, not telnet. A ser2net "telnet" port, or any server
in telnet mode, shows a few unreadable bytes when you connect: its option
negotiation. Switch the server to its raw mode.

### No line settings

The bridge sets the baud rate and the framing, so maxicom has none to set:

- the status bar shows `raw TCP` in their place (the `line_settings`
  condition hides them);
- the line settings popup only shows the address;
- `--baudrate`, `--bits`, `--parity`, `--stop-bits` and `--flow` are
  refused.

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

### Sessions

Session backups are kept per address. A session is restored only on the
same address, and its files are named after it: `192.168.1.50_4000`.

## Tips

Prefer an IP address to a `.local` name: mDNS names may not resolve in WSL2.
