# Marks

A mark is a letter, `a` to `z`, put on a line of the history so you can come
back to it later, like vi's marks. Mark the moment a board resets, the start
of a test, an error, then jump back to it from anywhere in the history.

Each mode keeps its own marks: the ones set in line mode are on the line
view's lines, the ones set in terminal mode on the terminal's rows.

## Adding a mark

Press `Ctrl-a ,`, then a letter. While maxicom waits for the letter, the
mode badge in the status bar reads `MARK` in green. Enter takes the first
free letter; any other key cancels. A letter already in use moves to the
new line.

The mark goes on the line you're looking at:

| Where you are | The mark goes on |
| --- | --- |
| The live view | The last line received; in terminal mode, the cursor's row |
| Scrolled up | The line in the middle of the view |
| Copy mode | The cursor's line |

With the mouse, scroll up or enter copy mode, then right-click a line and
pick Add mark: it marks that line with the first free letter.

A toast says which letter was set.

## Going to a mark

`Ctrl-a '` opens the [marks popup](#the-marks-popup), where a letter jumps
to its mark. `Ctrl-a (` and `Ctrl-a )` jump to the mark above and below the
middle of the view.

Outside copy mode, a jump only scrolls: the mark's line lands in the middle
of the view, and what you type still goes to the device. In copy mode the
cursor moves to the start of the line; the view moves only when the line is
off screen. Either way the line flashes for a moment, so you see where you
landed.

## The marks popup

The popup lists the marks in history order, one row each: the letter, the
time the line came in, and the line in its colors.

On top of the [popup keys](popups.md):

| Key | Does |
| --- | --- |
| A letter | Jumps to that mark at once, `q`, `j` and `k` included |
| Enter | Jumps to the selected mark |
| Delete | Removes the selected mark; the popup stays open |
| Esc | Closes the popup |

A click selects a row, a click on the selected row jumps to it. The Jump
and Remove buttons in the bottom border do the same as Enter and Delete.

The right-click menu has Marks… while there are marks, in any view.

## In copy mode

The same keys work in copy mode, without the prefix: `,` marks the cursor's
line, `'` opens the popup, `(` and `)` move the cursor to the mark above and
below.

## How marks show

A mark's letter replaces the column that marks the history's rows, on the
line's first row on screen: the bar after the timestamp, or the inset
prefix, or else the left border. When the line has none of these, no letter
shows, but the scrollbar still does: the text is never covered.

The scrollbar shows each mark's letter at its place in the whole history,
so marks far up or down are easy to find. On the thumb, the letter takes
the thumb's color behind it.

Each part can be turned off in [`[ui.marks]`](settings.md#uimarks), and
their colors are in the theme's [`[ui]`](theme.md#ui): `mark`,
`scrollbar_mark` and `flash`.

## When marks go

When the oldest lines leave the history, past its size, their marks go with
them. Clear history (`Ctrl-a C`) removes them all; Clear (`Ctrl-a c`) keeps
them. Marks last as long as maxicom runs: they aren't saved.

## Commands

| Command | Does |
| --- | --- |
| `mark [letter]` | Marks the line as `Ctrl-a ,` does; without a letter, waits for it |
| `jump-mark [letter]` | Jumps to a mark without the popup; without a letter, waits for it |
| `delete-mark <letter>` | Removes a mark |
| `marks` | Opens the marks popup |
| `prev-mark` / `next-mark` | Jumps to the mark above / below |

Bind them like any command: `jump-mark a` on a key jumps straight to `a`.
See [Command line](command-line.md#marks) and
[Keybindings](keybindings.md).
