# Highlights

Highlights color words of the received text wherever they show, in both
modes. An `ERROR` in red stands out in a scrolling log. They can also color
[whole lines](#whole-lines), from the way the lines start.

They live in `highlights.toml`, in the config folder, or where
`highlights_path` in `config.toml` points. `Ctrl-a r` applies a change.

## The file

This is the default file:

```toml
INFO = "Cyan" # a word on its own: its color


# A group: each word of words in this style, any case, whole words only
[errors]
words = ["ERROR", "ERR", "FAIL", "FAILED", "FAILURE", "FATAL", "PANIC"]
ignore_case = true
whole_word = true
fg = "White"
bg = "Red"
bold = true

[warnings]
words = ["WARN", "WARNING"]
ignore_case = true
whole_word = true
fg = "Yellow"
bold = true

[ok]
words = ["OK", "SUCCESS", "PASS", "PASSED"]
ignore_case = true
whole_word = true
fg = "Green"


# A word on its own in a style: a table without words styles its name
[TIMEOUT]
fg = "Magenta"
bold = true
```

Each table with `words` is a group: every word of its `words` list gets the group's
style. The name of the table is yours, it isn't searched. The style is
written as in the [theme](theme.md#writing-an-entry): `fg`, `bg`, `bold`
and the other keys, next to `words`.

## A word on its own

A word can also be a key of its own, as `INFO` and `TIMEOUT` in the default
file. Give it a color, or a style table without `words`: the table's name
is then the word it styles.

```toml
READY = "Green"           # a word = its color

[ERROR]                   # a table without words = a style for its name
fg = "White"
bg = "Red"
bold = true
```

Quote a key that has spaces or symbols:

```toml
"Guru Meditation" = "LightRed"
```

A table without `words` takes `ignore_case` and `whole_word` too.

## How matching works

By default a word is found exactly as written, and inside other words too:
`ERROR` doesn't match `error`, but lights up in `ERRORS`. Two keys change
that, for a group or a word:

| Key | Default | When `true` |
| --- | --- | --- |
| `ignore_case` | `false` | Any case: `error`, `Error` and `ERROR`, with letters beyond ASCII too (`échec` finds `ÉCHEC`) |
| `whole_word` | `false` | Only with no letter, digit or `_` right before or after: `OK` doesn't light up in `token` or `NOT_OK` |

Words are matched in each received line, not in the rows of the screen. A
word that wraps over two rows is lit on both, and one cut by the edge of the
view while you pan is lit on its visible part, as long as the whole word
matches. In line mode the time column and the line prefix are never matched.

In terminal mode, a line the device wrapped over several rows is matched as
one line too. A line that starts above the screen's rows (scrolled back far
enough) is matched from its first row in view.

Highlights are drawn over the device's own colors. Search matches and the
copy selection are drawn over the highlights.

## Whole lines

A table with `line_start` in place of `words` colors whole lines: every
line that starts with one of its texts gets the table's style, on all the
rows it takes. It suits logs that begin each line with their level:

```toml
[error-lines]
line_start = ["ERROR:", "[E]"]
fg = "Red"

[warning-lines]
line_start = ["WARN:", "[W]"]
fg = "Yellow"
```

The text must be at the very start of the line: `  ERROR:` with spaces
before it doesn't match. Add the spaces to the text if your lines have them.
`ignore_case` works as for words; `whole_word` doesn't apply, and a table
can't have both `words` and `line_start`: maxicom refuses the file.

A background (`bg`) covers the line's text, not the time column nor the
empty end of the row. The style is laid over the colors the device sent:
what it doesn't set, often the background, stays. The device's color codes
don't count as the start of the line, except in the `raw` display of line
mode, which shows them: there a colored line starts with `^[` and the rule
doesn't match.

The default file doesn't use `line_start`, so no device gets its lines
colored by surprise. Its header has the `error-lines` example to copy.

## When highlights overlap

The entries apply in the order of the file, a later one drawn over an
earlier one. With `FAIL` in red first and `FAILED` in yellow after,
`FAILED` is yellow.

Whole-line entries apply first, wherever they are in the file, so the words
are drawn over a colored line: an `ERROR` in it keeps its own style.

The words written as `WORD = "color"` go at the top of the file, before
any table: in TOML, a key under a table belongs to that table. So they
always come first, and a table can be drawn over them.

## Errors

A file that doesn't parse stops maxicom at startup. On a reload, the
running highlights stay.
