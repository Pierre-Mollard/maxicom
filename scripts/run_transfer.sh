#!/usr/bin/env bash
set -euo pipefail

# Fake devices for maxicom's `upload` and `download` (MAX-162), on tcp://127.0.0.1:PORT
# (default 4100)
#   ./scripts/run_transfer.sh ymodem [port]       lrzsz rb: files land in /tmp/maxicom-rcv/
#   ./scripts/run_transfer.sh xmodem [port]       lrzsz rx, checksum: /tmp/maxicom-rcv/out.bin
#   ./scripts/run_transfer.sh xmodem-crc [port]   lrzsz rx -c, CRC: /tmp/maxicom-rcv/out.bin
#   ./scripts/run_transfer.sh uboot [port]        U-Boot in QEMU: press a key to stop autoboot,
#                                                 `loady ${loadaddr}` (or `loadx`), upload, then
#                                                 `crc32 ${loadaddr} ${filesize}`
#   ./scripts/run_transfer.sh files               test files in /tmp/maxicom-files/, CRC-32 listed
#   ./scripts/run_transfer.sh send-ymodem [port]  lrzsz sb sends 3 test files, for `download ymodem`
#   ./scripts/run_transfer.sh send-xmodem [port]  lrzsz sx sends rand-54321.bin with 1K blocks,
#                                                 for `download xmodem`
# Then: maxicom tcp://127.0.0.1:4100 and `upload ymodem /tmp/maxicom-files/rand-100k.bin`,
# checked with `cmp` (XMODEM pads the last block with 0x1A: `cmp -n <size>`).
# The fake receivers start a new receiver per connection: detach / attach to send again. rx and
# rb ask again every 10 s or so: an upload typed after their first ask waits for the next one.
# U-Boot waits for maxicom to connect before it boots. Ctrl-C stops it.

MODE="${1:-}"
PORT="${2:-4100}"
RCV=/tmp/maxicom-rcv
FILES=/tmp/maxicom-files
UBOOT=/usr/lib/u-boot/qemu_arm64/u-boot.bin

need() {
  command -v "$1" >/dev/null || { echo "missing $1: sudo apt install $2" >&2; exit 1; }
}

receive() {
  need socat socat
  need rb lrzsz
  mkdir -p "$RCV"
  echo "lrzsz on tcp://127.0.0.1:$PORT, in $RCV/ (Ctrl-C stops)"
  # rb / rx talk to maxicom on stdout, their progress goes to this terminal
  socat TCP-LISTEN:"$PORT",bind=127.0.0.1,reuseaddr,fork SYSTEM:"cd $RCV && $1"
}

# the senders send test files: `files` makes them
send() {
  [[ -f "$FILES/rand-54321.bin" ]] || { echo "no test files: run $0 files first" >&2; exit 1; }
  RCV="$FILES" receive "$1"
}

case "$MODE" in
  ymodem) receive "rb -vv -y" ;;
  # rx opens its file at once: maxicom reconnecting after a transfer starts a new one, which
  # would empty out.bin. Each writes its own file, renamed when complete
  xmodem) receive 'rx -vv part-$$.bin && mv part-$$.bin out.bin || rm -f part-$$.bin' ;;
  xmodem-crc) receive 'rx -vv -c part-$$.bin && mv part-$$.bin out.bin || rm -f part-$$.bin' ;;
  # socat eats quotes: a glob passes the name with spaces
  send-ymodem) send "sb -vv rand-100k.bin name*spaces.bin rand-129.bin" ;;
  send-xmodem) send "sx -vv -k rand-54321.bin" ;;
  uboot)
    need qemu-system-aarch64 qemu-system-arm
    [[ -f "$UBOOT" ]] || { echo "missing $UBOOT: sudo apt install u-boot-qemu" >&2; exit 1; }
    echo "U-Boot waiting for maxicom on tcp://127.0.0.1:$PORT (Ctrl-C stops)"
    qemu-system-aarch64 -M virt -cpu cortex-a57 -m 512 -display none -monitor none \
      -bios "$UBOOT" -serial tcp:127.0.0.1:"$PORT",server=on,wait=on
    ;;
  files)
    mkdir -p "$FILES"
    : > "$FILES/empty.bin"
    for size in 1 128 129 1024 1025 54321; do
      head -c "$size" /dev/urandom > "$FILES/rand-$size.bin"
    done
    head -c 102400 /dev/urandom > "$FILES/rand-100k.bin"
    head -c 2097152 /dev/urandom > "$FILES/rand-2m.bin"
    head -c 3000 /dev/urandom > "$FILES/name with spaces.bin"
    # U-Boot's `crc32` prints the same value
    python3 -I - "$FILES" <<'EOF'
import os, sys, zlib
for name in sorted(os.listdir(sys.argv[1])):
    data = open(os.path.join(sys.argv[1], name), "rb").read()
    print(f"{zlib.crc32(data):08x}  {len(data):>8}  {name}")
EOF
    ;;
  *)
    sed -n '4,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
