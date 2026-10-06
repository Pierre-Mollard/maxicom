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
  - `plain text`                  Base theme style
  - `{variable}`                  Dynamic runtime value
  - `[content](style)`            Styled text span
  - `[content](cond?true:false)`  Conditional style
  - `(style1 style2)`             Group multiple styles with spaces

- Variables:
  - {mode}           "TERM" or "LINE"
  - {port}           Serial device path (e.g. /dev/ttyUSB0)
  - {baud}           Baud rate (e.g. 115200)
  - {framing}        Serial framing (e.g. 8N1)
  - {rx_bytes}       Received bytes count
  - {tx_bytes}       Transmitted bytes count
  - {time}           Local time (via time_format)
  - {pan_left}       Left pan amount (`◀ (3)`)
  - {pan_right}      Right pan amount (`!(2) ▶`)
  - {scroll_pos}     Position: "All", "Bot", "Top", "42%"
  - {scroll_pos_up}  Uppercase position: "ALL", "BOT", "TOP"
  - {scroll_percent} Numeric percent: 0..100
  - {scroll_offset}  Lines scrolled away from tail
  - {scroll_max}     Total scrollback depth

- Conditions:
  - Modes & Focus:
    - prefix         Prefix key (Ctrl-A) is active
    - rec            File capture is active (alias: logging)
    - line           Line input active (alias: linemode)
    - term           Terminal mode active (alias: terminal)
    - copy           Scrollback mode active (alias: copymode)
    - search         Search mode is active
    - overlay        Dialog/popup is open (alias: popup)
    - focused        Window has OS desktop focus

  - Viewport & Scroll:
    - is_live        Following live tail (alias: live, bot, bottom)
    - is_top         Scrolled to top of buffer (alias: top)
    - is_scrolled    Viewing past buffer history (alias: scrolled, paused)
    - scroll_up      History available above (alias: can_scroll_up)
    - scroll_down    History available below (alias: can_scroll_down)
    - has_scroll     Buffer exceeds visible screen height

  - Horizontal Pan:
    - panned         Viewport is panned (alias: pan, panning)
    - pan_left       Can pan left (alias: can_pan_left)
    - pan_right      Can pan right (alias: can_pan_right)
    - pan_alert      Unread content to right (alias: pan_right_alert)

- Styles:
  - Colors:
    black, red, green, yellow, blue, magenta, cyan, gray,
    darkgray, white, or hex (#RRGGBB)
  - Specifiers:
    `fg=<color>`, `bg=<color>`
  - Modifiers:
    bold, dim, italic, underline, reverse, crossed,
    sblink (slow blink), rblink (rapid blink), reset
  - Visibility:
    hide, hidden, none
