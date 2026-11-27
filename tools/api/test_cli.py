"""Integration tests of `maxicom cli` (MAX-285): the real binary as a client of a real maxicom
on a pty, against a TCP device. Linux and macOS; run by hand, from the repository:

    python3 -m unittest discover -s tools/api -v
"""

import json
import os
import signal
import subprocess
import tempfile
import unittest

from harness import Device, Maxicom, binary, build, until


def setUpModule():
    build()


def cli(env, *args):
    """`maxicom cli ARGS` in `env`: its exit code, stdout and stderr"""
    done = subprocess.run(
        [str(binary()), "cli", *args], env=env, capture_output=True, text=True, timeout=10
    )
    return done.returncode, done.stdout, done.stderr


class CliTest(unittest.TestCase):
    def setUp(self):
        self.device = Device()
        self.maxicom = Maxicom(self.device.address)
        self.client = self.maxicom.client()
        until(self.device.connected, "maxicom connected to the device")

    def tearDown(self):
        self.client.close()
        self.maxicom.stop()
        self.device.hang_up()

    def test_line_prints_its_values(self):
        """A command line's values as the TUI shows them, or as JSON with --json"""
        code, out, err = cli(self.maxicom.env, "status")
        self.assertEqual((code, err), (0, ""))
        self.assertIn("state: open\n", out)
        code, out, _ = cli(self.maxicom.env, "--json", "status")
        self.assertEqual(code, 0)
        value = json.loads(out)["values"][0]
        self.assertEqual((value["query"], value["value"]["state"]), ("status", "open"))

    def test_exit_codes(self):
        """A failed command exits 1, a refused one 4 (device access is off by default), the
        error on stderr; with --json the error on stdout"""
        code, out, err = cli(self.maxicom.env, "notify", "info", "")
        self.assertEqual((code, out), (1, ""))
        self.assertEqual(err, "maxicom cli: notify: nothing to say\n")
        self.assertEqual(cli(self.maxicom.env, "nope")[0], 1)
        code, _, err = cli(self.maxicom.env, "send", "hi")
        self.assertEqual(code, 4)
        self.assertIn("device access is off", err)
        code, out, _ = cli(self.maxicom.env, "--json", "send", "hi")
        self.assertEqual((code, json.loads(out)["code"]), (4, 111))
        self.assertEqual(self.device.received(), b"")

    def test_no_instance(self):
        """No maxicom in the socket folder exits 3; so does an --instance naming none"""
        with tempfile.TemporaryDirectory() as empty:
            env = dict(self.maxicom.env, XDG_RUNTIME_DIR=empty)
            code, _, err = cli(env, "status")
            self.assertEqual(code, 3)
            self.assertIn("no maxicom is running", err)
        code, _, err = cli(self.maxicom.env, "--instance", "nope", "status")
        self.assertEqual(code, 3)
        self.assertIn(str(self.maxicom.process.pid), err)

    def test_instance_among_several(self):
        """Two maxicoms: none picked without --instance (both listed), one by its pid or by its
        address"""
        with Device() as other_device, Maxicom(other_device.address, beside=self.maxicom) as other:
            other.client().close()
            code, _, err = cli(self.maxicom.env, "status")
            self.assertEqual(code, 3)
            self.assertIn(str(other.process.pid), err)
            self.assertIn(str(self.maxicom.process.pid), err)
            for instance in [str(other.process.pid), other_device.address]:
                code, out, _ = cli(self.maxicom.env, "--instance", instance, "status")
                self.assertEqual(code, 0)
                self.assertIn(f"link: {other_device.address}\n", out)

    def test_list(self):
        """`list` gives the maxicoms running as a table or as their hellos in JSON, the one
        `--instance` names only when given; none is an empty table, no error"""
        pid = str(self.maxicom.process.pid)
        with Device() as other_device, Maxicom(other_device.address, beside=self.maxicom) as other:
            other.client().close()
            code, out, err = cli(self.maxicom.env, "list")
            self.assertEqual((code, err), (0, ""))
            lines = out.splitlines()
            self.assertEqual(lines[0].split(), ["PID", "LINK", "SINCE"])
            pids = sorted([pid, str(other.process.pid)], key=int)
            self.assertEqual([line.split()[0] for line in lines[1:]], pids)
            self.assertIn(self.device.address, out)
            self.assertIn(other_device.address, out)
            code, out, _ = cli(self.maxicom.env, "--instance", other_device.address, "--json", "list")
            hellos = json.loads(out)
            self.assertEqual([hello["pid"] for hello in hellos], [other.process.pid])
            self.assertEqual(hellos[0]["address"], other_device.address)
        with tempfile.TemporaryDirectory() as empty:
            env = dict(self.maxicom.env, XDG_RUNTIME_DIR=empty)
            self.assertEqual(cli(env, "list"), (0, "", ""))
            self.assertEqual(cli(env, "--json", "list"), (0, "[]\n", ""))

    def test_ctrl_c_cancels_the_job(self):
        """Ctrl-C on a waiting client closes its connection: maxicom cancels its job"""
        waiting = subprocess.Popen(
            [str(binary()), "cli", "sleep 10; status"],
            env=self.maxicom.env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        jobs = lambda: self.client.query("status")["jobs"]
        until(lambda: jobs(), "the client's job running")
        os.kill(waiting.pid, signal.SIGINT)
        waiting.wait(5)
        until(lambda: not jobs(), "its job cancelled")


if __name__ == "__main__":
    unittest.main()
