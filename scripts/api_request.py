#!/usr/bin/env python3
"""Sends raw JSON-RPC lines to a running maxicom's API (MAX-74) and prints the replies.

  ./scripts/api_request.py '{"jsonrpc":"2.0","id":1,"method":"hello"}'
  ./scripts/api_request.py hello                    the same: a bare method name is wrapped
  ./scripts/api_request.py hello nope 'not json'    several lines, sent in order, one connection
  echo '{"jsonrpc":"2.0","id":1,"method":"hello"}' | ./scripts/api_request.py -
  ./scripts/api_request.py --list                   the running maxicoms
  ./scripts/api_request.py --pid 1234 hello         a given maxicom, when several run
  ./scripts/api_request.py --raw hello              the replies as they came, one per line

Lines are sent as they are, so malformed ones can be tried. A line that is a JSON object with no
"id" is a notification and gets no reply; any other line gets one. The script waits for the
replies it expects, or until maxicom closes the connection, or --timeout seconds (Unix).

maxicom needs `[api] enabled = true` in config.toml. Python's standard library only; runs on
Linux, macOS and Windows (the pipe \\\\.\\pipe\\maxicom-<pid>).
"""

import argparse
import json
import socket
import sys
from pathlib import Path

# the discovery and the connection are the API client's (tools/api)
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tools" / "api"))
from maxicom_client import Connection, NotFound, pick, running  # noqa: E402


def line_of(arg, number):
    """A bare method name becomes a request; anything else is sent as it is"""
    if arg.replace("-", "").replace("_", "").isalnum():
        return json.dumps({"jsonrpc": "2.0", "id": number, "method": arg})
    return arg


def expects_reply(line):
    """Everything but a notification (a JSON object without "id") gets a reply"""
    try:
        message = json.loads(line)
    except ValueError:
        return True
    return not (isinstance(message, dict) and "id" not in message)


def main():
    parser = argparse.ArgumentParser(
        description="Send raw JSON-RPC lines to a running maxicom's API, print the replies"
    )
    parser.add_argument("lines", nargs="*", help="JSON lines, or method names; - reads stdin")
    parser.add_argument("--pid", type=int, help="the maxicom to talk to, when several run")
    parser.add_argument("--list", action="store_true", help="list the running maxicoms")
    parser.add_argument("--raw", action="store_true", help="print replies as received")
    parser.add_argument("--timeout", type=float, default=3.0, help="seconds to wait (Unix)")
    args = parser.parse_args()

    if args.list:
        for pid, location in running():
            print(f"{pid}  {location}")
        return
    if not args.lines:
        parser.error("no line to send")

    lines = []
    for arg in args.lines:
        if arg == "-":
            lines += [line.rstrip("\n") for line in sys.stdin if line.strip()]
        else:
            lines.append(line_of(arg, len(lines) + 1))

    try:
        location = pick(args.pid)
    except NotFound as e:
        sys.exit(str(e))
    connection = Connection(location, args.timeout)
    for line in lines:
        print(f"> {line}", file=sys.stderr)
        connection.send(line)
    expected = sum(expects_reply(line) for line in lines)
    for _ in range(expected):
        try:
            reply = connection.receive()
        except (socket.timeout, TimeoutError):
            sys.exit("no reply in time")
        except ConnectionResetError:
            reply = None
        if reply is None:
            print("(connection closed by maxicom)", file=sys.stderr)
            break
        if args.raw:
            print(reply, flush=True)
        else:
            try:
                print(json.dumps(json.loads(reply), indent=2), flush=True)
            except ValueError:
                print(reply, flush=True)


if __name__ == "__main__":
    main()
