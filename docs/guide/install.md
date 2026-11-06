# Install

maxicom is a single binary and needs no system library. You can download a
prebuilt binary or build it yourself.

## Prebuilt binaries

Each release on the
[releases page](https://github.com/Pierre-Mollard/maxicom/releases) comes
with an archive per platform. Each archive holds the binary, the README and
the licenses.

### Which archive to pick

The archive name says which processor and which system it is built for.

| Archive | Processor | Systems |
| --- | --- | --- |
| `maxicom-x86_64-unknown-linux-musl.tar.gz` | Intel or AMD, 64-bit | Any Linux distribution with kernel 3.2 or newer |
| `maxicom-aarch64-unknown-linux-musl.tar.gz` | ARM, 64-bit | Any Linux distribution with kernel 3.2 or newer: Raspberry Pi 3, 4 and 5 with a 64-bit OS, ARM servers, Jetson boards |
| `maxicom-x86_64-apple-darwin.tar.gz` | Intel | macOS on Intel Macs |
| `maxicom-aarch64-apple-darwin.tar.gz` | Apple Silicon (M1 and later) | macOS 11 and newer |
| `maxicom-x86_64-pc-windows-msvc.zip` | Intel or AMD, 64-bit | Windows 10 and 11; also Windows 11 on ARM, through its x64 emulation |

On Linux, `uname -m` prints the processor: `x86_64` or `aarch64`. If it
prints `armv7l` or `armv6l`, the system is 32-bit (for example the 32-bit
Raspberry Pi OS). There is no binary for it, so
[build from source](#build-from-source).

The Linux binaries are built with musl and linked statically. They don't
depend on the system's C library (glibc), so the same binary runs on new
distributions, old ones like RHEL 7 and 8, and Alpine.

### glibc builds

Some releases also come with Linux binaries linked against glibc, for testing
on real targets. They are rare: most releases won't have them. Each one is
built against an old glibc version and runs on that version and every newer
one.

| Archive | glibc | Systems |
| --- | --- | --- |
| `maxicom-<processor>-unknown-linux-gnu-glibc2.17.tar.gz` | 2.17 or newer | RHEL and CentOS 7, Debian 8, Ubuntu 14.04, Amazon Linux 2, SLES 12, and everything newer |
| `maxicom-<processor>-unknown-linux-gnu-glibc2.28.tar.gz` | 2.28 or newer | RHEL, Rocky and Alma 8, Debian 10, Ubuntu 20.04, Fedora 29, and everything newer |

`<processor>` is `x86_64` or `aarch64`, as in the table above. `ldd
--version` prints the system's glibc version on its first line.

The musl binary is the one to use. The glibc build differs in one way that
matters to maxicom: it resolves host names the way the rest of the system
does, through `/etc/nsswitch.conf`. With it, a [TCP link](tcp-links.md) can
use a `.local` mDNS name (avahi) or a host known only to LDAP. The musl
binary only reads `/etc/hosts` and asks the DNS servers in
`/etc/resolv.conf`.

### Download

The archive names don't change between versions, so this link always points
to the newest release. On Linux, for example:

```bash
curl -L https://github.com/Pierre-Mollard/maxicom/releases/latest/download/maxicom-x86_64-unknown-linux-musl.tar.gz | tar xz
mkdir -p ~/.local/bin
mv maxicom-x86_64-unknown-linux-musl/maxicom ~/.local/bin/
```

`SHA256SUMS`, next to the archives, lets you check a download with
`sha256sum --check --ignore-missing SHA256SUMS`.

The binaries aren't signed. If macOS refuses to open maxicom because the
developer can't be verified, remove the quarantine flag the browser put on
it: `xattr -d com.apple.quarantine maxicom`. On Windows, SmartScreen may
warn the first time; choose **More info**, then **Run anyway**.

## Build from source

maxicom builds with Rust 1.85 or newer (edition 2024), which you can get
from [rustup](https://rustup.rs).

```bash
cargo install maxicom                                              # from crates.io
cargo install --git https://github.com/Pierre-Mollard/maxicom      # the latest code
```

From a clone, run `cargo build --release`. The binary is then
`target/release/maxicom`.

## Check it works

Run `maxicom --version`. `maxicom --help` lists the options.

## Linux

### Port names

| Name | Used for |
| --- | --- |
| `/dev/ttyUSB0` | USB-UART adapters: CP210x, CH340, FTDI |
| `/dev/ttyACM0` | Boards with native USB: Arduino, STM32, ESP32-S3 |
| `/dev/ttyS0` | On-board UARTs |

### Permissions

Serial ports belong to a group: `dialout` on Debian, Ubuntu and Fedora,
`uucp` on Arch. Join it once, then log out and back in:

```bash
sudo usermod -aG dialout "$USER"
```

Until you do, the port fails to open with a permission error. The
[directory](directory.md)'s test also points it out, naming the group that
owns the port: `no access: join dialout`.

## macOS

Ports are named `/dev/cu.usbserial-XXXX`, `/dev/cu.usbmodemXXXX` or
`/dev/cu.SLAB_USBtoUART`.

Use the `cu.` names. A `tty.` name waits for the carrier line, which most
boards don't drive, so it never opens.

You don't have to type the name: the directory (`Ctrl-a d`) lists the ports.

## Windows

Ports are named `COM3`, `COM4`... The device path form works too:
`maxicom \\.\COM10`.

Run maxicom in Windows Terminal, the default terminal on Windows 11. The
older console window isn't tested.

There is no group to join, but only one program can open a port at a time.
Close the Arduino IDE's serial monitor or PuTTY first.

## WSL2

WSL2 doesn't see the PC's USB ports by itself. You have three options:

- **Forward the adapter** with
  [usbipd-win](https://github.com/dorssel/usbipd-win): run
  `usbipd attach --wsl --busid <id>` in Windows, and the port shows up as
  `/dev/ttyUSB0` in WSL.
- **Run the Windows build** of maxicom instead.
- **Reach the board over TCP**, through a TCP-UART bridge in its firmware or
  ser2net on another machine. See [TCP links](tcp-links.md).

## Where its files are

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Config | `~/.config/maxicom/` | `~/Library/Application Support/maxicom/` | `%APPDATA%\maxicom\` |
| Session backups | `~/.cache/maxicom/sessions/` | `~/Library/Caches/maxicom/sessions/` | `%LOCALAPPDATA%\maxicom\sessions\` |

On Linux, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` move them.

- `maxicom --cache-dir` prints the backups folder, handy for
  `cd "$(maxicom --cache-dir)"`.
- The settings popup shows the config folder, and `C` copies its path.

The config files are written with their defaults on the first start. See
[Settings](settings.md).
