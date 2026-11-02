#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  echo
  echo "Stopping fake serial test..."

  if kill -0 "${FAKE_PID:-0}" 2>/dev/null; then
    kill "$FAKE_PID"
  fi

  if kill -0 "${SOCAT_PID:-0}" 2>/dev/null; then
    kill "$SOCAT_PID"
  fi
}

rm -f /tmp/maxicom /tmp/fake-device
socat -d -d \
  pty,raw,echo=0,link=/tmp/maxicom \
  pty,raw,echo=0,link=/tmp/fake-device &
SOCAT_PID=$!

cargo run --bin fake_device &
FAKE_PID=$!

trap cleanup EXIT

wait
# cargo run --bin maxicom -- --device /tmp/maxicom --baudrate 115200
# or another terminal open /dev/maxicom to interact with fake (like minicom)
