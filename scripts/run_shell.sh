#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  echo
  echo "Stopping interactive serial test..."

  if kill -0 "${SOCAT_PID:-0}" 2>/dev/null; then
    kill "$SOCAT_PID"
  fi
}

rm -f /tmp/maxicom-shell-test 2>/dev/null || true
exec socat -d -d \
  pty,raw,echo=0,link=/tmp/maxicom-shell-test \
  exec:"zsh -l",pty,stderr,setsid,sane

# cargo run --bin maxicom -- --device /tmp/maxicom-shell-test --baudrate 115200
# or another terminal open /tmp/maxicom-shell-test to interact with interactive shell (like minicom)
