# maxicom: AI-assisted development, experience report (archive)

<!--toc:start-->
- [maxicom: AI-assisted development, experience report (archive)](#maxicom-ai-assisted-development-experience-report-archive)
  - [Scope](#scope)
  - [Metrics](#metrics)
  - [What the numbers do not show](#what-the-numbers-do-not-show)
  - [How the work was done](#how-the-work-was-done)
  - [How the numbers were measured](#how-the-numbers-were-measured)
<!--toc:end-->

A report for anyone interested in metrics on AI usage in a real solo project:
what about two weeks of work with an AI coding agent produced on maxicom.

## Scope

- From `db94c91` (2026-10-22), the parent of `074a7ed`: the safety-net tests
  that opened the optimization epic (see `2026-10-optimization-epic.md`).
- To `43666c2` (2026-11-03): MAX-170, unit tests moved to `src/tests/`.
- 84 commits: the optimization epic first, then mostly new features (session
  restore, command line, triggers, TCP link, saved links, probes, mouse and UI
  work, ...).

## Metrics

tokei "Code" lines (no comments, no blank lines), `src` only:

| `src` | Start (`db94c91`) | End (`43666c2`) | Change |
| --- | --- | --- | --- |
| Source files | 48 | 85 | +37 |
| Code (not tests) | 11,596 | 26,536 | +14,940 (×2.3) |
| Tests | 0 | 16,404 | all new |
| Total code | 11,596 | 42,940 | +31,344 (×3.7) |

Churn, the sum of every commit's diff (git numstat, raw lines, the MAX-170 test
move left out): +50,656 / −12,173, about 62,800 lines touched. It is far above
the net growth because a lot of code was written, then rewritten.

## What the numbers do not show

The code growth is not "work that would have taken me N times longer". Most of
these features would not exist at all without the agent:

- some for their complexity, too much effort for a personal tool
- some because I did not know the domain well (terminal queries and escape
  sequences, lock file conventions of other programs, etc.)
- some because they are not interesting to write, and I would never have done
  them myself
- most importantly, knowing an agent was there meant complexity was no longer
  a limit and boosted creativity (v0.1.0 should have shipped like
  12 features ago...)

## How the work was done

- **I reviewed every generated line of code.** The review took far longer than
  the generation. Changes came one item per review round, so each diff stayed
  small enough to understand.
- **Architecture still needed fixing up at times**, more often than during the
  optimization epic. Most likely because this period was mostly adding
  features: new code needs more design decisions than reworking existing code.
- **Steering took effort.** A lot of steering and reprompting the agent;
  understanding features and their integration became harder as the project
  grew.
- **The unit tests are not reviewed now.** They are there to catch breaking
  changes while the agent works, not as a reviewed specification. Behavior is
  checked by the code review and by manual tests in the app.

## How the numbers were measured

- Lines: `tokei --exclude tests` and `tokei` in `src`, and `tokei` on a
  `git archive db94c91 src` export for the start. Quote globs (`--exclude
  'tests/*'`): unquoted, zsh expands them and tokei counts the wrong files.
- Churn: `git log --numstat --format= db94c91..43666c2^ -- src`, summed.
- Before MAX-170 tests were inline `mod tests` blocks, so plain line counts of
  a file mixed code and tests; since then all tests are in `src/tests/`.
