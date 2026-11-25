"""Integration tests of maxicom's API (MAX-284 step 2d): a real maxicom on a pty, a TCP device,
a client on its socket. Linux and macOS; run by hand, from the repository:

    python3 -m unittest discover -s tools/api -v

`cargo build` runs first (or set `MAXICOM_BIN`). Each test starts its own maxicom.
"""

import base64
import unittest

from harness import Device, Maxicom, build, until
from maxicom_client import RpcError

CTRL_A = b"\x01"


def setUpModule():
    build()


class ApiTest(unittest.TestCase):
    def setUp(self):
        self.device = Device()
        self.maxicom = Maxicom(self.device.address)
        self.client = self.maxicom.client()
        until(self.device.connected, "maxicom connected to the device")

    def tearDown(self):
        self.client.close()
        self.maxicom.stop()
        self.device.hang_up()

    def subscribe(self, *events, since=None):
        params = {"events": list(events)} if events else {}
        if since is not None:
            params["since"] = since
        return self.client.call("subscribe", params)

    def test_hello_says_who_answers(self):
        """`hello` gives the API version, the pid and the link"""
        hello = self.client.call("hello", {"client": "tests"})
        self.assertEqual(hello["pid"], self.maxicom.process.pid)
        self.assertEqual(hello["link"], self.device.address)
        self.assertEqual(hello["api"], 1)

    def test_query_and_its_refusals(self):
        """A getter's value as JSON; a command that isn't a getter refused with its code"""
        status = self.client.query("status")
        self.assertEqual((status["link"], status["state"]), (self.device.address, "open"))
        with self.assertRaises(RpcError) as refused:
            self.client.query("toggle-local-echo")
        self.assertEqual(refused.exception.code, 110)

    def test_bytes_received_come_by_rx_and_raw_since(self):
        """Bytes from the device: an `rx` notification with the stream's end, then
        `raw-since` gives them, base64 in JSON"""
        start = self.subscribe("rx")["rx"]
        self.device.send(b"hello\r\n")
        newest = self.client.wait_for("rx", lambda params: params["newest"] >= start + 7)
        raw = self.client.query(f"raw-since {start}")
        self.assertEqual(base64.b64decode(raw["bytes"]), b"hello\r\n")
        self.assertEqual(raw["end"], newest["newest"])

    def test_backlog_since_the_start(self):
        """`subscribe` since 0 gives the events kept from the start: the link's open first"""
        reply = self.subscribe("link-opened", since=0)
        self.assertGreaterEqual(reply["last"], 1)
        opened = self.client.event("link-opened")
        self.assertEqual(opened["label"], self.device.address)
        self.assertEqual(opened["how"], "connected")

    def test_keys_typed_are_sent_and_counted(self):
        """A key typed in terminal mode reaches the device, a `tx` notification counts it"""
        sent = self.subscribe("tx")["tx"]
        self.maxicom.type(b"x")
        until(lambda: self.device.received() == b"x", "the key at the device")
        self.assertEqual(self.client.wait_for("tx")["sent"], sent + 1)

    def test_settings_changed_by_key_and_command_line(self):
        """A setting toggled by a key is the user's input; set on the command line, its cause
        is the command line. Each with the value `get` gives"""
        self.subscribe("setting-changed")
        self.maxicom.type(CTRL_A + b"e")
        echo = self.client.event("setting-changed")
        self.assertEqual((echo["setting"], echo["value"]), ("local-echo", True))
        self.assertEqual(echo["cause"], {"kind": "input"})
        self.maxicom.type(CTRL_A + b":")
        self.maxicom.type(b"encoding latin1\r")
        encoding = self.client.event("setting-changed")
        self.assertEqual((encoding["setting"], encoding["value"]), ("encoding", "latin1"))
        self.assertEqual(encoding["cause"], {"kind": "command-line"})
        self.assertEqual(self.client.query("get encoding"), "latin1")

    def test_link_lost_when_the_device_hangs_up(self):
        """The device closing its end: `link-lost`"""
        self.subscribe("link-lost")
        self.device.hang_up()
        lost = self.client.event("link-lost")
        self.assertTrue(lost["link"])

    def test_unsubscribe_one_kind(self):
        """Unsubscribed from one kind, the others stay"""
        self.subscribe("link-lost", "tx")
        left = self.client.call("unsubscribe", {"events": ["tx"]})
        self.assertEqual(left["events"], ["link-lost"])
        self.maxicom.type(b"y")
        until(lambda: self.device.received() == b"y", "the key at the device")
        self.device.hang_up()
        self.client.event("link-lost")
        self.assertEqual([n["method"] for n in self.client.taken()], [])


if __name__ == "__main__":
    unittest.main()
