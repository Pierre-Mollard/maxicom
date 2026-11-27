"""Integration tests of `maxicom api` (MAX-292): the stdio bridge, run as a subprocess the way
`ssh bench maxicom api` runs it, in front of a real maxicom on a pty. Linux and macOS; run by
hand, from the repository:

    python3 -m unittest discover -s tools/api -v
"""

import json
import os
import subprocess
import tempfile
import unittest

from harness import Device, Maxicom, binary, build, until
from maxicom_client import Client, location_of


def setUpModule():
    build()


class BridgeTest(unittest.TestCase):
    def setUp(self):
        self.device = Device()
        self.maxicom = Maxicom(self.device.address)
        self.client = self.maxicom.client()
        until(self.device.connected, "maxicom connected to the device")
        self.ids = 0

    def tearDown(self):
        self.client.close()
        self.maxicom.stop()
        self.device.hang_up()

    def bridge(self, *args, env=None):
        """`maxicom api ARGS` as ssh would run it: its stdin and stdout piped, killed at the
        end of the test"""
        process = subprocess.Popen(
            [str(binary()), "api", *args],
            env=env or self.maxicom.env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        def close():
            process.kill()
            process.wait()
            for pipe in (process.stdin, process.stdout, process.stderr):
                pipe.close()

        self.addCleanup(close)
        return process

    def send(self, bridge, method, params=None):
        """A request written on the bridge's stdin: its id"""
        self.ids += 1
        request = {"jsonrpc": "2.0", "id": self.ids, "method": method, "params": params or {}}
        bridge.stdin.write(json.dumps(request) + "\n")
        bridge.stdin.flush()
        return self.ids

    def read(self, bridge):
        return json.loads(bridge.stdout.readline())

    def test_requests_and_replies(self):
        """A request in on stdin, its reply out on stdout, a `run`'s once its job ended,
        under the name the program's own `hello` gave"""
        bridge = self.bridge()
        hello_id = self.send(bridge, "hello", {"client": "remote"})
        hello = self.read(bridge)
        self.assertEqual(hello["id"], hello_id)
        self.assertEqual(hello["result"]["pid"], self.maxicom.process.pid)
        run_id = self.send(bridge, "run", {"command": "sleep 200; get local-echo"})
        reply = self.read(bridge)
        self.assertEqual(reply["id"], run_id)
        self.assertEqual(reply["result"]["values"][0]["command"], "get local-echo")

    def test_notifications_pass_through(self):
        """A subscription's notifications come out on stdout like the replies"""
        bridge = self.bridge()
        self.send(bridge, "subscribe", {"events": ["setting-changed"]})
        self.assertIn("result", self.read(bridge))
        self.client.run("toggle-local-echo")
        event = self.read(bridge)
        self.assertEqual((event["method"], event["params"]["setting"]), ("event", "local-echo"))

    def test_stdin_closed_ends_the_session(self):
        """stdin closing ends the bridge (exit 0) and the connection: its job is cancelled"""
        bridge = self.bridge()
        self.send(bridge, "run", {"command": "sleep 10000; status"})
        jobs = lambda: self.client.query("status")["jobs"]
        until(lambda: jobs(), "the bridge's job running")
        bridge.stdin.close()
        self.assertEqual(bridge.wait(5), 0)
        until(lambda: not jobs(), "its job cancelled")

    def test_maxicom_quitting_ends_it(self):
        """maxicom quitting ends the bridge with exit 6"""
        bridge = self.bridge()
        self.send(bridge, "hello")
        self.read(bridge)
        self.client.close()
        self.maxicom.stop()
        self.assertEqual(bridge.wait(5), 6)
        self.assertEqual(bridge.stderr.read(), "maxicom api: maxicom quit\n")

    def test_no_instance_or_several(self):
        """No maxicom exits 3 at once; two need --instance, which picks one by its address"""
        with tempfile.TemporaryDirectory() as empty:
            bridge = self.bridge(env=dict(self.maxicom.env, XDG_RUNTIME_DIR=empty))
            self.assertEqual(bridge.wait(5), 3)
            self.assertIn("no maxicom is running", bridge.stderr.read())
        with Device() as other_device, Maxicom(other_device.address, beside=self.maxicom) as other:
            other.client().close()
            bridge = self.bridge()
            self.assertEqual(bridge.wait(5), 3)
            self.assertIn("several maxicoms match", bridge.stderr.read())
            bridge = self.bridge("--instance", other_device.address)
            self.send(bridge, "hello")
            self.assertEqual(self.read(bridge)["result"]["pid"], other.process.pid)


class ClientOverTheBridgeTest(unittest.TestCase):
    """The Python client through `Client.command`, the bridge standing in for ssh, and
    through `Client.connect(socket=...)`, a link standing in for a forwarded socket"""

    def setUp(self):
        self.device = Device()
        self.maxicom = Maxicom(self.device.address)
        self.local = self.maxicom.client()
        until(self.device.connected, "maxicom connected to the device")

    def tearDown(self):
        self.local.close()
        self.maxicom.stop()
        self.device.hang_up()

    def test_session_through_the_bridge(self):
        """`hello`, a query, a `run` and an event, as over a socket; closing the client ends
        the bridge, exit 0"""
        client = Client.command([str(binary()), "api"], env=self.maxicom.env)
        try:
            hello = client.call("hello", {"client": "remote"})
            self.assertEqual(hello["pid"], self.maxicom.process.pid)
            self.assertEqual(client.query("status")["state"], "open")
            client.call("subscribe", {"events": ["setting-changed"]})
            done = client.run("toggle-local-echo; get local-echo")
            self.assertEqual(done["values"][0]["command"], "get local-echo")
            event = client.event("setting-changed")
            self.assertEqual(event["value"], done["values"][0]["value"])
        finally:
            client.close()
        self.assertEqual(client.connection.process.returncode, 0)

    def test_bridge_says_why_it_ended(self):
        """A bridge with no maxicom to reach: the client's call fails with what it said, even
        when the bridge ended before the request was written"""
        with tempfile.TemporaryDirectory() as empty:
            env = dict(self.maxicom.env, XDG_RUNTIME_DIR=empty)
            client = Client.command([str(binary()), "api"], env=env)
            until(lambda: client.connection.process.poll() is not None, "the bridge ended")
            with self.assertRaises(ConnectionError) as ended:
                client.call("hello")
            client.close()
        self.assertIn("no maxicom is running", str(ended.exception))

    def test_socket_given(self):
        """`Client.connect(socket=...)` and `$MAXICOM_SOCKET` reach a socket outside the
        socket folder"""
        location = location_of(self.maxicom.process.pid, self.maxicom.env)
        with tempfile.TemporaryDirectory() as elsewhere:
            forwarded = os.path.join(elsewhere, "bench.sock")
            os.symlink(location, forwarded)
            client = Client.connect(socket=forwarded, timeout=2)
            try:
                self.assertEqual(client.call("hello")["pid"], self.maxicom.process.pid)
            finally:
                client.close()
            os.environ["MAXICOM_SOCKET"] = forwarded
            try:
                client = Client.connect(timeout=2)
            finally:
                del os.environ["MAXICOM_SOCKET"]
            try:
                self.assertEqual(client.call("hello")["pid"], self.maxicom.process.pid)
            finally:
                client.close()


if __name__ == "__main__":
    unittest.main()
