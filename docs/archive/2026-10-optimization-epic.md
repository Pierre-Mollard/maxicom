# maxicom: optimization epic (archive)

<!--toc:start-->
- [maxicom: optimization epic (archive)](#maxicom-optimization-epic-archive)
  - [Recap](#recap)
    - [Scope](#scope)
    - [What changed, by area](#what-changed-by-area)
    - [Bugs fixed on the way](#bugs-fixed-on-the-way)
    - [Dropped, and why](#dropped-and-why)
    - [Lessons](#lessons)
  - [Workflow used](#workflow-used)
  - [Phase 0: safety net](#phase-0-safety-net)
  - [Phase 1: quick wins](#phase-1-quick-wins)
  - [Phase 2: main loop](#phase-2-main-loop)
  - [Phase 3: small isolated fixes](#phase-3-small-isolated-fixes)
  - [Phase 4: medium, one module each](#phase-4-medium-one-module-each)
  - [Phase 5: vt100 upgrade, then terminal mode on top](#phase-5-vt100-upgrade-then-terminal-mode-on-top)
  - [Phase 6: data model and display mapping](#phase-6-data-model-and-display-mapping)
  - [Phase 7: line-mode render, lay out and draw only what is shown](#phase-7-line-mode-render-lay-out-and-draw-only-what-is-shown)
  - [After closing](#after-closing)
  - [Results](#results)
  - [Code Recap](#code-recap)
  - [Notes from me](#notes-from-me)
<!--toc:end-->

Performance, memory and bug review of maxicom, 2026-10-22 to 2026-10-24.
Jira epic [MAX-78](https://creepierre4.atlassian.net/browse/MAX-78) holds the
details of every item (where, problem, fix, measured result). This file is the
closed index plus a recap. It is kept as an archive and is not updated anymore.

## Recap

### Scope

- 35 items in 8 phases (0 to 7) plus one bug after closing: 34 done,
  2 dropped (#2B, #22). Three designs were also dropped inside done items
  after measuring or weighing the cost (see below).
- 14 commits, from `074a7ed` (the safety-net tests) to `c4d5ad0` (MAX-123).
- `src/`: 27 files changed, +5670 / -1032 lines, most of the additions are
  tests.
- Tests: 0 → 148.
- Dependencies: vt100 0.16 → 0.18, `tui-term` and `strip-ansi-escapes` dropped
  (own cell mapping and own sanitizer), `const-hex` dropped (own byte tables),
  `memchr` added (byte search).

### What changed, by area

- **Main loop.** Frames are drawn only when something changed, capped at
  60 fps. Serial data is drained every loop and input polling is the idle
  wait. Toasts expire on their own, pending input events are drained per
  frame. Idle CPU went from 2.2% to 0.2% of a core and terminal-mode replay
  from ~0.46 MB/s to ~100 MB/s.
- **Formatting.** Hex, octal and binary use static tables with no allocation
  per byte. Timestamp width is cached. Status bar templates are compiled once.
  Loggers are buffered with a timed flush.
- **Line mode render.** Only the entries around the view are laid out, the
  paragraph gets only the visible rows (no more `u16` row cap), word wrap
  borrows the text instead of copying it, and per-entry row heights give a
  scrollbar, `%` and `SCROLL +n` counted in rows. Byte modes count their rows
  by arithmetic. A render cache was measured and dropped: after these changes
  formatting is cheap and the cost was in laying out rows nobody sees.
- **Scrolling.** Line mode scrolls by rows through entries taller than the
  view: the position is (entry, rows hidden below), resolved in the render.
- **Data model.** `LogEntry` keeps the received bytes when they differ from the
  text, so the raw panel, raw search, raw yank and the byte display modes show
  what was on the wire (Latin1, CP437, invalid UTF-8), not re-encoded UTF-8.
- **Control bytes.** One sanitizer (`walk_visible_line`) is shared by display,
  render, search and the text-to-screen map: tabs go to 8-column stops, a
  mid-line CR keeps the text after the last one, broken escape sequences are
  cut cleanly. Clean lines take a `Cow` fast path.
- **Search.** Line search uses a lazy map, ASCII case-insensitive matching and
  `memmem`. Terminal search text is built once per search and rebuilt on
  RX, resize or clear.
- **Terminal mode.** vt100 0.18 with an own vt100 → ratatui cell mapping.
  Resize keeps the bottom rows and the prompt. Timestamps live in one per-row
  list aligned with the scrollback. Keys follow the application cursor mode
  (DECCKM). Line refs follow their rows once the scrollback is full.
- **Input.** Bracketed paste works in every input context (one serial write
  per paste). Kill-to-end/start and vi text objects use byte indexes.

### Bugs fixed on the way

- Crash on an empty byte search needle (`0x`).
- Line refs pointing at the wrong line once the history or scrollback is full.
- Raw search column landing on a separator, not the byte.
- Word wrap not splitting long words, continuation rows losing characters
  under the indent.
- Copy mode reading terminal rows without their tab and cursor-move gaps.
- Line mode unable to scroll through entries taller than half the view.
- Raw panel, raw search and raw yank showing re-encoded bytes.
- Copy mode from a search losing its cursor row with tall entries (MAX-123).

### Dropped, and why

- **#2B reader threads:** the single-threaded loop already drains ~100 MB/s,
  threads would add complexity for no visible gain.
- **#22 `panic = "abort"` and `mimalloc`:** measured, no gain worth the change.
- **Line-mode render cache (#5):** measured first (#5.0). The cost was laying
  out unseen entries, not formatting, so #5a to #5d fixed that instead.
- **Arithmetic wrap for drawing:** 35-60 µs per frame left after #5c, not worth
  a second wrap implementation.
- **vt100 resize on `Event::Resize`:** rejected in #35, the stored viewport
  covers it.

### Lessons

- Lock today's behavior in tests (`// FIXME: current behavior, see #N`) before
  touching it: several real bugs showed up that way (#5a scroll to top,
  #36b tall first entry, #5d rows counted per frame).
- Measure before building: the render cache looked necessary and was not.
- Compare against a `git archive HEAD` build to confirm a fix in the real app,
  not only in tests.

## Workflow used

- One item (or one sub-step) per review round: implement, run fmt + clippy +
  tests, update the ticket and this index, then stop until the change is
  reviewed (staged or committed).
- Plan numbers (`#N`) stay: the code and tests refer to them, and ticket
  summaries start with `[#N]`.
- Known bugs are locked in tests as `// FIXME: current behavior, see #N`. When
  #N is fixed, its test is updated to the correct expectation.
- Jira labels: `optimization` on every ticket, `phase-0` … `phase-7`,
  `dropped`.

Status: `[x]` done · `[-]` dropped

## Phase 0: safety net

- [x] #23 MAX-79  Unit tests on mapping, wrap, format, search, selection

## Phase 1: quick wins

- [x] #27 MAX-80  Byte search crash on empty needle ("0x")
- [x] #7  MAX-81  Zero-alloc hex/octal/binary formatting
- [x] #9  MAX-82  Cached timestamp width
- [x] #15 MAX-83  MaxicomCursor borrows its glyph
- [x] #16 MAX-84  Buffered loggers, timed flush
- [x] #17 MAX-85  RX ingest without extra copies
- [x] #11 MAX-86  Terminal rows walked once, not O(rows²)
- [x] #4  MAX-87  Toasts expire without input
- [x] #3  MAX-88  Drain pending input events per frame

## Phase 2: main loop

- [x] #1  MAX-89  Dirty-flag rendering, 60 fps cap
- [x] #2A MAX-90  Serial drain per loop, input poll as idle wait
- [-] #2B MAX-91  Reader threads + one channel: dropped, not worth threads

## Phase 3: small isolated fixes

- [x] #19 MAX-92  Line refs follow their line when history is full
- [x] #24 MAX-93  Raw search col lands on the byte, not the separator
- [x] #26 MAX-94  Word wrap splits long words
- [x] #30 MAX-95  Continuation rows no longer lose chars under the indent
- [x] #21 MAX-96  Deques grow on demand, `send_history_size` setting
- [x] #14 MAX-97  Scrollback memory documented, default 2500
- [-] #22 MAX-98  `panic="abort"` / `mimalloc` measured, dropped

## Phase 4: medium, one module each

- [x] #18 MAX-99  Line search: lazy map, ASCII case-insensitive, memmem
- [x] #10 MAX-100 Precompiled status bar templates
  - follow-up: skip the 1 s forced redraw (#1) when no template uses `{time}`
- [x] #28 MAX-101 Bracketed paste in every input context
- [x] #32 MAX-116 Kill-to-end/start and vi text objects: char index used as
  bytes

## Phase 5: vt100 upgrade, then terminal mode on top

- [x] #29 MAX-102 vt100 0.16 → 0.18, drop tui-term
  - [x] #29a Own vt100 → ratatui cell mapping, tui-term dropped (vt100 0.16)
  - [x] #29b vt100 0.18: breaking changes, `scrollback_len` replaces the HACK
- [x] #31 MAX-115 Terminal keys follow application cursor mode (DECCKM)
- [x] #33 MAX-118 Copy mode reads rows with their tab and cursor-move gaps
- [x] #13 MAX-103 Terminal timestamps: one per-row list aligned with scrollback
  - [x] #13a Row list, eviction counter, one update per chunk (no byte loop)
  - [x] #13b Resize: evictions from a full scrollback, blank rows at the top
- [x] #34 MAX-119 Terminal line refs follow their rows once scrollback is full
- [x] #12 MAX-104 Terminal search text built once per search, rebuilt on
  RX/resize/clear

## Phase 6: data model and display mapping

- [x] #35 MAX-114 `App.viewport` stored, setting toggles re-lay out with the
  real size
  - resize vt100 on `Event::Resize` instead: considered and rejected (see
    ticket)
- [x] #25 MAX-105 Control bytes: one sanitizer for display, render and map
  - decided: tab = spaces to the next 8-col stop, mid-line CR = keep text
    after the last one
  - [x] #25a Lock today's disagreements (tab, BEL, BS, DEL, mid-line CR,
    DCS/APC payloads)
  - [x] #25b Shared sanitizer used by search/copy text, map and render
    - review: `ESC ( B` intermediates, ESC aborts a cut sequence, CR inside a
      title
  - [x] #25c `Cow` fast path for clean lines (#18 follow-up: text search cost)
- [x] #36 MAX-120 Line mode scrolls by rows through entries taller than the
  view
  - fix: position = (entry, rows hidden below), resolved in render, one row
    per step
  - [x] #36a Render harness test locking the skipped rows (Binary, 4 tall
    lines)
  - [x] #36b Row scrolling through tall entries
- [x] #20 MAX-106 Store wire bytes in `LogEntry` (only when they differ from
  the text)
  - [x] #20a Lock raw panel / raw search / raw yank for Latin1, CP437,
    invalid UTF-8
  - [x] #20b Store the wire bytes, text to wire offsets (no consumer change)
  - [x] #20c Raw panel, raw search, raw yank, focus sync use the wire bytes
    - byte display modes (Hex, Octal, Binary) of the main view too

## Phase 7: line-mode render, lay out and draw only what is shown

- render cache dropped after measuring (#5.0): formatting is cheap since #25c,
  the cost is laying out ~2 view heights of entries (and dropping them) when
  only one view is shown
- [x] #5  MAX-107 Lay out and draw only what is shown
  - [x] #5.0 MAX-108 Benchmark and profile (Pretty 0.5 ms, Hex/Binary 2.2 ms
    at 200x50)
  - [x] #5a  MAX-109 Lay out only the entries needed around the view
  - [x] #5b  MAX-110 Hand the paragraph only the visible rows
    - no speed gain (`Paragraph` skips scrolled rows), removes its `u16` row
      cap
  - [x] #5c  MAX-111 Zero-copy word wrap: spans moved or sliced, not copied
    (#6)
  - [x] #5d  MAX-122 Per-entry height cache: scrollbar, `Bot`/`%`,
    `SCROLL +n` in rows
    - the #36 follow-up; byte modes counted by arithmetic, without laying out
    - not done: an arithmetic wrap for drawing (35-60 µs/frame left after 5c,
      not worth a second wrap)

## After closing

- [x] #37 MAX-123 Copy mode from a search lost its cursor row with tall
  entries (byte modes)

## Results

| Measure | Before | After | Item |
| --- | --- | --- | --- |
| Idle CPU, % of a core | 2.2 | 0.2 | #1 |
| Terminal replay, 40 MB | ~0.46 MB/s | ~100 MB/s | #2A |
| Byte formatting, allocs per byte per frame | 1 | 0 | #7 |
| Default scrollback memory, 80 cols | 12.8 MB | 6.4 MB | #14 |
| Raw byte search, 100k lines, per key | 46.6 ms | 3.0 ms | #18 |
| Text search, 100k lines, per key | 41.5 ms | 35.7 ms | #18 |
| Text search, 100k lines, full scan | 33.2 ms | 3.3 ms | #25b, #25c |
| Status bar, 3 default templates | 4.69 µs | 0.39 µs | #10 |
| Paste, serial writes | one per char | one per paste | #28 |
| Terminal ingest bookkeeping, % of RX | 23 | ~1 | #13a |
| Terminal ingest throughput | 76 MB/s | ~88 MB/s | #13a |
| Pretty render, 50 lines | 3.9 µs | 2.7 µs | #25c |
| Line frame 200x50, Binary | 2.35 ms | 0.56 ms | #5a |
| Line frame 200x50, Hex | 2.22 ms | 0.42 ms | #5a, #5c |
| Line frame 200x50, Pretty | 0.50 ms | 0.36 ms | #5a |
| Flood CPU 200x50, Binary, % | 11.8 | 4.8 | #5a |
| Flood CPU 200x50, Hex, % | 12.8 | 5.2 | #5a, #5c |
| Flood CPU 200x50, Pretty, % | 4.2 | 3.4 | #5a |

Also: terminal resize keeps the bottom rows and the prompt, top rows go to the
scrollback (#29)

## Code Recap

Before the epic: 13,202 lines with 11,600 of code, 219 comment lines and no tests.
After the epic:

- about +3,600 lines of code, most of them tests
- comment lines going from 219 to 934

## Notes from me

I made the initial architecture (before this epic),
Then expanded it with the help of some chatbots through Perplexity.

So then Clause could review, optimize and fix the whole made project.
Claude Opus 5.5 did all the analysis/correction, I reviewed them step by step (#Xy).
I had to move some code around and redo some functions to match my architecture.
A few of the bugs found were from me during functional testing, I had to steer
Claude a lot during some changes that seemed too big or ambitious.

All the tests are passed and made by Claude; most of them were not deeply reviewed.

Overall, this was a great experience,
From now on I will let Claude directly implement new features (with reviews of course).
