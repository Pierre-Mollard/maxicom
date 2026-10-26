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

### UI

- `title_details` in `[ui]` (default `false`): the history box's title shows
  only ` maxicom LINE ` or ` maxicom TERMINAL `. With `true` it also shows the
  alerts (`!`, `● REC`), the VT and view sizes, the scroll, the pan and the
  prefix. What doesn't fit in the border is left out whole, never cut.
- `compact_cols` and `compact_rows` in `[ui]` (default `50` and `14`): while
  the screen is narrower or shorter than that, it is drawn without borders,
  status bar and scrollbar, to leave the room to the content. The settings
  stay as they are: a bigger window brings them back. `0` turns a side off.

### TERMINAL

- `dimension_mode` in `[terminal_mode]`: the size of the emulated screen. The
  device never learns it (there is no resize signal on a serial line), so it
  only changes what can be shown, and an automatic resize never loses data:
  - `grow_only` (default)  Grows with the view, never shrinks. When the window
                         gets smaller, the view follows the cursor
  - `match_view`           The view's size, but never smaller than the content
                         below the cursor or the widest row
  - `fixed_cols`           The view's rows, but never fewer than the content
                         below the cursor; `cols` columns
  - `fixed`                `rows` x `cols`
  A size set from the resize popup applies as asked. The settings popup
  cycles the mode with `S` (Terminal Mode section).

### THEME

#### STATUS BAR FORMAT

- Syntax:
  - `plain text`                  Base theme style (`status_bar`), on the UI
                                background unless the layout is `band`
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
  - Mouse:
    `click=<action>` runs a keymap action (e.g. `help`, `settings`, `quit`,
    `toggle_logging`) when the block is clicked
    `hover=<style>` styles the block under the mouse pointer, e.g.
    `hover=(bg=main fg=black)`, or `hover=none`; blocks without it take
    `hover` in `[ui.status_bar]` (default "reversed", "none" for no hover)

- Clicks: a block without `click=` runs what its first variable implies:
  - {mode}                       Switch between LINE and TERM
  - {baud}, {framing}            Open the serial port settings ({port} does nothing)
  - {copy_side}                  Switch the copy cursor's panel
  - {pan_left}, {pan_right}      Pan left / right
  - {scroll_pos}, {scroll_pos_up}, {scroll_percent}, {scroll_offset}
                                 Back to the live tail
  A variable in plain text (outside `[...]`) is clickable on its own.

- Layout: `layout` in `[ui.status_bar]`
  - `line`             A row of its own, plain text on the UI background
  - `band`             A row filled with the `status_bar` bg
  - `border` (default) In the bottom border of the last box (TX box in line
                     mode, history in terminal mode, where a search takes
                     its place while open)
  - `box`              In a box of its own, 3 rows high
  `border` and `box` need borders, else they are `line`. With borders on,
  `line` and `band` are inset one column each side.
