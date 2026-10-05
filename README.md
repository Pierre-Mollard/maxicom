# MAXICOM

minicom but written in rust with my custom features.

## PLAN

1. Example @ 115200 baud on testing port
2. Add data bits, parity, stop bits and flow control

## NOTES

1. Build with cargo

```bash
cargo build
```

1. Run with cargo and arguments

```bash
cargo run -- --port /dev/ttyUSB0 --baud 115200 --test coucou alkgjl df - sflj
```

1. Run the bin utility "fake_device" for testing (target)

```bash
cargo run --bin fake_device
```

## CONFIG

### THEME

#### STATUS BAR FORMAT

- Syntax:
  - `plain text`, rendered using base style 'status_bar' in theme
  - `{variables}`, replaced at runtime from list below
  - `[content](style)`, content rendered using specified style
  - `[content](condition?true_style:false_style)`, conditionnal from list below
  - `(style1 style2 style3)`, use parenthesis to regroup styles with spaces in conditions

- Variables:
  - {mode}       => "TERM" or "LINE"
  - {port}       => Active serial port (e.g. /dev/ttyUSB0)
  - {baud}       => Baud rate (e.g. 115200)
  - {framing}    => Serial framing (e.g. 8N1)
  - {rx_bytes}   => Bytes received counter
  - {tx_bytes}   => Bytes sent counter
  - {time}       => Local time (configured via time_format)

- Conditions:
  - prefix       => When Ctrl-A prefix mode is active
  - rec          => When recording to file is active
  - line         => When line input mode is active
  - term         => When terminal input mode is active
  - copy         => When copy mode is active
  - search       => When search mode is active
  - overlay      => When a dialog or popup window is active
  - focused      => When the application has the cursor focus

- Styles:
  - Colors       => black, red, green, yellow, blue, magenta, cyan, gray, #RRGGBB
  - Specifiers   => `fg=<color>`, `bg=<color>`
  - Modifiers    => bold, dim, italic, underline, reverse, crossed, sblink, rblink
  - Visibility   => hide, hidden, none
