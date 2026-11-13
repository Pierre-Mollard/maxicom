# Notifications

Notifications are short messages that stack in the top right corner and go
away after a few seconds.

## How long they stay

A notification stays the time it takes to read it: 2 seconds, plus a second
for every 20 characters. A short one like "Text copied" goes in under 3
seconds, a line with a file's path stays about 4.

A warning stays at least 4 seconds and an error at least 6, however short, and
none stays more than 10. Click one to close it sooner.

The time counts from when the notification first shows: one that can't show
yet, on a screen too small for notifications, waits for you instead of going
unseen.

## When many come at once

Four notifications show at most. A new one then takes the place of the oldest
info or success; a warning goes only when none of those is left, and an error
only when all four are errors. A burst of messages can't push out an error you
haven't read.

The same message again while it shows doesn't add a box: its time starts over
and its title counts it, `Warning ×2`.

## One box per activity

The messages of a file transfer follow each other in one notification instead
of piling up: waiting, then each file received, then done or stopped. The
link's do the same: connecting then connected, lost then reconnected, detached
then attached. Each new message takes the box's place and starts its time
again.

What you go through one press at a time works the same way, so cycling
doesn't stack a box per press:

- the copies: the last one says what was copied;
- the [plot](plots.md): shown, switched, frozen, off;
- each setting you cycle (the raw side panel's view, the encoding, the display
  mode, Add CR...): its box shows its last value. Two different settings still
  show two boxes.

## Your own notifications

The `notify` command shows one of yours, at the level you choose:

```text
notify success "flash done, power-cycle the board"
```

From a [trigger](triggers.md#raising-a-notification) or a key bound to a
command line, it tells you about something the device printed. See
[Command line](command-line.md#notifications).

## Closing them

| To close | Do |
| --- | --- |
| One notification | Click anywhere on it. Over a popup, the popup stays open. |
| The top one | `Ctrl-a n` |
| All of them | `Ctrl-a N` |

## Levels

Each level has its color in `[notification]` of the theme, and `border` is
their frame.

| Level | For example |
| --- | --- |
| `info` | A setting changed, a trigger fired |
| `success` | Text copied, a file saved |
| `warn` | The port detached, a recording stopped |
| `error` | A command refused, a file that didn't open |

Errors you need to read in full, like a config file that doesn't load or a
failed reload, open a popup with the details instead. Esc or Enter closes
it.
