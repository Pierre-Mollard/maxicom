# Highlights

Highlights color words of the received text wherever they show, in both
modes. An `ERROR` in red stands out in a scrolling log.

They live in `highlights.toml`, in the config folder, or where
`highlights_path` in `config.toml` points. `Ctrl-a r` applies a change.

## The file

This is the default file:

```toml
ESP_OK = "Green"          # a word = its color

[ERROR]                   # a table = a style
fg = "White"
bg = "Red"
bold = true

[WARN]
fg = "Yellow"
bold = true
```

Each key is the text to find, and its value a color or a style table, as in
the [theme](theme.md#writing-an-entry). Quote a key that has spaces or
symbols:

```toml
"Guru Meditation" = "LightRed"
```

## How matching works

- The text is found exactly as written, case included: `ERROR` doesn't
  match `error`.
- It matches inside words too: `ERROR` lights up in `ERRORS`.
- Matching happens on each row of the screen as drawn. A text cut by a
  wrap, at the end of one row and the start of the next, isn't lit.
- Highlights are drawn over the device's own colors.

## Errors

A file that doesn't parse stops maxicom at startup. On a reload, the
running highlights stay.
