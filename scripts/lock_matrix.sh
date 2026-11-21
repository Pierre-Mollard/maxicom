#!/usr/bin/env bash
set -euo pipefail

# MAX-268: who keeps whom out of a port, maxicom and minicom, in containers (needs docker and
# `rustup target add x86_64-unknown-linux-musl`):
#   ./scripts/lock_matrix.sh [image...]    default rockylinux/rockylinux:9 debian:stable ubuntu:24.04
# For each image, 54 runs: maxicom first then minicom, and the other way; the second program
# run by the same user, another user, or root (with CAP_SYS_ADMIN, as `sudo`: it ignores
# exclusive mode); each one opening the port as the device, an alias in /dev or an alias
# outside /dev. The port is a socat pty bind-mounted as /dev/ttyFAKE<n>, a name like a real
# port's (a node made with mknod can't open a pty). After each start, root reads the
# programs' open fds (CAP_SYS_PTRACE) to tell who holds the port.
# Prints a grid per order, user and image: two letters per run, the first program's path then
# the second's (D device, A alias in /dev, O alias outside /dev), and
#   ok       the second was kept out
#   BOTH     both hold the port
#   nofirst  the first program didn't take it (RHEL's minicom refuses a link outside /dev)
# Raw results in target/lock-matrix/*.csv. Runs take about 6 minutes, images in parallel.

cd "$(dirname "$0")/.."
command -v docker >/dev/null || { echo "missing docker" >&2; exit 1; }
[ $# -gt 0 ] || set -- rockylinux/rockylinux:9 debian:stable ubuntu:24.04
cargo build --release --target x86_64-unknown-linux-musl --bin maxicom
BIN=target/x86_64-unknown-linux-musl/release/maxicom
OUT=target/lock-matrix
rm -rf "$OUT"
mkdir -p "$OUT"

run() {
  docker run --rm -i --cap-add SYS_PTRACE --cap-add SYS_ADMIN \
    -v "$PWD/$BIN:/usr/local/bin/maxicom:ro" "$1" bash -s <<'EOF'
set -u
if command -v dnf >/dev/null; then
  dnf -q -y install minicom socat procps-ng util-linux >/dev/null 2>&1
  command -v script >/dev/null || dnf -q -y install util-linux-script >/dev/null 2>&1
else
  apt-get -qq update >/dev/null
  DEBIAN_FRONTEND=noninteractive apt-get -qq install -y minicom socat procps bsdutils >/dev/null 2>&1
fi
DISTRO=$(. /etc/os-release; echo "$ID-$VERSION_ID")
useradd -m alice
useradd -m bob

holds() { # program -> yes/no: a process of that name has the port open
  local p
  for p in $(pgrep -x "$1"); do
    ls -l /proc/$p/fd 2>/dev/null | grep -qE " ($DEV|$PTS)\$" && { echo yes; return; }
  done
  echo no
}

start() { # user command: in a tty of its own, its input kept open
  local run="sleep 600 | script -qfc 'TERM=xterm $2' /dev/null"
  if [ "$1" = root ]; then
    bash -c "$run" >/dev/null 2>&1 &
  else
    runuser -u "$1" -- bash -c "$run" >/dev/null 2>&1 &
  fi
}

path() {
  case $1 in
    device) echo "$DEV" ;;
    dev-alias) echo "/dev/board$n" ;;
    outside-alias) echo "/tmp/myboard$n" ;;
  esac
}

command_of() {
  if [ "$1" = maxicom ]; then echo "maxicom --device $2"; else echo "minicom -D $2"; fi
}

n=0
for order in maxicom-first minicom-first; do
for users in same other root; do
for p1 in device dev-alias outside-alias; do
for p2 in device dev-alias outside-alias; do
  n=$((n+1))
  socat pty,raw,echo=0,link=/tmp/port$n,perm=0666 pty,raw,echo=0,link=/tmp/far$n 2>/dev/null &
  for _ in $(seq 50); do [ -e /tmp/port$n ] && break; sleep 0.05; done
  PTS=$(readlink -f /tmp/port$n)
  DEV=/dev/ttyFAKE$n
  touch "$DEV"
  mount --bind "$PTS" "$DEV"
  ln -s "$DEV" /dev/board$n
  ln -s "$DEV" /tmp/myboard$n
  case $users in same) second=alice ;; other) second=bob ;; root) second=root ;; esac
  if [ $order = maxicom-first ]; then a=maxicom; b=minicom; else a=minicom; b=maxicom; fi
  start alice "$(command_of $a "$(path $p1)")"
  sleep 2
  h1=$(holds $a)
  start $second "$(command_of $b "$(path $p2)")"
  sleep 3
  h2=$(holds $b)
  echo "$DISTRO,$order,$users,$p1,$p2,$h1,$h2"
  pkill -9 -x minicom; pkill -9 -x maxicom; pkill -9 -x script; pkill -9 -f 'sleep 600'
  pkill -9 -x socat
  sleep 0.5
  umount "$DEV"
  rm -f "$DEV" /dev/board$n /tmp/myboard$n /run/lock/LCK.* /run/lock/LTMP.* \
    /run/lock/lockdev/* /var/lock/LCK.* 2>/dev/null
  rm -rf /tmp/maxicom-locks
done; done; done; done
EOF
}

for image in "$@"; do
  run "$image" >"$OUT/$(echo "$image" | tr '/:' '__').csv" 2>/dev/null &
done
wait

python3 -I - "$OUT"/*.csv <<'EOF'
import csv, sys
rows = [row for name in sys.argv[1:] for row in csv.reader(open(name))]
kinds = {"device": "D", "dev-alias": "A", "outside-alias": "O"}
distros = sorted({row[0] for row in rows})

def result(row):
    first, second = row[5], row[6]
    if first == "no":
        return "nofirst"
    return "BOTH" if second == "yes" else "ok"

for order in ["maxicom-first", "minicom-first"]:
    print(f"## {order}")
    for users in ["same", "other", "root"]:
        for distro in distros:
            cells = []
            for p1, k1 in kinds.items():
                for p2, k2 in kinds.items():
                    found = [r for r in rows if r[:5] == [distro, order, users, p1, p2]]
                    cells.append(f"{k1}{k2}:{result(found[0]) if found else '?'}")
            print(f"{users:5} {distro:13} {' '.join(cells)}")
EOF
