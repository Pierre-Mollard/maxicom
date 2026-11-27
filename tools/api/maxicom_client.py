"""A client of maxicom's API (MAX-74, MAX-284): finds a running maxicom, sends JSON-RPC 2.0
requests on its socket and collects what it pushes (`event`, `rx`, `tx` notifications).

    from maxicom_client import Client, running
    client = Client.connect()                 # the only maxicom running, or pid=...
    client.call("query", {"command": "status"})
    client.call("subscribe", {"events": ["link-lost"]})
    client.wait_for("event", lambda params: params["kind"] == "link-lost")

    client = Client.connect(socket="/tmp/bench.sock")             # a forwarded socket
    client = Client.command(["ssh", "bench", "maxicom", "api"])   # another computer's

Newline-delimited JSON, one message a line. Python's standard library only; Linux, macOS and
Windows (the pipe \\\\.\\pipe\\maxicom-<pid>). Used by `scripts/api_request.py` and the
integration tests next to it (`test_api.py`, `test_bridge.py`).
"""

import base64
import itertools
import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

# the socket to reach in place of the ones running here, as `maxicom cli` reads it (MAX-292)
SOCKET_VAR = "MAXICOM_SOCKET"
PREFIX = "maxicom-"
SUFFIX = ".sock"
PIPES = "\\\\.\\pipe\\"


def socket_dir(env=None):
    """maxicom's socket folder (src/api.rs): `maxicom` in the runtime folder, else
    `maxicom-<uid>` in the temporary folder, for a maxicom started with `env` (this process's
    environment by default)"""
    env = os.environ if env is None else env
    runtime = env.get("XDG_RUNTIME_DIR")
    if runtime and sys.platform.startswith("linux"):
        return Path(runtime) / "maxicom"
    return Path(env.get("TMPDIR") or "/tmp") / f"maxicom-{os.geteuid()}"


def location_of(pid, env=None):
    """Where the maxicom of `pid` started with `env` listens, whether it does or not"""
    if os.name == "nt":
        return f"{PIPES}{PREFIX}{pid}"
    return str(socket_dir(env) / f"{PREFIX}{pid}{SUFFIX}")


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


class NotFound(Exception):
    """No maxicom to talk to, or several and none picked"""


def pick(pid=None):
    """The location of the maxicom to talk to: `pid`, else the only one running"""
    found = running()
    if pid is not None:
        for each in found:
            if each[0] == pid:
                return each[1]
        raise NotFound(f"no maxicom with pid {pid} has its API on: {[p for p, _ in found]}")
    if len(found) == 1:
        return found[0][1]
    if not found:
        raise NotFound("no maxicom with its API on ([api] enabled = true in config.toml)")
    listed = "\n".join(f"  {p}  {location}" for p, location in found)
    raise NotFound(f"several maxicoms run, pick one with its pid:\n{listed}")


class Connection:
    """A Unix socket, or a Windows pipe opened as a file: lines in, lines out"""

    def __init__(self, location, timeout=None):
        if os.name == "nt":
            self.file = open(location, "r+b", buffering=0)
            self.reader = open(self.file.fileno(), "rb", closefd=False)
            self.sock = None
        else:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(timeout)
            try:
                self.sock.connect(location)
            except OSError:
                self.sock.close()
                raise
            self.file = self.sock.makefile("wb", buffering=0)
            self.reader = self.sock.makefile("rb")

    def send(self, line):
        self.file.write(line.encode() + b"\n")

    def receive(self):
        """A line, or None when maxicom closed the connection"""
        line = self.reader.readline()
        return line.decode(errors="replace").rstrip("\n") if line else None

    def close(self):
        if self.sock is not None:
            try:
                self.sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.sock.close()
        else:
            self.file.close()

    def why(self):
        """Why the connection ended, when something said so: nothing for a socket"""
        return None


class CommandConnection:
    """A command's stdin and stdout, as `ssh bench maxicom api` gives them (MAX-292): lines in,
    lines out, like a socket. What it writes on stderr is kept, to say why it ended"""

    def __init__(self, argv, env=None):
        self.process = subprocess.Popen(
            argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env
        )
        self.errors = []
        self.error_reader = threading.Thread(target=self._read_errors, daemon=True)
        self.error_reader.start()

    def _read_errors(self):
        for line in self.process.stderr:
            self.errors.append(line.decode(errors="replace").rstrip("\n"))

    def send(self, line):
        """A line written; a command already ended (ssh refused, no maxicom there) takes
        nothing, and the reader says why"""
        try:
            self.process.stdin.write(line.encode() + b"\n")
            self.process.stdin.flush()
        except BrokenPipeError:
            pass

    def receive(self):
        """A line, or None when the command closed its stdout (maxicom quit, ssh ended)"""
        line = self.process.stdout.readline()
        return line.decode(errors="replace").rstrip("\n") if line else None

    def close(self):
        """stdin closed, which ends the session; the command waited for a moment, then
        stopped"""
        try:
            self.process.stdin.close()
        except OSError:
            pass
        try:
            self.process.wait(2)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.process.stdout.close()
        self.process.stderr.close()

    def why(self):
        """What the command said on stderr, once it ended: `no maxicom is running`... Its
        stderr can end a moment after its stdout: waited for, a second at most"""
        self.error_reader.join(1)
        return "\n".join(self.errors) or None


class RpcError(Exception):
    """A JSON-RPC error reply: its `code`, `message` and `data`"""

    def __init__(self, error):
        self.code = error.get("code")
        self.data = error.get("data")
        super().__init__(f"{self.code}: {error.get('message')}")


class Client:
    """One connection, its replies matched to their requests by id, its notifications kept in
    order until taken. A reader thread reads every line as it comes. The connection is a
    socket (`connect`) or a command's pipes (`command`)"""

    def __init__(self, connection):
        self.connection = connection
        self.ids = itertools.count(1)
        self.lock = threading.Condition()
        self.replies = {}
        self.notifications = []
        self.closed = False
        threading.Thread(target=self._read, daemon=True).start()

    @classmethod
    def connect(cls, pid=None, env=None, timeout=10.0, socket=None):
        """The maxicom at the path `socket` (MAX-292), else the one of `pid` started with
        `env`, else at `$MAXICOM_SOCKET`, else the only one running; waited for until
        `timeout`: a maxicom just started listens a moment later"""
        deadline = time.monotonic() + timeout
        while True:
            try:
                if socket is not None:
                    location = socket
                elif pid is not None:
                    location = location_of(pid, env)
                else:
                    location = os.environ.get(SOCKET_VAR) or pick()
                return cls(Connection(location))
            except (NotFound, OSError):
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.05)

    @classmethod
    def command(cls, argv, env=None):
        """A session through a command's stdin and stdout (MAX-292), the bridge of another
        computer's maxicom: `["ssh", "bench", "maxicom", "api"]`. The command is started
        here; closing the client closes its stdin, which ends the session"""
        return cls(CommandConnection(argv, env))

    def _read(self):
        while True:
            try:
                line = self.connection.receive()
            except OSError:
                line = None
            with self.lock:
                if line is None:
                    self.closed = True
                    self.lock.notify_all()
                    return
                message = json.loads(line)
                if "id" in message:
                    self.replies[message["id"]] = message
                else:
                    self.notifications.append(message)
                self.lock.notify_all()

    def _wait(self, ready, timeout, what):
        """The value `ready` gives under the lock, once it gives one; `TimeoutError` past
        `timeout`, `ConnectionError` when maxicom closed first"""
        deadline = time.monotonic() + timeout
        with self.lock:
            while True:
                value = ready()
                if value is not None:
                    return value
                if self.closed:
                    why = self.connection.why()
                    said = f": {why}" if why else ""
                    raise ConnectionError(
                        f"maxicom closed the connection, waiting for {what}{said}"
                    )
                left = deadline - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"no {what} in {timeout} s")
                self.lock.wait(left)

    def request(self, method, params=None, timeout=5.0):
        """The whole reply to `method` (its `result` or its `error`)"""
        number = next(self.ids)
        message = {"jsonrpc": "2.0", "id": number, "method": method}
        if params is not None:
            message["params"] = params
        self.connection.send(json.dumps(message))
        return self._wait(lambda: self.replies.pop(number, None), timeout, f"reply to {method}")

    def call(self, method, params=None, timeout=5.0):
        """The result of `method`; its error raised as `RpcError`"""
        reply = self.request(method, params, timeout)
        if "error" in reply:
            raise RpcError(reply["error"])
        return reply["result"]

    def query(self, command, timeout=5.0):
        """A getter's value: `query {command}`"""
        return self.call("query", {"command": command}, timeout)

    def run(self, command, timeout=5.0):
        """A command line run as a job of this connection: `run {command}`, its reply when
        the job ends (its notes and its queries' values)"""
        return self.call("run", {"command": command}, timeout)

    def send(self, data, timeout=5.0):
        """Raw bytes sent as this connection's `send`: `send {bytes}`, base64"""
        encoded = base64.b64encode(data).decode("ascii")
        return self.call("send", {"bytes": encoded}, timeout)

    def wait_for(self, method, match=lambda params: True, timeout=5.0):
        """The params of the first notification `method` that `match` takes, taken off the
        ones kept; those before it stay"""

        def first():
            for i, message in enumerate(self.notifications):
                if message.get("method") == method and match(message.get("params")):
                    return self.notifications.pop(i)["params"]
            return None

        return self._wait(first, timeout, f"'{method}' notification")

    def event(self, kind, match=lambda params: True, timeout=5.0):
        """The first `event` notification of `kind` that `match` takes"""
        return self.wait_for("event", lambda p: p["kind"] == kind and match(p), timeout)

    def taken(self):
        """Every notification kept, taken off"""
        with self.lock:
            notifications, self.notifications = self.notifications, []
            return notifications

    def close(self):
        self.connection.close()
