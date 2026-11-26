# API

maxicom can be driven by other programs: a script that waits for a boot
prompt and answers it, a test bench that reads what the board printed, an AI
tool that sends a command and reads the reply. They connect to a local
socket and exchange JSON messages with the maxicom you see on screen.

The API uses the same commands as the [command line](command-line.md). A
program reads the state with the getters (`status`, `raw-since`,
`history-since`...), and runs commands the way you type them on the command
line. There is no second command set to learn.

## Turning it on

The API is off by default. Turn it on in `config.toml`, then reload the
config (`Ctrl-a r`) or start maxicom again:

```toml
[api]
enabled = true
```

What a program may do once connected is up to you: see
[Permissions](#permissions) below and the
[`[api]` settings](settings.md#api).

## Where it listens

Each maxicom listens on its own socket, named after its process id:

| System | Socket |
| --- | --- |
| Linux | `$XDG_RUNTIME_DIR/maxicom/maxicom-<pid>.sock` |
| Linux without `$XDG_RUNTIME_DIR`, macOS | `maxicom-<uid>/maxicom-<pid>.sock` in the temporary folder (`/tmp`, or `$TMPDIR` on macOS) |
| Windows | The named pipe `\\.\pipe\maxicom-<pid>` |

Only your user can connect. On Linux and macOS the folder belongs to you and
nobody else can enter it; maxicom refuses to use a folder another user owns
or can write to. On Windows the pipe lets only your account in.

To find the maxicoms running, list the socket folder (or the pipes whose
name starts with `maxicom-` on Windows). A socket file left by a maxicom that
was killed refuses the connection, and the next maxicom to start removes it.

A program on another computer reaches the API through SSH; maxicom never
opens a network port.

## Messages

The messages are [JSON-RPC 2.0](https://www.jsonrpc.org/specification), one
per line: each message is a JSON object on a single line, ended by a newline.

A request has a `method`, its `params` given by name, and an `id`. The reply
carries the same `id`, with a `result` or an `error`:

```json
{"jsonrpc":"2.0","id":1,"method":"query","params":{"command":"get local-echo"}}
{"jsonrpc":"2.0","id":1,"result":false}
```

`method`, `params`, `id`, `result` and `error` are JSON-RPC's own names. What
goes in `params` and `result` is each method's, described below.

A message with no `id` is a notification: maxicom runs it and doesn't reply.
maxicom sends notifications too, for the events a program subscribed to.

A few rules about the lines themselves:

| What comes in | What happens |
| --- | --- |
| A line that isn't JSON | An error with code -32700, then the connection closes: the stream is out of step |
| JSON that isn't a request, or a batch (an array) | An error with code -32600, and the connection goes on |
| `params` given by position (an array) | An error with code -32602: give them by name |
| A line longer than 1 MiB | An error, then the connection closes |
| Empty lines | Skipped |

## Methods

| Method | Does |
| --- | --- |
| `hello` | Says who this maxicom is, and what the program may do |
| `query` | Runs one getter and gives its value at once |
| `run` | Runs a command line as a job, and replies when the job ends |
| `send` | Sends raw bytes, given in base64 |
| `cancel` | Stops this connection's jobs |
| `subscribe` | Asks for notifications: events, bytes received and sent, new lines and rows |
| `unsubscribe` | Stops some notifications, or all |

### hello

`hello` takes an optional `client`, a name the program gives itself. maxicom
keeps it with the connection: the ask popup and the events this program
causes show it.

```json
{"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"bench"}}
```

| Field | Is |
| --- | --- |
| `api` | The API's version, 1 today. Check it first |
| `version` | maxicom's version |
| `pid` | This maxicom's process id |
| `link` | The link it uses, as the status bar names it |
| `since` | When this maxicom started, UTC |
| `access` | The levels the program has: `read`, `control` and `device`, see [Permissions](#permissions) |

`hello` always answers, whatever the levels, so a program can find out what
it may do.

### query

`query` runs one getter of the [command line](command-line.md#reading-the-state)
and gives its value as JSON. Its `command` is written as you would type it:

```json
{"jsonrpc":"2.0","id":2,"method":"query","params":{"command":"raw-since 0"}}
{"jsonrpc":"2.0","id":2,"result":{"oldest":0,"newest":4,"cut":false,"start":0,"bytes":"b2sNCg=="}}
```

A query runs at once and never waits: it reads the state and changes nothing.
It isn't a job, so it doesn't queue behind anything. The values are described
with the getters in [Reading the state](command-line.md#reading-the-state). In
JSON, a text is a string, a number a number, an on/off a boolean, a list an
array, a set of named values an object in the same order, and bytes a base64
string.

`marks`, `triggers` and `notifications` give their content, as they do in a
trigger's line. A command that isn't a getter, or more than one command, is
refused with code 110: use `run` for those.

### run

`run` runs a command line as a job of this connection. The `command` is any
line the command line takes, several commands with `;` included:

```json
{"jsonrpc":"2.0","id":3,"method":"run","params":{"command":"set local-echo on; get local-echo"}}
{"jsonrpc":"2.0","id":3,"result":{"job":0,"notes":[],"values":[{"query":"get","value":true}]}}
```

The reply comes when the job ends, not when it starts. A `sleep`, an
`open`, a `mash`, a `wait-for` or a file transfer in the line make it wait.
A `wait-for` gives the text that came in as its value. Several `run`
can be in flight at once, each with its own `id`. A program's jobs run one
after the other, the oldest first, next to yours and to the triggers'.

| Field | Is |
| --- | --- |
| `job` | The job's number, for `cancel` |
| `notes` | What the commands said, the way they would in a toast |
| `values` | The value of each getter in the line, with its name in `query` |

When the job stops before its end, the reply is an error instead. Its message
names the command and why, and says how many commands didn't run, like
`send: device access is off in [api] (1 more not run)`. Its code says what
stopped it, see [Errors](#errors). A line that doesn't parse is refused at
once, before anything runs.

A program's job never shows a toast or opens a popup: what it reports goes
back to the program. Commands that need you at the keyboard, like `help` or
`mark` without its letter, are refused, as they are in a trigger's line.

A line starting with `cancel` cancels this connection's jobs first, then
runs the rest, as your command line does.

### send

`send` writes raw bytes to the device, given in base64 in `bytes`. It is for
binary data a command line could only write with escapes:

```json
{"jsonrpc":"2.0","id":4,"method":"send","params":{"bytes":"aGkNCg=="}}
{"jsonrpc":"2.0","id":4,"result":{"job":3,"notes":[],"values":[]}}
```

It runs as a `send` command of this connection, so it goes through the
`device` level and the ask popup like any other, and its reply comes once
the bytes are written. Bytes that aren't valid base64 are refused with code
-32602.

### cancel

`cancel` stops this connection's jobs. With `job`, it stops only that one:

```json
{"jsonrpc":"2.0","id":5,"method":"cancel","params":{"job":3}}
{"jsonrpc":"2.0","id":5,"result":{"notes":["Cancelled job 3"]}}
```

Each `run` that was waiting for a job cancelled gets its reply, an error
with code 113. A program can only cancel its own jobs: a job number that
isn't one of them is refused with code -32602.

When a program's connection closes, its jobs are cancelled the same way.

### subscribe and unsubscribe

`subscribe` asks for notifications. Its `events` lists what to follow: event
kinds (see [Events](#events)) and these streams:

| Stream | Notification | Its params |
| --- | --- | --- |
| `rx` | Bytes came in | `newest`: the offset the next byte will get |
| `tx` | Bytes were sent | `sent`: the count of bytes sent |
| `history` | Line mode got new lines | `newest`: the number the next line will get |
| `rows` | Terminal rows scrolled up | `newest`: the number the next row will get |

Without `events`, it subscribes to every kind and every stream. Each
`subscribe` adds to what the connection follows already.

The notifications only say that something new is there; the getters give
it. A program that follows the bytes keeps the offset it read up to, and on
each `rx` notification asks `raw-since` from there. Lines and rows work the
same way with `history-since` and `rows-since`.

The reply gives where things stand:

| Field | Is |
| --- | --- |
| `last` | The number of the last event so far |
| `oldest` | The oldest event still kept (the last 1000 are) |
| `rx`, `tx`, `history`, `rows` | Where each stream is now |
| `events` | Everything this connection follows now |

With `since`, an event number, the events after it are sent again, for the
kinds this `subscribe` adds. A program that reconnects asks from the last
event it saw, and misses nothing still kept.

`unsubscribe` takes `events` off what the connection follows, the others
kept. Without `events`, it stops them all. Its reply gives what is still
followed.

## Events

An event is something that happened in the session: the link opened or
lost, a trigger fired, a file received, a setting changed. Each comes as an
`event` notification:

```json
{"jsonrpc":"2.0","method":"event","params":{"seq":3,"time":"2026-11-26T20:44:17.981Z","kind":"setting-changed","setting":"local-echo","value":false,"cause":{"kind":"client","name":"bench"}}}
```

Every event has its number (`seq`, counting up from 1), its time in UTC,
its `kind`, and fields of its own.

### Events with a cause

The actions that reach the device or the link carry a `cause`: who did it.
Its `kind` is `input`, `command-line` or `key-binding` for you, `trigger` or
`profile` with their `name`, `client` with the program's name, or `link`
for what opening a link sets by itself.

| Kind | Fields |
| --- | --- |
| `line-settings` | `baud`, `framing` (like `8N1`), `flow` |
| `line-level` | `line` (`dtr` or `rts`), `on` |
| `break-sent` | `ms` |
| `link-detached` | |
| `link-opening` | `label`: the link being opened. What comes of it is its own event |
| `setting-changed` | `setting` as `get` names it, `value` as `get` gives it |
| `replay-started` | `path`, `sessions` |
| `replay-stopped` | |
| `replay-control` | `control` (`pause`, `resume`, `delay`, `restart`), `delay_ms` |
| `transfer-started` | `direction` (`send` or `receive`), `name`, `size`, `protocol` |
| `mash-started` | `until` |

### Other events

| Kind | Fields |
| --- | --- |
| `link-opened` | `label`, `how` (`connected`, `reconnected`, `switched`) |
| `link-shared` | `label`, `holder` |
| `link-not-opened`, `reattach-failed` | `error` |
| `open-failed` | `error`, `back`: the link it went back to |
| `link-lost` | `link`: its kind |
| `link-idle` | `secs` |
| `write-failed` | `link`, `error` |
| `read-failed` | `link` |
| `break-failed` | `error` |
| `link-warning` | `text` |
| `sent` | `name`, `size`, `protocol` |
| `file-received` | `name`, `size` |
| `received-file` | `name`, `size`, `folder` |
| `received-files` | `count`, `size`, `folder` |
| `nothing-received` | |
| `transfer-stopped` | `stopped`, `why` |
| `mash-done` | `until` |
| `mash-stopped` | `why` |
| `trigger-fired` | `name`, `run`, `silent` |
| `unhandled` | `kinds`, `more` |
| `device-copied` | `base64_len` |
| `device-copy-failed` | `error` |
| `shell-seen` | `key` |
| `replay-finished` | |
| `session-restored` | `span` |
| `restore-failed` | `error` |
| `restore-overflowed` | |
| `sessions-pruned` | `message` |
| `backup-off` | `error` |
| `recording-stopped` | |
| `group-loaded` | `message`, `warn` |
| `config-warning`, `screen-warning` | `text` |

## Permissions

You decide what programs may do, in the `[api]` section of `config.toml`.
A program can't change these settings: only you do, in that file. Each
command belongs to one of three kinds:

| Level | Covers | Default |
| --- | --- | --- |
| `read` | `query`, `subscribe` and `unsubscribe`: reading the state | `on` |
| `control` | Commands that change maxicom itself: the view, marks, settings, recording | `on` |
| `device` | Commands that reach the device or the link: sending, DTR and RTS, line settings, opening a link, replays, file transfers, triggers and profiles | `off` |

`off` refuses, `on` allows. `control` and `device` also take `ask`: each
command shows you the API popup, with the program's name and the command,
for you to allow it once, allow that program's commands of the same kind
until it disconnects, or refuse it. With no answer within `ask_timeout`
(30 seconds by default), the command is refused. See
[Questions](popups.md#questions) for the popup's keys.

`set answer-queries` counts as `device`: with it on, maxicom answers the
device's queries by itself.

To know which level a command needs, query `commands` with its name: its
`access` says `read`, `control` or `device`. `hello` gives the program its
levels.

A reload applies new levels to the programs already connected.
`disconnect-clients` drops every program at once, and your own `cancel`
stops their jobs too, see
[Waiting and stopping](command-line.md#waiting-and-stopping).

## Errors

| Code | Meaning |
| --- | --- |
| -32700 | The line isn't JSON; the connection closes |
| -32600 | Not a request, or a batch |
| -32601 | No such method |
| -32602 | Wrong params: given by position, of the wrong type, missing, bad base64, a job that isn't yours |
| 101 | An unclosed quote in the command line |
| 102 | A bad escape in the command line |
| 103 | An unknown command |
| 104 | A missing argument |
| 105 | Too many arguments |
| 106 | A bad argument |
| 107 | A command failed while it ran: the link couldn't do it, the file wasn't there |
| 108 | A command of `[commands]` given arguments |
| 110 | `query` given something that isn't one getter |
| 111 | Refused by the levels in `[api]`, or a command that needs you at the keyboard |
| 112 | You refused the command, or didn't answer in time |
| 113 | The job was cancelled |

## A Python client

The repository has a small client in `tools/api/maxicom_client.py`, using
Python's standard library only. It finds the running maxicoms, connects,
and matches replies to requests:

```python
from maxicom_client import Client

client = Client.connect()                    # the only maxicom running, or pid=...
print(client.call("hello", {"client": "bench"}))
start = client.call("subscribe", {"events": ["rx"]})["rx"]
client.send(b"version\r\n")                  # needs device = "on" or "ask"
client.wait_for("rx", lambda params: params["newest"] > start)
print(client.query(f"raw-since {start}"))
```

`scripts/api_request.py` sends raw lines from a shell, to try things by
hand:

```sh
./scripts/api_request.py --list
./scripts/api_request.py hello
./scripts/api_request.py '{"jsonrpc":"2.0","id":1,"method":"run","params":{"command":"status"}}'
```

## Versioning

`hello` gives the API's version in `api`, 1 today. A change that breaks
programs written for a version gives the next one: check it before anything
else.
