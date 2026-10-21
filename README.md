# MAXICOM

minicom but written in rust with my custom features.

## NOTES

- Build with cargo

```bash
cargo build
```

- Run with local bash redirect for testing interactive programs

```bash
./scripts/run_shell.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baud 115200 --port /tmp/maxicom-shell-test
```

- Run the bin utility "fake_device" for testing dumb programs

```bash
./scripts/run_fake.sh
```

```bash
cargo run --bin maxicom --profile profiling -- --baud 115200 --port /tmp/maxicom
```

- Profile (flame chart) with sample

```bash
cargo build --bin maxicom --profile profiling
```

```bash
samply record ./target/profiling/maxicom --baud 115200 --port /tmp/maxicom-shell-test
```

- Check license compat

```bash
cargo deny check
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
