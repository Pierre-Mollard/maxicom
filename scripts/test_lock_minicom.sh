#!/usr/bin/env bash
set -euo pipefail

# MAX-268: another user's minicom holds a port first, maxicom must refuse it. Runs the ignored
# test `other_users_minicom_refused` in a container (needs docker):
#   ./scripts/test_lock_minicom.sh [image] [path]
#     image   rockylinux/rockylinux:9 (default, RHEL 9), rockylinux/rockylinux:10, almalinux:8,
#             fedora:latest: minicom locks through lockdev; debian:stable: minicom's own files
#     path    how minicom opens the port: the pty itself (default), or an alias made for it,
#             `/dev/board` (as udev makes one) or `/tmp/myboard` (a link outside /dev)
# In the container, as root: user `other` runs minicom on a socat pty (under `script`, for a
# tty), then user `me` runs the test binary built here on the pty itself, in MAXICOM_HELD_PORT.
# The binary is built for musl, static, so it runs on any image (rustup target add
# x86_64-unknown-linux-musl).

IMAGE="${1:-rockylinux/rockylinux:9}"
ALIAS="${2:-}"
cd "$(dirname "$0")/.."

command -v docker >/dev/null || { echo "missing docker" >&2; exit 1; }
BIN=$(cargo test --no-run --target x86_64-unknown-linux-musl 2>&1 | sed -n 's/.*Executable unittests src\/main.rs (\(.*\))/\1/p')
[ -n "$BIN" ] || { echo "test binary not found" >&2; exit 1; }

docker run --rm -i -e ALIAS="$ALIAS" -v "$PWD/$BIN:/maxicom-tests:ro" "$IMAGE" bash -s <<'EOF'
set -euo pipefail
if command -v dnf >/dev/null; then
  dnf -q -y install minicom socat procps-ng util-linux >/dev/null
  # `script` has a package of its own on Fedora and RHEL 10
  command -v script >/dev/null || dnf -q -y install util-linux-script >/dev/null
else
  apt-get -qq update >/dev/null
  DEBIAN_FRONTEND=noninteractive apt-get -qq install -y minicom socat procps >/dev/null
fi
useradd -m other
useradd -m me
socat pty,raw,echo=0,link=/tmp/port,perm=0666 pty,raw,echo=0,link=/tmp/board 2>/dev/null &
for _ in $(seq 50); do [ -e /tmp/port ] && break; sleep 0.1; done
PORT=$(readlink -f /tmp/port)
OPEN="$PORT"
if [ -n "$ALIAS" ]; then ln -s "$PORT" "$ALIAS"; OPEN="$ALIAS"; fi
runuser -u other -- script -qfc "TERM=xterm minicom -D $OPEN" /dev/null </dev/null >/dev/null &
for _ in $(seq 50); do ls /run/lock /run/lock/lockdev 2>/dev/null | grep -q '^L' && break; sleep 0.1; done
echo "minicom of other on $OPEN ($PORT), lock files:"
ls -l /run/lock /run/lock/lockdev 2>/dev/null || true
pgrep -a minicom || echo "minicom not running"
runuser -u me -- env MAXICOM_HELD_PORT="$PORT" /maxicom-tests \
  --ignored --exact link::lock::tests::other_users_minicom_refused
EOF
