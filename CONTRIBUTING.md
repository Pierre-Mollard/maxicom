# Contributing

Issues and pull requests are welcome. This page covers building maxicom,
checking a change the way CI does, trying it without hardware, and the rules
the code follows.

## Building

```bash
cargo build
cargo run -- /dev/ttyUSB0
```

The `profiling` profile is a release build with debug symbols, for the
profiler (see [Profiling](#profiling)).

## Checking a change

CI runs these on each pull request and each push to `main`: the format and
dependency checks once, clippy, the tests and a release build on Linux, macOS
and Windows. Run them before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check   # needs `cargo install cargo-deny`
```

`cargo deny check` (set up in `deny.toml`) checks the dependencies: their
licenses, where they come from, and the [RustSec](https://rustsec.org)
advisories, so a dependency with a known vulnerability, a yanked version or an
unmaintained crate fails it.

### Windows and macOS from Linux or WSL

The Windows build and its tests work from WSL, which runs the Windows test
binaries directly:

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64
cargo build --release --target x86_64-pc-windows-gnu
cargo test --target x86_64-pc-windows-gnu
```

macOS can only be checked, not linked or run, without a Mac:

```bash
rustup target add aarch64-apple-darwin
cargo clippy --all-targets --target aarch64-apple-darwin -- -D warnings
```

CI builds and tests the real macOS target.

## Tests

The tests of a module live in one mirror tree under `src/tests/`, not next to
the code. A module points to its file:

```rust
#[cfg(test)]
#[path = "tests/link.rs"]
mod tests;
```

`src/link.rs` has its tests in `src/tests/link.rs`, `src/app/device.rs` in
`src/tests/app/device.rs`. A new test goes in the module's mirror file.

## Trying it without hardware

These scripts make a fake device with `socat`, and say how to open it.

- A local shell behind a pty, to try interactive programs:

  ```bash
  ./scripts/run_shell.sh
  cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom-shell-test
  ```

- `fake_device`, a (very) dumb device. It's an example
  (`examples/fake_device.rs`, `cargo run --example fake_device`), so
  `cargo install` never installs it:

  ```bash
  ./scripts/run_fake.sh
  cargo run --bin maxicom --profile profiling -- --baudrate 115200 --device /tmp/maxicom
  ```

- A device over TCP (`tcp://` links): a shell on `127.0.0.1:4000`, one per
  connection, or a real serial port shared like ser2net:

  ```bash
  ./scripts/run_tcp.sh                            # zsh on port 4000
  ./scripts/run_tcp.sh 4000 /dev/ttyUSB0 115200   # bridge a serial port
  cargo run --bin maxicom -- --device tcp://127.0.0.1:4000
  ```

## Profiling

A flame chart with [samply](https://github.com/mstange/samply):

```bash
cargo build --bin maxicom --profile profiling
samply record ./target/profiling/maxicom --baudrate 115200 --device /tmp/maxicom-shell-test
```

## The documentation

The user guide is in `docs/guide/`, one Markdown file per chapter, in the
order of `docs/guide/SUMMARY.md`. A change that a user would notice goes in
its chapter, and in [CHANGELOG.md](CHANGELOG.md) under Unreleased.

- Read the guide as a website while editing it (needs
  [mdBook](https://rust-lang.github.io/mdBook/), `cargo install mdbook`); the
  page reloads on each save:

  ```bash
  mdbook serve docs --open
  ```

- Redo the guide's screenshots (`docs/guide/images/`) after a UI change, see
  [tools/screenshots](tools/screenshots/README.md) for what it needs:

  ```bash
  python3 -I tools/screenshots/shots.py
  ```

- `.markdownlint.yaml` and `.marksman.toml` set up the Markdown linters, as
  editors like Neovim and VS Code run them.

## Rules the code follows

- **maxicom never starts another program.** No `std::process::Command` for
  an opener, an editor or a shell. Give the user the data instead: copy a
  path, print it with a command-line option.
- **maxicom never sends anything to the device on its own.** Only what the
  user types, binds to a key, or arms as a trigger, and the answers to the
  queries the device itself sends. It can't know whether a shell, a
  bootloader or a binary upload is listening.
- **Visibility is private or `pub`.** No `pub(crate)` or `pub(super)`: a
  child module already sees its parent's private items.

## Commit messages

A conventional prefix and a short lowercase summary:

| Prefix | For |
| --- | --- |
| `feat:` | A new feature, or a change users see |
| `fix:` | A bug fix |
| `refactor:` | Code changed without changing behavior |
| `test:` | Tests only |
| `doc:` | Documentation only |
| `conf:` | Configuration, CI, packaging |

For example:

```text
fix: ctrl-c goes to the device, clears the line input first in line mode
```

## License

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in maxicom by you, as defined in the Apache-2.0
license, shall be dual licensed under the
[Apache License, Version 2.0](LICENSE-APACHE) and the
[MIT license](LICENSE-MIT), without any additional terms or conditions.
