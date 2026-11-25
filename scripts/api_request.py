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
import os
import socket
import sys
from pathlib import Path

PREFIX = "maxicom-"
SUFFIX = ".sock"
PIPES = "\\\\.\\pipe\\"


def socket_dir():
    """maxicom's socket folder: the runtime folder, else the cache folder (src/api.rs)"""
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if runtime and sys.platform.startswith("linux"):
        return Path(runtime) / "maxicom"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Caches" / "maxicom"
    cache = os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache"
    return Path(cache) / "maxicom"


def alive(path):
    """A socket file a maxicom listens on: a dead one's refuses the connect"""
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        probe.connect(str(path))
        return True
    except OSError:
        return False
    finally:
        probe.close()


def running():
    """(pid, location) of each maxicom found, by pid"""
    found = []
    if os.name == "nt":
        names = [name for name in os.listdir(PIPES) if name.startswith(PREFIX)]
        found = [(name[len(PREFIX):], PIPES + name) for name in names]
    else:
        folder = socket_dir()
        if folder.is_dir():
            for path in folder.iterdir():
                name = path.name
                if name.startswith(PREFIX) and name.endswith(SUFFIX) and alive(path):
                    found.append((name[len(PREFIX):-len(SUFFIX)], str(path)))
    return sorted((int(pid), location) for pid, location in found if pid.isdigit())


def pick(pid):
    """The maxicom to talk to: `pid`, else the only one running"""
    found = running()
    if pid is not None:
        for each in found:
            if each[0] == pid:
                return each[1]
        sys.exit(f"no maxicom with pid {pid} has its API on: {[p for p, _ in found]}")
    if len(found) == 1:
        return found[0][1]
    if not found:
        sys.exit("no maxicom with its API on ([api] enabled = true in config.toml)")
    listed = "\n".join(f"  {p}  {location}" for p, location in found)
    sys.exit(f"several maxicoms run, pick one with --pid:\n{listed}")


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


class Connection:
    """A Unix socket, or a Windows pipe opened as a file"""

    def __init__(self, location, timeout):
        if os.name == "nt":
            self.file = open(location, "r+b", buffering=0)
            self.reader = open(self.file.fileno(), "rb", closefd=False)
        else:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(timeout)
            self.sock.connect(location)
            self.file = self.sock.makefile("wb", buffering=0)
            self.reader = self.sock.makefile("rb")

    def send(self, line):
        self.file.write(line.encode() + b"\n")

    def receive(self):
        """A reply line, or None when maxicom closed the connection"""
        line = self.reader.readline()
        return line.decode(errors="replace").rstrip("\n") if line else None


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

    connection = Connection(pick(args.pid), args.timeout)
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
