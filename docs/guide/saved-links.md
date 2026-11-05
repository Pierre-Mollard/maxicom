# Saved links

A saved link gives a name to a device and its line settings, so you can open
it with `maxicom esp32` instead of typing the port and the baud rate. It can
point to a serial port or to a [TCP link](tcp-links.md).

Saved links show in the Saved links tab of the [directory](directory.md).

## Where they are

Saved links live in `links.toml` in the config folder. The file is created
on the first save. `links_path` in `config.toml` moves it elsewhere.

```toml
[[link]]
name = "esp32"
hosts = ["laptop"]           # the machines it's for; none: every one
address = "/dev/ttyUSB0"
baud = 115200                # the line fields left out are [link]'s
bits = 8
parity = "none"
stop_bits = 1
flow = "none"

[[link]]
name = "router console"
address = "tcp://192.168.1.50:4000"  # no line: the bridge sets it
```

Line settings you leave out take the values of `[link]` in `config.toml`.

## Editing in the app

You rarely need to edit the file by hand. In the directory:

| Key | Opens the form |
|---|---|
| `s` on the Saved links tab | Filled from the link in use |
| `s` on the Serial ports tab | Filled from the selected port |
| `n` | Empty, for a new link |
| `e` | On the selected link, to edit it |

In the form, move between the fields and change the bits, parity, stop bits
and flow with the [popup keys](popups.md). Enter saves, and Esc goes back to
the directory. A TCP address greys out the line settings, since its bridge
sets them.

`x` or `Del` in the directory deletes the selected link, after a confirmation.

From the [command line](command-line.md#saved-links), `link-save [name]` and
`link-delete <name>` do the same.

### How a save writes the file

A save reads `links.toml` again first, in case another maxicom changed it,
then writes it whole. Comments in the file are lost.

If the link you edit or delete was changed in the file in the meantime, the
change is refused.

`Ctrl-a r` reloads the file after you edit it by hand.

## One file for several machines

`hosts` lets one `links.toml`, on a network share for example, serve several
machines. Each machine sees the links listing its hostname (in any case),
plus the links without `hosts`.

When saved from the app:

- a serial port is for this machine only, since a path belongs to one
  machine;
- a TCP link is for every machine.

Two links can share a name as long as no machine sees both. So
`maxicom esp32` can open `/dev/ttyUSB0` on one machine and `COM3` on
another.

## How a saved link shows

maxicom calls a saved link by its name:

- in the status bar (`{port}`);
- in the notices and the link's popups, with the address after it:
  `esp32 (/dev/ttyUSB0)`.

## When the file doesn't load

If `links.toml` has an error (a bad address, a name used twice on one
machine, a misspelt field), the Saved links tab says why. The rest of the
app works normally.

A file maxicom can't read is never written over.
