# Characters drawn differently

maxicom shows what the device sends, but a few kinds of characters are drawn
differently on the screen than they were received. Each one is there to keep
the screen right: drawn as received, these characters would show nothing, or
shift the rest of their row and leave pieces of old text behind.

Only the screen changes. What maxicom keeps is always what was received: a
copy, a search, highlights and triggers, the
[raw side panel](raw-side-panel.md), captures and recordings all have the
real characters.

| Received | Drawn as | Setting |
| --- | --- | --- |
| [Bytes that aren't valid text](#bytes-that-arent-valid-text) | `�` | none, see [Text encoding](encoding-and-newlines.md#text-encoding) |
| [Invisible characters](#invisible-characters) | nothing | `draw_zero_width` in `[ui]` |
| [Characters of uncertain width](#characters-of-uncertain-width) | a dimmed `▯` | `draw_unsure_width` in `[ui]` |
| [A wide character cut by the edge](#a-wide-character-cut-by-the-edge) | a blank | none |

They apply to line mode and terminal mode alike. The settings are off by
default, and the next sections say why to keep them that way.

## Why the screen can go wrong

maxicom draws its screen inside your terminal (tmux, Ghostty, WezTerm, Windows
Terminal...), and both count how many columns each character takes. For
almost every character they agree. When they don't, your terminal puts the
character one column wider or narrower than maxicom planned, and everything
after it on the row moves by that much.

maxicom only redraws the parts of the screen that changed. After a scroll,
the moved row is mixed with the rows drawn over it, and pieces of earlier
lines stay on the screen until the whole window is drawn again.

`Ctrl-a R` (or `Ctrl-a Ctrl-l`, the `redraw` command) clears your terminal and
draws every cell again, whatever went wrong.

## What this doesn't fix

These rules keep maxicom's own drawing consistent, but maxicom isn't the only
program that places the characters you see. Several layers each count widths
their own way:

- **Your terminal** (WezTerm, Ghostty, Windows Terminal...), and on Windows
  the console layer between it and WSL, each with its own Unicode tables and
  version.
- **A multiplexer** like tmux or screen, which keeps its own copy of the
  window and re-sends it to your terminal.
- **The programs on the device**: a full-screen program there (a shell's line
  editor, `vim`, `top`) places its text with its own idea of the widths too.

maxicom can't see how these layers count, so it can't fix a disagreement
between them. A binary file printed with `cat` straight into your terminal,
without maxicom, can leave the same kind of mess for the same reason. The
characters above are the ones known to cause trouble with common setups; a
terminal with other tables may still shift a row now and then: `Ctrl-a R`
puts the screen back.

## Bytes that aren't valid text

Bytes that don't make a character in the `encoding` you use, like a binary
file printed with `cat`, show as `�`, one per broken character. There's no
setting: without the `�`, they'd show nothing at all. How they're counted is
in [Text encoding](encoding-and-newlines.md#bytes-that-arent-valid-text).

## Invisible characters

Some characters take no room on the screen: the soft hyphen (a place a word
may be cut), the zero-width space, the joiner that makes one emoji out of
several (👨 + joiner + 👩), and the selector that asks for an emoji's
colored form (❤ + selector). Terminals don't agree on how many columns they
take: one draws the soft hyphen in a column of its own, another makes a single
two-column emoji out of a joined one.

So maxicom doesn't send them to your terminal. An emoji with a selector shows
in its plain form followed by a blank column, a joined emoji as its parts side
by side, a soft hyphen not at all.

### Sending them anyway

If your terminal counts them as maxicom does, set `draw_zero_width = true` in
`[ui]` to see the emoji as sent. A row that shifts, or a character left behind
after a clear, means it doesn't: turn it back off.

## Characters of uncertain width

Each terminal counts widths with its own table, made from some version of
Unicode, and tables don't agree on every character. maxicom draws the
characters they disagree on as a dimmed `▯`, in the same columns: one for a
narrow character, `▯` and a blank for a wide one.

What gets the `▯`:

- Characters added in the newest Unicode versions, which most terminals don't
  know yet and draw one column wide: the latest Chinese ideographs, the Seal
  and Jurchen scripts.
- Code points with no character assigned yet. They come with binary data or a
  wrong baud rate, rarely on purpose.
- A few hundred older characters that terminals count differently: some
  Arabic and Syriac marks, about 25 vowel signs of Tamil, Kannada, Malayalam
  and Bengali, the trigrams `☰`, circled numbers on black squares.

Everything else is drawn as received: accents, Greek, Cyrillic, Hebrew,
Chinese, Japanese, Korean, emoji, box drawing, symbols.

### Why keep the marker

On a serial link, these characters nearly always come from bad data: a binary
file, an uninitialized buffer, a baud rate that doesn't match. Without the
marker, a single one can scramble the rows around it, and you can't trust
what you read until the window is redrawn. With it, the screen stays right,
and the `▯` tells you something unusual came in: the raw side panel shows
its bytes.

### Drawing them as received

If your device sends text in one of these scripts and your terminal is
recent enough to know it, set `draw_unsure_width = true` in `[ui]` to see the
characters themselves. A row that shifts, or pieces of earlier lines left on
the screen after a scroll, means your terminal counts them another way: turn
it back off.

## A wide character cut by the edge

A wide character (Chinese, Japanese, Korean, most emoji) takes two columns.
When terminal mode's view is panned so that it lands in the last column, only
its first half would fit: it's drawn as a blank instead of spilling over the
border. Pan one column further to see it.
