# File transfers

maxicom moves files in three ways:

| Command | Moves | For |
| --- | --- | --- |
| `send-file <path> [line-delay-ms]` | A file to the device, its bytes as they are, paced to the baud rate | A script pasted into a shell, a text file to a console |
| `upload <protocol> <path>` | A file to the device, in XMODEM or YMODEM packets, each one checked and sent again if damaged | A bootloader or a program waiting for a file: U-Boot's `loady` and `loadx`, `rx`, `rb`, many MCU bootloaders |
| `download <protocol> [path]` | Files from the device, in XMODEM or YMODEM packets | `sb` or `sx` on a Linux board, a bootloader that dumps memory or logs |

They run from the [command line](command-line.md) (`Ctrl-a :`). The path
completes from the disk, `~` is your home folder, and a path with spaces goes
in quotes. One transfer runs at a time.

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

## Downloading

`download` receives what the device sends with XMODEM or YMODEM, for example
from lrzsz on a Linux board:

```text
$ sb logs.tar.gz
```

```text
download ymodem
```

### Where the files go

| Command | Writes |
| --- | --- |
| `download ymodem` | The files under their own names, in `[transfers] dir` |
| `download ymodem ~/dumps/` | The files under their own names, in `~/dumps/` |
| `download xmodem` | One file, named by `[transfers] xmodem_name`, in `[transfers] dir` |
| `download xmodem ~/dumps/` | The same, in `~/dumps/` |
| `download xmodem ~/dumps/flash.bin` | One file, `~/dumps/flash.bin` |

A path is a folder when it exists as one or ends with `/`; a missing folder is
made. YMODEM refuses a path that isn't a folder, since its files come with
their names. `[transfers] dir` is empty by default: the folder maxicom was
started in. See [Settings](settings.md#transfers).

### YMODEM or XMODEM

YMODEM sends each file's name, size and date: the file comes out exactly as it
was, dated as on the device, and one `download` takes a whole batch
(`sb a b c`). A name keeps its last part only, so a sender can't choose the
folder: `../../.bashrc` is written as `.bashrc`.

XMODEM sends neither a name nor a size: the file keeps the padding of its last
packet, up to 1023 `0x1A` bytes, and the progress row shows the bytes received
without a percent. Use it only when the device has nothing else.

### Existing files and partial files

A file is written as `<name>.part`, then renamed when complete. A name already
taken gets a number instead of overwriting anything: `logs.tar.gz.1`,
`logs.tar.gz.2`.

A download that stops in the middle of a file keeps its `.part`, and the
notification says where: `Stopped receiving logs.tar.gz, kept as
/home/me/logs.tar.gz.part: the sender cancelled`, or after your `cancel`,
`Cancelled: logs.tar.gz, kept as /home/me/logs.tar.gz.part`.

### Starting the sender

Start the sender on the device first, then `download`. Until the first packet
comes, maxicom asks for the file with a `C` every 3 seconds, and those go to
whatever reads them: typed after `download`, a shell prompt would get them. With
no packet for 60 seconds, the download stops with
`no packet from the sender: is it sending (sb, sx)?`.

While it runs, the link is the download's, as for an upload, see
[While it runs](#while-it-runs). The device's text before the first packet,
like the shell echoing `sb logs.tar.gz`, and after the end shows as usual.

Each file that comes in is said by name in the transfer's notification,
`Received logs.tar.gz (5546432 bytes)`, and the end gives the folder once:
`Received logs.tar.gz (5546432 bytes) into /home/me`, or for a batch
`Received 3 files (6012345 bytes) into /home/me`.

## The progress row

While a file goes through, a row at the bottom of the view shows its progress:

```text
 ⇡ kernel.itb YMODEM [█████████░░░░░░░░░░░░░░░░░░░░░]  32%  1.7M/5.3M  11K/s  ETA 5:35  2 retries   ✕
```

| Part | Shows |
| --- | --- |
| `⇡ kernel.itb` | The file, `⇡` sent to the device, `⇣` received from it |
| `YMODEM` | The protocol, for an upload or a download |
| The gauge and the percent | How much is through. For an upload, what the receiver acknowledged. An XMODEM download has no size, so neither. |
| `1.7M/5.3M` | Bytes through and the file's size |
| `11K/s`, `ETA 5:35` | The rate so far and the time left at that rate. For a protocol, counted from the other side's first ask or packet. |
| `2 retries` | Packets sent again, when there were some |
| `✕` | A click stops the transfer |

On a narrow screen the row drops its parts, the retries first, then the time
left, the rate, the sizes and the protocol, the gauge last.

The row sits over the status bar, which doesn't move. A search bar opened
during a transfer goes over the row.

`ui.transfer_row = false` in `config.toml` hides it. A notification then says
when the transfer starts and waits for the other side, which the row does
otherwise. The status bar can show the transfer too, with `{send_file}`,
`{send_percent}`, `{send_rate}` and `{send_eta}`, see
[Status bar](status-bar.md#variables).

## Stopping a transfer

`cancel`, or a click on the row's `✕`, stops it. An upload or a download also
tells the other side to stop. A detach, a lost link or a replay stops it too.

The notification tells your stop from a failure. A cancel says `Cancelled`, as
a warning: `Cancelled: fw.bin at 32%`, or for a download
`Cancelled: logs.tar.gz, kept as /home/me/logs.tar.gz.part`. A failure says
`Stopped`, as an error, with why: see below.

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

## When a download fails

The notification says why: `Stopped receiving <file>, kept as <part>: <why>`,
or `Stopped receiving: <why>` before the first file.

| Why | Means |
| --- | --- |
| `no packet from the sender: is it sending (sb, sx)?` | Nothing came within 60 seconds |
| `the sender cancelled` | The sender gave up: Ctrl-C on the device, a timeout on its side |
| `too many retries` | 10 damaged or missing packets in a row: a noisy line, a wrong baud rate |
| `a block came out of order` | The two sides lost each other; maxicom told the sender to stop |
| `the sender stopped answering` | YMODEM: nothing after asking for the data or the next file |
| `write error: <reason>` | The file couldn't be written: a full disk, a folder you can't write to |

## ZMODEM and Kermit

maxicom doesn't do ZMODEM or Kermit. To use them, hand the port to another
program for the time of the transfer:

1. Detach the link with `Ctrl-a D`, see
   [Detach and attach](links.md#detach-and-attach).
2. Run the transfer in another terminal, for example
   `sz kernel.itb < /dev/ttyUSB0 > /dev/ttyUSB0` once `rz` runs on the device,
   or `rz < /dev/ttyUSB0 > /dev/ttyUSB0` after `sz` on the device.
3. `Ctrl-a D` again opens the port, the screen as you left it.
