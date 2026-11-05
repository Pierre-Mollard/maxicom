# Notifications

Notifications are short messages that stack in the top right corner and go
away after a few seconds.

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
