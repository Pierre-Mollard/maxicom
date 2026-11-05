# Contributing

## Development notes

- Build with cargo

```bash
cargo build
```

- Run with local bash redirect for testing interactive programs

```bash
./scripts/run_shell.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom-shell-test
```

- Run the bin utility "fake_device" for testing (very) dumb programs

```bash
./scripts/run_fake.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom
```

- Run a device over TCP (`tcp://` links): a shell on `127.0.0.1:4000`, one per
  connection, or a real serial port shared like ser2net

```bash
./scripts/run_tcp.sh                            # zsh on port 4000
./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200   # bridge a serial port
```

```bash
cargo run --bin maxicom -- --device tcp://127.0.0.1:4000
```

- Profile (flame chart) with sample

```bash
cargo build --bin maxicom --profile profiling
```

```bash
samply record ./target/profiling/maxicom --baudrate 115200 --device /tmp/maxicom-shell-test
```

- Redo the guide's screenshots (`docs/guide/images/`) after a UI change, see
  [tools/screenshots](tools/screenshots/README.md) for what it needs

```bash
python3 -I tools/screenshots/shots.py
```
