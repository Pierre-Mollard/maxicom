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

## Reading them again

maxicom keeps what the notifications said, so you can read again one that
went by too fast. Type `notifications` on the
[command line](command-line.md#notifications) to open the list.

No key opens it by default. If you use it often, give it one in
`keymap.toml`, for example `Ctrl-a H`:

```toml
[prefixed]
H = "notifications"
```

### The notifications popup

Each row shows the level's icon, the time and the text. The newest is at the
bottom, and it's selected when the popup opens. A text too long for its row
is cut with `…`, and shows whole under the list when you select its row.

On top of the [popup keys](popups.md):

| Key | Does |
| --- | --- |
| Enter, `c` | Copies the selected text |

A click selects a row, and the Copy button copies it too. The list is the one
when the popup opened: a notification coming while it's open shows the next
time.

### What is kept

The list keeps each message once:

- a message said again while its box shows, counted `×2` in its title, isn't
  added again;
- in a box that follows an activity, like a file transfer or the link, each
  new message is added;
- a message is never added twice in a row, even from another box.

A message that comes back after its box went is added again, with its new
time. The list keeps the last 500.

### In the session's folder

The same lines go to a `.notices` file in the
[session backup's folder](capture.md#the-files-of-a-session), so you can
read them after maxicom quit:

```text
14:02:31 warn    Stopped sending fw.bin at 42%: no answer
14:02:35 info    Reconnected to /dev/ttyUSB0
```

The file starts with the first notification. During a replay, the lines go
to the session the replay set aside. Without a session backup, there's no
file, but the popup still works.

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
