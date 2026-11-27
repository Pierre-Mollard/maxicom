"""Integration tests of `maxicom cli` (MAX-285): the real binary as a client of a real maxicom
on a pty, against a TCP device. Linux and macOS; run by hand, from the repository:

    python3 -m unittest discover -s tools/api -v
"""

import json
import os
import signal
import subprocess
import tempfile
import threading
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


def cli_bytes(env, *args):
    """`cli` with stdout and stderr as bytes"""
    done = subprocess.run([str(binary()), "cli", *args], env=env, capture_output=True, timeout=10)
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

    def test_socket_given(self):
        """`--socket` or `MAXICOM_SOCKET` reach a maxicom at that path, outside the socket
        folder (a link stands in for a forwarded socket), with no discovery; nothing there
        exits 3 naming the path. `list --json` gives each one's socket"""
        code, out, _ = cli(self.maxicom.env, "--json", "list")
        location = json.loads(out)[0]["location"]
        with tempfile.TemporaryDirectory() as elsewhere:
            forwarded = os.path.join(elsewhere, "bench.sock")
            os.symlink(location, forwarded)
            env = dict(self.maxicom.env, XDG_RUNTIME_DIR=elsewhere)
            code, out, _ = cli(env, "--socket", forwarded, "status")
            self.assertEqual(code, 0)
            self.assertIn(f"link: {self.device.address}\n", out)
            code, out, _ = cli(dict(env, MAXICOM_SOCKET=forwarded), "--json", "list")
            hellos = json.loads(out)
            self.assertEqual([hello["pid"] for hello in hellos], [self.maxicom.process.pid])
            self.assertEqual(hellos[0]["location"], forwarded)
            gone = os.path.join(elsewhere, "gone.sock")
            code, _, err = cli(env, "--socket", gone, "status")
            self.assertEqual((code, err), (3, f"maxicom cli: no maxicom answers at {gone}\n"))

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

    def follow(self, *args):
        """`maxicom cli ARGS` left running, its stdout read line by line; killed at the end
        of the test, its pipes closed"""
        process = subprocess.Popen(
            [str(binary()), "cli", *args],
            env=self.maxicom.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(lambda: (process.kill(), process.communicate()))
        return process

    def toggle_until(self, done, what):
        """Local echo toggled, a `setting-changed` event each time, until `done()`"""

        def check():
            if done():
                return True
            self.client.run("toggle-local-echo")
            return False

        until(check, what)

    def first_line(self, process):
        """The first line `process` prints, events made until it does: its subscription
        starts some time after the process"""
        lines = []
        reader = threading.Thread(target=lambda: lines.append(process.stdout.readline()))
        reader.start()
        self.toggle_until(lambda: lines, "a first event")
        reader.join(5)
        return lines[0]

    def test_events_until_maxicom_quits(self):
        """`events KIND` prints each event of that kind as a line of JSON, in order, until
        maxicom quits (exit 6)"""
        following = self.follow("events", "setting-changed")
        first = json.loads(self.first_line(following))
        self.assertEqual((first["kind"], first["setting"]), ("setting-changed", "local-echo"))
        self.client.run("toggle-local-echo")
        second = json.loads(following.stdout.readline())
        self.assertEqual(second["seq"], first["seq"] + 1)
        self.assertNotEqual(second["value"], first["value"])
        self.client.close()
        self.maxicom.stop()
        self.assertEqual(following.wait(5), 6)
        self.assertEqual(following.stderr.read(), "maxicom cli: maxicom quit\n")

    def test_events_reader_gone(self):
        """An unknown kind is a usage error (2); a reader gone ends `events` quietly (0)"""
        code, _, err = cli(self.maxicom.env, "events", "nope")
        self.assertEqual(code, 2)
        self.assertIn("nope", err)
        following = self.follow("events")
        self.first_line(following)
        following.stdout.close()
        self.toggle_until(lambda: following.poll() is not None, "events ended")
        self.assertEqual(following.returncode, 0)
        self.assertEqual(following.stderr.read(), "")

    def received(self, count):
        """Waits for maxicom to have received `count` bytes"""
        until(lambda: self.client.query("status")["rx"] >= count, f"{count} bytes received")

    def test_rx_polls_by_offset(self):
        """`rx` writes the bytes received as they came, the next offset on stderr; `--since`
        that offset gets only the new ones; `--json` the `raw-since` value; `--hex` a dump"""
        self.device.send(b"boot\r\n\x00\xff")
        self.received(8)
        code, out, err = cli_bytes(self.maxicom.env, "rx")
        self.assertEqual((code, out), (0, b"boot\r\n\x00\xff"))
        self.assertEqual(err, b"maxicom cli: next offset 8\n")
        self.device.send(b"ok\n")
        self.received(11)
        self.assertEqual(cli_bytes(self.maxicom.env, "rx", "--since", "8")[1], b"ok\n")
        self.assertEqual(cli_bytes(self.maxicom.env, "rx", "--since", "11")[1], b"")
        code, out, _ = cli(self.maxicom.env, "--json", "rx", "--since", "11")
        page = json.loads(out)
        self.assertEqual((code, page["start"], page["newest"], page["bytes"]), (0, 11, 11, ""))
        code, out, _ = cli(self.maxicom.env, "rx", "--hex", "--since", "8")
        self.assertEqual(out.split(), ["8", "6f", "6b", "0a", "ok."])

    def test_rx_follows_until_maxicom_quits(self):
        """`rx --follow` writes the bytes kept, then each one as it comes, until maxicom quits
        (exit 6)"""
        self.device.send(b"first ")
        self.received(6)
        following = subprocess.Popen(
            [str(binary()), "cli", "rx", "--follow"],
            env=self.maxicom.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        with following:
            self.assertEqual(following.stdout.read(6), b"first ")
            self.device.send(b"second")
            self.assertEqual(following.stdout.read(6), b"second")
            self.client.close()
            self.maxicom.stop()
            self.assertEqual(following.wait(5), 6)

    def test_ctrl_c_cancels_the_job(self):
        """Ctrl-C on a waiting client closes its connection: maxicom cancels its job"""
        waiting = subprocess.Popen(
            [str(binary()), "cli", "sleep 10000; status"],
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
