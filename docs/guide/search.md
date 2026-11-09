# Search

A search finds text or bytes in the history and moves the
[copy mode](copy-mode-and-mouse.md#copy-mode) cursor to them. It works in
both modes.

![A search for temp in line mode, its matches lit, the count in the search bar](images/search.png)

## Starting a search

| Key | Starts |
| --- | --- |
| `Ctrl-a /` | A search forward, down the history |
| `Ctrl-a ?` | A search backward, up the history |
| `/` in copy mode | A search forward, from the cursor |
| `?` in copy mode | A search backward, from the cursor |

Search in the right-click menu does the same as `Ctrl-a /`.

A search always runs in copy mode. `Ctrl-a /` and `Ctrl-a ?` enter copy mode
first, at the bottom of the view. From the live view, `Ctrl-a ?` finds the
newest match and `Ctrl-a /` the oldest.

The search bar opens right under the history: in its bottom border when that
border has nothing else in it, else on a row of its own, the history one row
shorter. In terminal mode the device's screen keeps its size: the view
scrolls by a row instead.

The bar shows the search as you'd type it in vi, `/` for a search forward
and `?` for a search backward, with the count and the keys at its right end:

```text
/error                                    (3/12)  n/N Match · Esc Exit
```

A byte search reads `[bytes]` before the `/`. Past 1000 matches, the count
reads `1000+`. While you type, the status bar reads `SRCH`; once Enter takes
you to the match, it's back to `COPY`.

## Typing the search

The bar opens empty, with your last search dimmed in it. Enter on the empty
bar searches it again; type to search something else.

Type the text: every match lights up as you type, and the current one
stands out. The copy mode cursor follows the current match, and the view
with it. The bar shows the current match's number and the count. Up to
1000 matches are found.

The first match is the one after the cursor for a search forward, and the
one before it for a search backward. When there is none that way, the
search goes on from the other end of the history, and a notification says
so when you press Enter. A match right on the cursor is skipped.

| Key | Does |
| --- | --- |
| Ctrl-n / Ctrl-p | Next / previous match, down / up the history |
| Enter | Close the bar, the cursor on the current match |
| Esc | Close the bar |
| Tab | Switch between a text search and a byte search |
| Up / Down | An older / newer search in place of what you typed, among those starting with it |

The last 100 searches you ran with Enter are kept, for both modes, until
maxicom quits.

Esc takes you back to where you started, and keeps your last search for `n`
and `N`: what you typed is dropped. A search started with `Ctrl-a /` or
`Ctrl-a ?` leaves copy mode for the live view. One started in copy mode puts
the cursor back where it was, your selection kept.

## Going through the matches

After Enter, you stay in copy mode. The matches stay lit and the bar stays,
showing the search, the current match and the keys to go through the
matches. They go away when you leave copy mode.

| Key | Does |
| --- | --- |
| `n` | Next match, in the search's direction |
| `N` | Next match the other way |
| `/` / `?` | A new search forward / backward from the cursor |
| `*` | A search for the word under the cursor |

`n` and `N` start from the cursor, so they also work after you moved it.
They move the cursor without dropping a selection: start one with `v`, then
`n` to stretch it to the next match. Going past the last match goes on from
the other end of the history, and a notification says so.

In copy mode, `n` also brings back the matches of your last search after you
left copy mode.

## Searching the word under the cursor

In copy mode, `*` searches the word under the cursor, forward, as if you had
typed it and pressed Enter: the cursor goes to the next match, the matches
stay lit, `n` and `N` go through them, and the word goes into the search
history. The word is the one a double-click selects. On a blank, it's the
next word on the line; a line with none after the cursor only shows a
notification.

The word is searched as text, not as a whole word: `*` on `err` also finds
`error`. For a backward search, bind `search-word-backward` in
`[copy_mode]`, for example to `#`; it has no key by default.

## Text search

A text search ignores case: `error` finds `ERROR`, and `été` finds `ÉTÉ`.

It searches what is on display: in line mode, the lines in their current
display mode; in terminal mode, the screen and the scrollback, or only the
screen while a full-screen app runs. In terminal mode the matches follow the
screen: when the device redraws it, the old ones go and the new ones light
up.

## Byte search

A byte search looks for bytes in the received data. Write them the way the
[raw side panel](raw-side-panel.md) shows them:

| Panel view | Examples |
| --- | --- |
| hex (default) | `0d0a`, `0d 0a`, `0x0d 0x0a`, `\x0d\x0a` |
| octal | `015 012` |
| binary | the bits of each byte |

## From the command line

`search <text>` on the [command line](command-line.md) starts a search forward
for that text, the bar open on it. Bind it to a key for a search you use
often.

## Colors

The `[search]` section of `theme.toml`: `search_match` for the matches,
`search_match_active` for the current one, and `search_indicator` for the
status bar's `SRCH` badge.
