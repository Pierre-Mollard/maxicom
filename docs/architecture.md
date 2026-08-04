# ARCHITECTURE

## Plan

- main
- commands
- serial/
  - settings.rs
  - transport.rs

## Main loops

### Input sources

- Keyboard
- serial
- timer
- unix sockets
- signals

### MODES

- Normal / Serial Pass through
- PrefixPending
- Command Prompt
- Help Overlay
- Setting Overlay
- Confirm Quit
- Scrollback mode

### Design

input sources --> app event --> main update loop (as mode)
parallel renderer (never owns logic or serial I/O)
use queues for events
serial workers for rendering and serial coms

### Tasks

App main loop: running, input, command, status, scrollaback, rx/tx, catpure, dirty flag, render time
Serial worker: owns tty, read bytes, sends events, receives requests (write, reconnect, disconnects)
Serial worker: render TUI

### Unix sockets

Use unix socket with an exectuable (maxicomctl ?) to send commands to the executable using a defined socket in args.
