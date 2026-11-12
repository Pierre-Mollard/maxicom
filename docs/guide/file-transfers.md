# File transfers

maxicom sends a file in two ways:

| Command | Sends | For |
| --- | --- | --- |
| `send-file <path> [line-delay-ms]` | The bytes as they are, paced to the baud rate | A script pasted into a shell, a text file to a console |
| `upload <protocol> <path>` | The file in XMODEM or YMODEM packets, each one checked and sent again if damaged | A bootloader or a program waiting for a file: U-Boot's `loady` and `loadx`, `rx`, `rb`, many MCU bootloaders |

Both run from the [command line](command-line.md) (`Ctrl-a :`). The path
completes from the disk, `~` is your home folder, and a path with spaces goes
in quotes. One file is sent at a time.

## Sending a file as it is

`send-file` sends the file's bytes with no framing, paced to the baud rate and
framing (10 bits a byte in 8N1, 12 in 8E2). The screen stays live, and the
file can be any size.

With a delay, it waits that long after each line, for a device without flow
control that handles one line at a time:

```text
send-file ~/scripts/setup.sh 50
```

The link stays yours while it runs: what you type and `send` go out between
its chunks. The rest of the command line doesn't wait for the file either:
`send-file a; send b` sends `b` first.

## Uploading with XMODEM or YMODEM

`upload` sends the file with a protocol the receiving side drives. The receiver
asks for the file, checks each packet, and asks for a damaged one again. The
protocols are built into maxicom: it runs no other program.

```text
upload ymodem ~/build/u-boot.itb
```

### Which protocol

| Protocol | Packets | The receiver gets | Receivers |
| --- | --- | --- | --- |
| `ymodem` | 1024 bytes | The file's name, its exact size and its date | U-Boot `loady`, `rb`, most YMODEM bootloaders |
| `xmodem-1k` | 1024 bytes | The data, its last packet padded | U-Boot `loadx`, `rx` |
| `xmodem` | 128 bytes | The data, its last packet padded | Old or small bootloaders that only know XMODEM |

XMODEM doesn't send the size, so the received file ends with up to 1023 padding
bytes (`0x1A`). Most bootloaders don't mind: the image says its own size. Pick
YMODEM when the receiver knows it.

The receiver also picks how packets are checked: a simple checksum or a CRC.
1024-byte packets need the CRC, so `xmodem-1k` and `ymodem` fall back to
128-byte packets when the receiver asks for the checksum.

### An example with U-Boot

At the U-Boot prompt, start the receiver, then the upload:

```text
=> loady ${loadaddr}
## Ready for binary (ymodem) download to 0x40200000 at 115200 bps...
C
```

```text
upload ymodem ~/build/kernel.itb
```

When it ends, U-Boot prints the size it got:

```text
## Total Size      = 0x0054a1c0 = 5546432 Bytes
## Start Addr      = 0x40200000
```

`crc32 ${loadaddr} ${filesize}` then prints the CRC-32 of what arrived. On
Linux, `crc32 kernel.itb` (from the `libarchive-zip-perl` package) prints the
file's, or with Python:

```sh
python3 -c "import zlib,sys;print(hex(zlib.crc32(open(sys.argv[1],'rb').read())))" kernel.itb
```

For XMODEM, use `loadx ${loadaddr}` and `upload xmodem-1k`.

### Starting the receiver

The order doesn't matter: start the receiver and then `upload`, or `upload`
first and then the receiver. Until the receiver asks, the progress row says
`waiting for the receiver (C or NAK)` and the link works as usual, so you can
still type `loady`.

A receiver asks again every few seconds: U-Boot about every second, `rx` and
`rb` every ten. An `upload` typed after its first ask starts at the next one.
The `C` it sent first shows on the screen, as the upload didn't exist yet.

With no answer for 60 seconds, the upload stops with
`no NAK or C from the receiver: is it waiting (loady, loadx, rx)?`.

### While it runs

Once the receiver has asked, the link belongs to the upload until it ends: a
stray byte would break a packet, and some bytes even abort the receiver.

- What you type isn't sent. A notification says so once: `A file transfer is
  running: cancel stops it`. A line typed in line mode stays in its input, so
  you can send it afterwards.
- `send`, `send-hex` and `mash` are refused.
- The receiver's answers aren't shown, and maxicom doesn't answer the device's
  terminal queries.
- Everything else works: scrolling, copy mode, search, popups.

What the device prints before the receiver starts, and after the upload ends,
like U-Boot's `## Total Size` line, shows as usual.

The [session backup](capture.md#session-backup) and a
[full recording](capture.md#full-recordings) keep every byte received, the
receiver's answers too. A [text recording](capture.md#text-recordings) keeps
neither the packets nor the answers: they aren't text.

## The progress row

While a file is sent, a row at the bottom of the view shows its progress:

```text
 ⇡ kernel.itb YMODEM [█████████░░░░░░░░░░░░░░░░░░░░░]  32%  1.7M/5.3M  11K/s  ETA 5:35  2 retries   ✕
```

| Part | Shows |
| --- | --- |
| `⇡ kernel.itb` | The file |
| `YMODEM` | The protocol, for an upload |
| The gauge and the percent | How much is sent. For an upload, what the receiver acknowledged. |
| `1.7M/5.3M` | Bytes sent and the file's size |
| `11K/s`, `ETA 5:35` | The rate so far and the time left at that rate. For an upload, counted from the receiver's first ask. |
| `2 retries` | Packets sent again, when there were some |
| `✕` | A click stops the transfer |

On a narrow screen the row drops its parts, the retries first, then the time
left, the rate, the sizes and the protocol, the gauge last.

The row sits over the status bar, which doesn't move. A search bar opened
during a transfer goes over the row.

`ui.transfer_row = false` in `config.toml` hides it. The status bar can show
the transfer instead, with `{send_file}`, `{send_percent}`, `{send_rate}` and
`{send_eta}`, see [Status bar](status-bar.md#variables).

## Stopping a transfer

`cancel`, or a click on the row's `✕`, stops it. An upload also tells the
receiver to stop. A detach, a lost link or a replay stops it too.

## When an upload fails

The notification says why: `Stopped sending <file> at <n>%: <why>`.

| Why | Means |
| --- | --- |
| `no NAK or C from the receiver: is it waiting (loady, loadx, rx)?` | Nothing asked for the file within 60 seconds |
| `the receiver cancelled` | The receiver gave up: Ctrl-C in U-Boot, a full disk, a timeout on its side |
| `too many retries` | A packet was refused or unanswered 10 times: a noisy line, a wrong baud rate, flow control |
| `the receiver stopped answering` | YMODEM: no next ask after the header or the end of the file |
| `the write failed`, `the link closed` | The link went away |

A receiver that keeps refusing packets often means a line that loses bytes:
try a lower baud rate, or flow control if both sides have it.

## ZMODEM and Kermit

maxicom doesn't do ZMODEM or Kermit. To use them, hand the port to another
program for the time of the transfer:

1. Detach the link with `Ctrl-a D`, see
   [Detach and attach](links.md#detach-and-attach).
2. Run the transfer in another terminal, for example
   `sz kernel.itb < /dev/ttyUSB0 > /dev/ttyUSB0` once `rz` runs on the device.
3. `Ctrl-a D` again opens the port, the screen as you left it.
