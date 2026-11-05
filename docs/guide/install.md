# Install

maxicom is a single binary and needs no system library. It builds with Rust
1.85 or newer (edition 2024), which you can get from
[rustup](https://rustup.rs).

```bash
cargo install maxicom                                              # from crates.io
cargo install --git https://github.com/Pierre-Mollard/maxicom      # the latest code
```

From a clone, run `cargo build --release`. The binary is then
`target/release/maxicom`.

<!-- prebuilt binaries: when the release job exists -->

To check it works, run `maxicom --version`. `maxicom --help` lists the
options.

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
