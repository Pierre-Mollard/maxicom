# Highlights

Highlights color words of the received text wherever they show, in both
modes. An `ERROR` in red stands out in a scrolling log.

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
words = ["OK", "ESP_OK", "SUCCESS", "PASS", "PASSED"]
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
ESP_OK = "Green"          # a word = its color

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
| `whole_word` | `false` | Only with no letter, digit or `_` right before or after: `OK` doesn't light up in `token` or `ESP_OK` |

Matching happens on each row of the screen as drawn. A word cut by a wrap,
at the end of one row and the start of the next, isn't lit. Highlights are
drawn over the device's own colors.

## When highlights overlap

The entries apply in the order of the file, a later one drawn over an
earlier one. With `FAIL` in red first and `FAILED` in yellow after,
`FAILED` is yellow.

The words written as `WORD = "color"` go at the top of the file, before
any table: in TOML, a key under a table belongs to that table. So they
always come first, and a table can be drawn over them.

## Errors

A file that doesn't parse stops maxicom at startup. On a reload, the
running highlights stay.
