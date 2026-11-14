# Performance tests

`perf.py` measures the CPU and the memory maxicom takes in scenarios that
matter: waiting, a busy device, a flood, a big scrollback, heavy content and
the features (search, highlights, raw panel, plot). It runs any version of
maxicom the same way, so a change can be compared with the releases before
it, and minicom for reference (MAX-233). Results can be saved in
`results/` and compared with later.

## Requirements

Linux (it reads `/proc`), with:

| Tool | For |
| --- | --- |
| Rust and cargo | Building the maxicom versions measured |
| Python 3.8+ | The script; standard library only |
| tmux | Running each program in a hidden window of a fixed size |
| minicom (optional) | `--minicom` |

## Usage

From the repository's root:

```bash
python3 -I tools/perf/perf.py                          # the working tree only
python3 -I tools/perf/perf.py --rev v0.1.1 --wip       # a release against the working tree
python3 -I tools/perf/perf.py --rev v0.1.0 --rev v0.1.1 --rev HEAD --minicom
python3 -I tools/perf/perf.py --only idle-line,steady-line --repeat 5
python3 -I tools/perf/perf.py --list                   # the scenarios
python3 -I tools/perf/perf.py --wip --against v0.1.1    # the working tree against saved results
python3 -I tools/perf/perf.py --rev v0.1.2 --save       # measure a release and keep its results
```

| Option | What it measures |
| --- | --- |
| `--rev <rev>` | maxicom at a git rev (a tag, a branch, a commit). Built once, then cached. |
| `--wip` | The working tree, uncommitted changes included, built in release. The default when nothing else is given. |
| `--bin <name>=<path>` | A maxicom binary you built yourself. |
| `--minicom` | The installed minicom, on the scenarios it has (terminal ones). |
| `--repeat <n>` | Runs of each scenario on each program, 3 by default. |
| `--only <a,b>` | Only these scenarios. |
| `--save` | Keep each program's results in `results/` (see below). |
| `--against <name>` | Add a saved result as a column, before the programs measured: `v0.1.1` reads `results/v0.1.1.json`; a path works too. Repeatable. |

The first program given is the baseline: the table gives the others' change
from it. Each value is the median of the runs, with the spread (the highest
minus the lowest) after `±`. A spread as big as the change means noise: run
again with more `--repeat`.

```text
## steady-line

| metric | v0.1.1 | wip |
| --- | --- | --- |
| cpu_% | 2.80 ±0.00 | 3.00 ±0.00 (+7%) |
| rss_kb | 11,008 ±0.00 | 11,648 ±0.00 (+6%) |
```

(One run each, so no spread: with `--repeat 1` a change of a few percent
says nothing yet.)

Each run also writes everything it measured, with the machine and the
versions, to `target/perf/runs/<date>.json`.

## Saved results

`--save` writes each program's results to `results/<name>.json`, to commit:

| Program | File |
| --- | --- |
| `--rev v0.1.1` | `v0.1.1.json`: the name `git describe` gives, so a commit after a tag is `v0.1.1-20-gbbf97e4.json` |
| `--minicom` | `minicom-2.9.json`, its version |
| `--bin name=path` | `name.json` |
| `--wip` | Never saved: the working tree can't be told apart later |

A file holds every run's value (not only the median), the date, the number of
runs, the version and the machine. Saving again into a file replaces the
scenarios just run and keeps the others, so `--only` runs add up.

`--against <name>` puts a saved file's results in the table, first, as the
baseline. A file measured on another CPU gets a warning: its numbers don't
compare with this machine's. The files in `results/` were measured on the
machine their `machine` field gives; on another machine, measure the versions
again (`--rev`) rather than compare with them.

Save a release once it is tagged, and the commits around a big change:

```bash
python3 -I tools/perf/perf.py --rev v0.1.2 --minicom --save
python3 -I tools/perf/perf.py --wip --against v0.1.2 --only flood-line,steady-line
```

## The scenarios

| Scenario | Programs | Measures |
| --- | --- | --- |
| `idle-line`, `idle-terminal` | maxicom (both), minicom (terminal) | Started, nothing received, 10 s: CPU %, RSS |
| `steady-line`, `steady-terminal` | maxicom (both), minicom (terminal) | Log lines at 115200 baud (11.5 KB/s) for 10 s: CPU %, RSS |
| `flood-line`, `flood-terminal` | maxicom (both), minicom (terminal) | 10 MB of log lines written as fast as the program reads them: MB/s until it's quiet again (drawing included), CPU seconds per MB, RSS and peak after |
| `flood-line-small`, `flood-terminal-small` | maxicom | The same with v0.1.0's history sizes (line mode 1000 lines, terminal mode 2500 rows): what the bigger histories cost |
| `flood-line-only` | maxicom | `flood-line` with `dual_mode_buffering = "active_only"`: without the terminal scrollback line mode fills too |
| `memory-line`, `memory-terminal` | maxicom (both), minicom (terminal) | 250,000 log lines, enough to fill both default histories: RSS at start, after, and the peak |
| `memory-line-only` | maxicom | The same in line mode with `dual_mode_buffering = "active_only"`: line mode's history alone |
| `ansi-line`, `ansi-terminal` | maxicom (both), minicom (terminal) | 8.7 MB of log lines with each word in its own color, as fast as read: MB/s, CPU s per MB |
| `long-line`, `long-terminal` | maxicom (both), minicom (terminal) | 4.8 MB of 600 char lines, each wrapping over several rows, as fast as read: MB/s, CPU s per MB |
| `hex-line` | maxicom | 2.4 MB of log lines in line mode's Hex display, as fast as read: MB/s, CPU s per MB |
| `redraw-terminal` | maxicom, minicom | A full-screen program (like top, 150x40) rewriting every row 10 times a second, for 10 s: CPU % |
| `resize-line`, `resize-line-plain` | maxicom | 10,000 lines of 160 chars in line mode, then the window narrowed and widened back 6 times: CPU ms per resize, with the default highlights and without any (an empty `highlights.toml`) |
| `search-line`, `search-terminal` | maxicom | 100,000 lines, then a search typed one key at a time (`ready n=4`, 9 keys): CPU ms per key, mean and highest |
| `highlights-line`, `highlights-terminal` | maxicom | `flood-*` with 300 highlight words, the 17 the log lines are made of among them: compare with `flood-*` |
| `raw-line`, `raw-terminal` | maxicom | `flood-*` with the raw side panel in Hex: compare with `flood-*` |
| `plot-line`, `plot-terminal` | maxicom | `flood-*` with the `key-value` plot shown, charting each line's `n=` and `t=`: compare with `flood-*` |

The metrics name their unit: `cpu_%` is the percentage of one core over the
measure, `rss_kb` the resident memory at its end and `peak_kb` the highest it
reached (`VmHWM`), in kB; `mb_per_s` and `cpu_s_per_mb` count megabytes of
1,000,000 bytes.

A resize lays the whole line mode history out again to count its rows, the
cost the `resize` scenarios measure; the `-plain` one shows what the
highlights add to it.

A feature scenario is its `flood-*` with one thing more: put the two tables
side by side to see what the feature costs. The highlights file uses the
`word = "Color"` form every version reads.

When a version doesn't have what a scenario measures, the scenario sees it on
the screen and the program gets `n/a` in the table, the reason printed during
the run: no hex rows for `hex-line` and `raw-*`, no `key-value` title for
`plot-*` (plots came after 0.1.1), no search bar with the query and its count
for `search-*`.

To find the commit behind a change, measure the commits around it:

```bash
python3 -I tools/perf/perf.py --rev 1ec844c --rev 53ae8ab --repeat 2 --only flood-line,highlights-line
```

The flood ends when the program has used no CPU for half a second: its
throughput counts the drawing of what it took in, not only the reading. A
program that hasn't read everything after 2 minutes fails the run.

By default maxicom feeds both modes whatever the one shown
(`dual_mode_buffering = "always_both"`), so `memory-line` holds the terminal's
scrollback too; `memory-line-only` leaves it out. The terminal's scrollback
takes about `history_size x cols x 32` bytes once full, and the grid is as
wide as the window in terminal mode: the 200 columns here make it big.

## What it does

For each scenario, on each program, as many times as `--repeat` says, the
programs in turns (so a machine getting slower spreads over all of them):

1. **A throwaway home.** `target/perf/homes/<program>/<scenario>/` becomes
   `HOME`, the `XDG_*` variables unset. A maxicom version writes its own
   default config files there, then the scenario sets the keys it needs
   (`mode = "line"`). Each version runs with its own defaults, as a user
   would get them. The session backup is on, as by default; the restore is
   off (`--no-restore`). minicom runs with `-o` (no modem init).
2. **A fake serial port.** A pty the program opens with `-D`. The script
   writes the scenario's data to it; what the program sends back is read and
   dropped.
3. **A hidden window.** The program runs in a tmux server of its own
   (`tmux -L maxicom-perf`), 200x50, never the one you work in. The script
   waits until it uses no CPU after starting, then runs the scenario.
4. **The measure.** CPU time (user + system) and memory (`VmRSS`, `VmHWM`)
   are read from `/proc/<pid>` before and after the scenario.

The versions given with `--rev` are exported with `git archive` into
`target/perf/build/<commit>/` and built there with `cargo build --release
--locked`. The repository's index and working tree are never touched. A
release build takes a few minutes the first time; the binary is then reused.

## Reading the numbers

- **Same machine only.** CPU numbers depend on the machine, its load and its
  power settings. Compare runs made on the same machine, with nothing heavy
  running beside. Under WSL2 the numbers are noisier than on Linux itself.
- **The terminal isn't counted.** Only the program's own process is
  measured. tmux draws the screen in its own process.
- **maxicom and minicom don't do the same work.** minicom passes the bytes to
  the terminal, which does the drawing (here tmux, not counted). maxicom has
  its own terminal emulator and draws its whole screen itself, besides its
  line mode history, its session backup and the rest. minicom is a
  reference for "the least a serial terminal costs", not a like for like.
- **The resolution.** CPU time comes in ticks of 10 ms: over 10 s, 0.1% of
  a core. Below that, idle numbers are 0 or 0.1.
