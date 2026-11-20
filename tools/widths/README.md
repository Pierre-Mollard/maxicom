# Unsure widths

`src/glyphs/unsure_width.rs` lists the chars a host terminal may draw wider or
narrower than ratatui counts them (MAX-271). When the two disagree, the rest of
the row lands one or two columns off on the real screen, and pieces of earlier
lines stay there after scrolling. maxicom draws these chars as a marker instead
(unless `draw_unsure_width` is on). This tool writes that table. It runs on its
own, not in the build, and the app has no code that depends on the OS or the
terminal: the table is the same everywhere.

## Usage

From the repository's root (needs curl and the network for the first step):

```bash
tools/widths/fetch.sh    # Unicode data files into target/ucd
cargo run --release --manifest-path tools/widths/Cargo.toml --target-dir target/widths
```

The second command writes `src/glyphs/unsure_width.rs`, already formatted as
`cargo fmt` wants it. Never edit that file by hand.

To see what the table holds, `--list` prints each assigned char with its name
and the rule that caught it (zero-width ones on a dotted circle `◌`), then how
many unassigned code points are in it. It doesn't write the table:

```bash
cargo run --release --manifest-path tools/widths/Cargo.toml --target-dir target/widths -- --list | less
```

## When to run it again

| Change | What to do |
| --- | --- |
| `unicode-width` updated in maxicom's `Cargo.lock` (ratatui and vt100 use it) | Change the pin in `tools/widths/Cargo.toml` and `UW` in `src/main.rs` |
| A new Unicode version | Change `NEW`, in `src/main.rs` and `fetch.sh` |
| Hosts commonly know a newer Unicode version | Change `OLD`, in `src/main.rs` and `fetch.sh` |
| A host draws a char with another width than its own data says | Add it to `QUIRKS` and a test |

Then run `fetch.sh` and the generator, and `cargo test unsure_width`.

## What counts as unsure

The width a host gives a char is computed the way glibc's `wcwidth` does it,
which tmux follows:

| Width | Chars |
| --- | --- |
| 0 | Combining marks, format chars (except the soft hyphen and the prepended concatenation marks), Hangul medial and final jamo |
| 2 | East Asian Wide and Fullwidth |
| 1 | The rest, and any code point the host's data doesn't know (tmux draws an unknown char in one column, ideographs included) |

A char is unsure when:

* its width under `NEW` (Unicode 18) data differs from what `unicode-width`
  gives it
* its width under `OLD` (Unicode 15.1, glibc 2.39's, where tmux was measured)
  data differs from `NEW`'s: a char added since then takes 1 column on a host
  that doesn't know it yet
* it's in `QUIRKS`: chars a known host draws with another width than its own
  data says (glibc makes U+3248..U+324F wide)

A host older than `OLD` may still drift on the chars added between its version
and `OLD`: they're rare, and the redraw key repaints the screen. Choosing an
older `OLD` hides many more chars that current hosts draw fine: with Unicode 9,
the table also took the 23,000 CJK ideographs added from Unicode 10 to 15.

The tests in `src/tests/glyphs/unsure_width.rs` hold one char of each disagreement
measured in tmux 3.8, and common text that must stay out of the table.

## Measuring a host

How the tmux numbers were found, to check another terminal: inside it, print
each char after a carriage return, ask the cursor position with `CSI 6n`, and
the column it answers is the width the terminal gave the char. A char whose
measured width differs from `unicode-width`'s must be in the table.
