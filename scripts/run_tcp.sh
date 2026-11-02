#!/usr/bin/env bash
set -euo pipefail

# A device reached over TCP, for maxicom's tcp:// links (MAX-156), like ser2net or an ESP32 bridge
#   ./scripts/run_tcp.sh                              a zsh on tcp://127.0.0.1:4000
#   ./scripts/run_tcp.sh 4001                         on another port
#   ./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200     a real serial port shared on the network
# Each connection gets its own shell (fork): detach / attach in maxicom starts a new one.
# Ctrl-C stops the server: maxicom shows the device lost, then reconnects when it runs again.
# Local only (bind=127.0.0.1): use bind=0.0.0.0 to reach it from another machine, it is a shell
# with no password.

PORT="${1:-4000}"
DEVICE="${2:-}"
BAUD="${3:-115200}"

if [[ -n "$DEVICE" ]]; then
  # socat opens the device on each connection: when that fails, it closes the connection at
  # once and maxicom loops between lost and reconnected. A busy port (another program, or a
  # maxicom holding it) shows as an `E ... Device or resource busy` line below
  if [[ ! -e "$DEVICE" ]]; then
    echo "no such device: $DEVICE" >&2
    exit 1
  fi
  if [[ ! -r "$DEVICE" || ! -w "$DEVICE" ]]; then
    echo "no read/write access to $DEVICE (is $USER in the dialout group?)" >&2
    exit 1
  fi
  TARGET="$DEVICE,raw,echo=0,b$BAUD"
else
  TARGET="exec:zsh -l,pty,stderr,setsid,sigint,sane"
fi

echo "listening on tcp://127.0.0.1:$PORT"
exec socat -d -d "tcp-listen:$PORT,bind=127.0.0.1,reuseaddr,fork" "$TARGET"

# cargo run --bin maxicom -- --device tcp://127.0.0.1:4000
