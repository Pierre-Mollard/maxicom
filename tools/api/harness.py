"""A real maxicom for the API's integration tests (MAX-284 step 2d): run on a pty of its own, as
in a terminal, against a TCP listener playing the device, with a throwaway HOME whose
config.toml turns the API on. Linux and macOS (Python's `pty`).

    with Device() as device, Maxicom(device.address) as maxicom:
        client = maxicom.client()
        device.send(b"hello\\r\\n")
        maxicom.type(b"\\x01e")             # Ctrl-a e: toggle local echo

The binary is `target/debug/maxicom`, built by `build()`, or `MAXICOM_BIN`.
"""

import fcntl
import os
import pty
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import threading
import time
from pathlib import Path

from maxicom_client import Client

REPO = Path(__file__).resolve().parent.parent.parent
CONFIG = "[api]\nenabled = true\n"


def binary():
    return Path(os.environ.get("MAXICOM_BIN") or REPO / "target" / "debug" / "maxicom")


def build():
    """`cargo build` of maxicom, unless `MAXICOM_BIN` names one"""
    if "MAXICOM_BIN" not in os.environ:
        subprocess.run(["cargo", "build", "--quiet", "--bin", "maxicom"], cwd=REPO, check=True)


def until(done, what, timeout=5.0):
    """Waits for `done()` to be true, `what` said when it isn't by `timeout`"""
    deadline = time.monotonic() + timeout
    while not done():
        if time.monotonic() > deadline:
            raise TimeoutError(f"{what}: not in {timeout} s")
        time.sleep(0.02)


class Device:
    """A TCP listener playing the device: one connection, what maxicom sends kept"""

    def __init__(self):
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.address = "tcp://127.0.0.1:%d" % self.listener.getsockname()[1]
        self.conn = None
        self.lock = threading.Lock()
        self.got = bytearray()
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self):
        try:
            self.conn, _ = self.listener.accept()
        except OSError:
            return
        while True:
            try:
                chunk = self.conn.recv(65536)
            except OSError:
                return
            if not chunk:
                return
            with self.lock:
                self.got += chunk

    def connected(self):
        return self.conn is not None

    def send(self, data):
        until(self.connected, "maxicom connected to the device")
        self.conn.sendall(data)

    def received(self):
        """What maxicom sent so far"""
        with self.lock:
            return bytes(self.got)

    def hang_up(self):
        """The device closes its end: maxicom loses the link"""
        if self.conn is not None:
            # the reader thread holds it in `recv`: a close alone sends nothing
            try:
                self.conn.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.conn.close()
        self.listener.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.hang_up()


class Maxicom:
    """maxicom on a pty of `cols` × `rows`, on `address`, in a folder of its own: its HOME,
    config, cache and runtime folders. What it draws is read and dropped"""

    def __init__(self, address, cols=100, rows=30):
        self.dir = Path(tempfile.mkdtemp(prefix="mx-", dir="/tmp"))
        self.env = dict(os.environ)
        for name, sub in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
        ]:
            folder = self.dir / sub
            folder.mkdir(mode=0o700)
            self.env[name] = str(folder)
        self.env["TERM"] = "xterm-256color"
        config = self.dir / "config" / "maxicom"
        config.mkdir()
        (config / "config.toml").write_text(CONFIG)

        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.process = subprocess.Popen(
            [str(binary()), "--tcp", address.removeprefix("tcp://")],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=self.env,
            # its own session, the pty its controlling terminal, as a shell gives it
            start_new_session=True,
            preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
        )
        os.close(slave)
        threading.Thread(target=self._drain, daemon=True).start()

    def _drain(self):
        while True:
            try:
                if not os.read(self.master, 65536):
                    return
            except OSError:
                return

    def client(self, timeout=10.0):
        """A client connected to it, once it listens"""
        return Client.connect(self.process.pid, self.env, timeout)

    def type(self, keys):
        """Keys typed in its terminal"""
        os.write(self.master, keys)

    def stop(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        os.close(self.master)
        shutil.rmtree(self.dir, ignore_errors=True)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.stop()
