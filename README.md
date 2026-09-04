# MAXICOM

minicom but written in rust with my custom features.

## PLAN

1. Example @ 115200 baud on testing port
2. Add data bits, parity, stop bits and flow control
3. Add timestamps
4. Detect disconnects and error
5. Add small TUI with escape and ctrl-c

## TODO

- [x] Main loops and render / update system
- [o] Serial byte stream interpreter
- [o] Configuration
- [o] TUI
- [o] History
- [o] timestamps
- [ ] Integration with espressif idf.py monitor

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
