# MAXICOM

minicom but written in rust with my custom features.

## PLAN

1. Example @ 115200 baud on testing port
2. Add data bits, parity, stop bits and flow control

## NOTES

1. Build with cargo

```bash
cargo build
```

1. Run with cargo and arguments

```bash
cargo run -- --port /dev/ttyUSB0 --baud 115200 --test coucou alkgjl df - sflj
```

1. Run the bin utility "fake_device" for testing (target)

```bash
cargo run --bin fake_device
```
