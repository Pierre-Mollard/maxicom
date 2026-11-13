#!/usr/bin/env python3
"""perf.py: CPU and memory of maxicom versions (and minicom), measured from outside (MAX-233).

Each program runs in a tmux server of its own at a fixed size, against a fake serial port (a
pty) the script feeds, with a throwaway HOME under target/perf/. Its CPU time and memory come
from /proc/<pid>. Every scenario runs the same way on any maxicom build and on minicom, so
versions can be compared. See README.md next to this file.
"""

import argparse
import functools
import json
import os
import platform
import pty
import random
import re
import shlex
import shutil
import statistics
import subprocess
import sys
import threading
import time
from datetime import datetime

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WORK = f"{REPO}/target/perf"  # builds, homes and run results: never committed
RESULTS = f"{REPO}/tools/perf/results"  # what --save keeps, to commit
SERVER = "maxicom-perf"  # a tmux server of its own: nothing reaches the user's sessions
SESSION = "perf"
COLS, ROWS = 200, 50
CLK_TCK = os.sysconf("SC_CLK_TCK")


# ---- tmux, only ever on the SERVER server, exact targets ----

def tmux(*args, check=True):
    return subprocess.run(["tmux", "-L", SERVER, *args], capture_output=True, text=True,
                          check=check)


def pane_pid():
    out = tmux("display-message", "-p", "-t", f"={SESSION}:", "#{pane_pid}", check=False)
    return int(out.stdout.strip()) if out.returncode == 0 and out.stdout.strip() else None


def screen():
    return tmux("capture-pane", "-p", "-t", f"={SESSION}:", check=False).stdout


# ---- /proc ----

def cpu_seconds(pid):
    """User + system CPU time of `pid` so far, in seconds"""
    with open(f"/proc/{pid}/stat") as f:
        # the fields after the command name, which can hold spaces: utime and stime are the
        # 14th and 15th of the line, the 12th and 13th after it
        fields = f.read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / CLK_TCK


def memory_kb(pid):
    """VmRSS and VmHWM (the peak RSS) of `pid`, in kB"""
    values = {}
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            key, _, rest = line.partition(":")
            if key in ("VmRSS", "VmHWM"):
                values[key] = int(rest.split()[0])
    return values.get("VmRSS", 0), values.get("VmHWM", 0)


def drained(pid, quiet=0.5, timeout=120.0):
    """When `pid` last used CPU, once it has used none for `quiet` seconds: the end of the
    work data gave it, polled every 20 ms. None when it never went quiet"""
    end = time.monotonic() + timeout
    last_cpu, last_busy = cpu_seconds(pid), time.monotonic()
    while time.monotonic() < end:
        time.sleep(0.02)
        cpu, now = cpu_seconds(pid), time.monotonic()
        if cpu != last_cpu:
            last_cpu, last_busy = cpu, now
        elif now - last_busy >= quiet:
            return last_busy
    return None


def settle(pid, quiet=0.5, timeout=20.0):
    """Waits until `pid` uses (almost) no CPU for `quiet` seconds: start-up or a drain done"""
    end = time.monotonic() + timeout
    last = cpu_seconds(pid)
    while time.monotonic() < end:
        time.sleep(quiet)
        now = cpu_seconds(pid)
        if now - last <= 1 / CLK_TCK:
            return True
        last = now
    return False


# ---- the fake serial port ----

class Port:
    """A pty playing a serial port: the program opens `path`, the script writes to `master`.
    What the program sends back (keys, answers to queries) is read and dropped"""

    def __init__(self):
        self.master, self.slave = pty.openpty()
        self.path = os.ttyname(self.slave)
        threading.Thread(target=self._drain, daemon=True).start()

    def _drain(self):
        while True:
            try:
                os.read(self.master, 65536)
            except OSError:
                time.sleep(0.05)

    def write(self, data):
        view = memoryview(data)
        while view:
            view = view[os.write(self.master, view):]

    def paced(self, data, rate, step=0.02):
        """`data` written at `rate` bytes per second, a slice every `step` seconds, on time
        whatever the writes took"""
        start = time.monotonic()
        per_step = max(1, int(rate * step))
        for i, at in enumerate(range(0, len(data), per_step)):
            delay = start + i * step - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            self.write(data[at:at + per_step])

    def flood(self, data, timeout=120.0):
        """`data` written as fast as the program reads it (a write waits while the pty is
        full); an error when it isn't all read within `timeout` seconds"""
        done = threading.Event()

        def write():
            try:
                self.write(data)
            except OSError:
                pass
            done.set()

        threading.Thread(target=write, daemon=True).start()
        if not done.wait(timeout):
            raise RuntimeError(f"the program didn't read the data within {timeout:.0f} s")

    def close(self):
        for fd in (self.master, self.slave):
            try:
                os.close(fd)
            except OSError:
                pass


# ---- what is fed ----

WORDS = ["sensor", "read", "temp", "value", "queue", "task", "wifi", "retry", "buffer",
         "packet", "timer", "flash", "ready", "update", "config", "client", "frame"]
LEVELS = ["INFO"] * 12 + ["DEBUG"] * 5 + ["WARN"] * 2 + ["ERROR"]


@functools.lru_cache(maxsize=None)
def log_lines(count, seed=1, width=80):
    """`count` log lines of about `width` chars, CRLF ended: a time, a level, a module, words
    and numbers, now and then a level word the default highlights light"""
    rng = random.Random(seed)
    out = []
    for i in range(count):
        level = rng.choice(LEVELS)
        words = " ".join(rng.choice(WORDS) for _ in range(max(6, width // 12)))
        line = f"[{i * 7 % 86400000:>9}] {level:<5} {rng.choice(WORDS)}: {words}"
        line += f" n={rng.randrange(100000)} t={rng.uniform(-40, 125):.2f}"
        out.append(line[:width])
    return ("\r\n".join(out) + "\r\n").encode()


COLORS = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96]


@functools.lru_cache(maxsize=None)
def ansi_lines(count, seed=2):
    """Log lines with each word in its own color, a tenth of them bold: a colored log"""
    rng = random.Random(seed)
    out = []
    for line in log_lines(count).decode().split("\r\n")[:-1]:
        words = (f"\x1b[{'1;' if rng.random() < 0.1 else ''}{rng.choice(COLORS)}m{word}\x1b[0m"
                 for word in line.split(" "))
        out.append(" ".join(words))
    return ("\r\n".join(out) + "\r\n").encode()


@functools.lru_cache(maxsize=None)
def long_lines(count, width=600, seed=3):
    """Lines of `width` chars, each wrapping over several rows: a dump, a long JSON"""
    rng = random.Random(seed)
    out = []
    for i in range(count):
        line = f"[{i:>7}] DATA"
        while len(line) < width:
            line += f" {rng.choice(WORDS)}={rng.randrange(1000)}"
        out.append(line[:width])
    return ("\r\n".join(out) + "\r\n").encode()


def top_frame(i, cols=150, rows=40):
    """Frame `i` of a full-screen program (top, htop): every row rewritten in place with the
    cursor, colors and numbers changing. Smaller than the window, so it fits any program's
    screen"""
    rng = random.Random(i)
    parts = ["\x1b[H"]
    for row in range(1, rows + 1):
        text = (f"{rng.randrange(99999):>6} user  20   0 {rng.randrange(10**6):>8} "
                f"{rng.uniform(0, 100):5.1f} {rng.uniform(0, 10):4.1f} {rng.choice(WORDS)}")
        color = COLORS[(row + i) % len(COLORS)]
        parts.append(f"\x1b[{row};1H\x1b[{color}m{text[:cols].ljust(cols)}\x1b[0m")
    return "".join(parts).encode()


def many_highlights(count=300):
    """A highlights.toml of `count` words in the form every version reads (`word = "Color"`):
    the words the log lines are made of, which light up all the time, then words that never
    come"""
    colors = ["Red", "Green", "Yellow", "Blue", "Magenta", "Cyan"]
    words = WORDS + [f"token{i:03}" for i in range(count - len(WORDS))]
    return "".join(f'{word} = "{colors[i % len(colors)]}"\n' for i, word in enumerate(words))


# ---- programs ----

class Program:
    """A program to measure: a maxicom build or minicom. `save_as` names its file in
    results/, none when it can't be told apart later (the working tree)"""

    def __init__(self, name, binary, kind="maxicom", version="", save_as=None):
        self.name, self.binary, self.kind, self.version = name, binary, kind, version
        self.save_as = save_as

    def describe(self):
        return {"kind": self.kind, "binary": self.binary, "version": self.version}


def git(*args):
    return subprocess.run(["git", "-C", REPO, *args], capture_output=True, text=True,
                          check=True).stdout.strip()


def build_rev(rev):
    """maxicom at `rev`, exported with `git archive` into target/perf/build/<sha>/ and built
    there in release, once: the index and the working tree are never touched"""
    sha = git("rev-parse", "--verify", f"{rev}^{{commit}}")
    folder = f"{WORK}/build/{sha[:12]}"
    binary = f"{folder}/target/release/maxicom"
    if not os.path.exists(binary):
        print(f"building {rev} ({sha[:12]}), once...", flush=True)
        shutil.rmtree(folder, ignore_errors=True)
        os.makedirs(folder)
        archive = subprocess.Popen(["git", "-C", REPO, "archive", "--format=tar", sha],
                                   stdout=subprocess.PIPE)
        subprocess.run(["tar", "-x", "-C", folder], stdin=archive.stdout, check=True)
        if archive.wait() != 0:
            sys.exit(f"git archive {rev} failed")
        subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=folder,
                       check=True)
    described = git("describe", "--tags", "--always", sha)
    return Program(rev, binary, version=f"{described} ({sha[:12]})", save_as=described)


def build_wip():
    """maxicom as the working tree has it, built in release in the repo's own target/"""
    print("building the working tree...", flush=True)
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=REPO, check=True)
    dirty = git("status", "--porcelain")
    described = git("describe", "--tags", "--always")
    version = f"{described}{' + changes' if dirty else ''}"
    return Program("wip", f"{REPO}/target/release/maxicom", version=version)


def minicom():
    path = shutil.which("minicom")
    if not path:
        sys.exit("minicom isn't installed")
    out = subprocess.run([path, "--version"], capture_output=True, text=True).stdout
    version = out.split("\n")[0].strip()
    number = re.search(r"\d+(\.\d+)+", version)
    return Program("minicom", path, kind="minicom", version=version,
                   save_as=f"minicom-{number.group() if number else 'unknown'}")


# ---- one run: a program, a scenario ----

def set_key(toml, section, key, value):
    """`key = value` in `[section]` (None: the top level), replaced or added"""
    lines = toml.split("\n")
    start = 0
    if section:
        if f"[{section}]" not in lines:
            lines += ["", f"[{section}]"]
        start = lines.index(f"[{section}]") + 1
    end = next((i for i in range(start, len(lines)) if lines[i].startswith("[")), len(lines))
    for i in range(start, end):
        if re.match(rf"{re.escape(key)}\s*=", lines[i]):
            lines[i] = f"{key} = {value}"
            return "\n".join(lines)
    lines.insert(start, f"{key} = {value}")
    return "\n".join(lines)


def prepare_home(program, home, config, files):
    """A fresh HOME for `program`: maxicom's own default files, then `config`'s keys
    ({(section, key): value}) and `files` ({name: content}) in its config folder; minicom
    gets an empty one"""
    if not home.startswith(f"{WORK}/"):
        sys.exit(f"refusing: {home} isn't under target/perf/")  # it becomes HOME
    shutil.rmtree(home, ignore_errors=True)
    os.makedirs(home)
    env = {k: v for k, v in os.environ.items() if not k.startswith("XDG_")}
    env["HOME"] = home
    if program.kind == "maxicom":
        # the first start writes the default files
        subprocess.run([program.binary, "--cache-dir"], env=env, capture_output=True,
                       check=True)
        path = f"{home}/.config/maxicom/config.toml"
        toml = open(path).read()
        toml = set_key(toml, "capture", "dir", '"~/captures"')
        for (section, key), value in config.items():
            toml = set_key(toml, section, key, value)
        open(path, "w").write(toml)
        for name, content in files.items():
            open(f"{home}/.config/maxicom/{name}", "w").write(content)
    return home


class Run:
    """A program started in tmux on its own port: what a scenario measures and drives"""

    def __init__(self, program, home, port):
        self.program, self.home, self.port = program, home, port
        self.pid = None

    def start(self):
        if self.program.kind == "maxicom":
            argv = [self.program.binary, "-D", self.port.path, "--no-restore"]
        else:
            argv = [self.program.binary, "-D", self.port.path, "-o"]
        env = (f"env -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME "
               f"HOME={shlex.quote(self.home)} TERM=tmux-256color")
        command = f"cd {shlex.quote(self.home)} && exec {env} {shlex.join(argv)}"
        tmux("kill-server", check=False)
        tmux("new-session", "-d", "-s", SESSION, "-x", str(COLS), "-y", str(ROWS), command)
        for _ in range(50):
            pid = pane_pid()
            if pid and os.path.exists(f"/proc/{pid}"):
                self.pid = pid
                break
            time.sleep(0.1)
        if not self.pid:
            raise RuntimeError("the program didn't start")
        time.sleep(1.0)
        if not settle(self.pid):
            raise RuntimeError("the program never went quiet after starting")

    def alive(self):
        return self.pid is not None and os.path.exists(f"/proc/{self.pid}")

    def cpu(self):
        return cpu_seconds(self.pid)

    def memory(self):
        return memory_kb(self.pid)

    def stop(self):
        tmux("kill-server", check=False)


# ---- scenarios ----
# A scenario's function gets the started Run and returns its metrics, {name: value}, the
# unit in the name. It raises NotAvailable when the program doesn't have what it measures

class NotAvailable(Exception):
    """The program doesn't have what the scenario measures: n/a"""


def require(pattern, what):
    """NotAvailable unless the screen shows `pattern`: the scenario's setup took"""
    if not re.search(pattern, screen()):
        raise NotAvailable(f"no {what} on screen")


def idle(run, seconds=10.0):
    before = run.cpu()
    time.sleep(seconds)
    rss, _ = run.memory()
    return {"cpu_%": (run.cpu() - before) / seconds * 100, "rss_kb": rss}


def steady(run, seconds=10.0, rate=11520):
    data = log_lines(int(rate * seconds / 50))[:int(rate * seconds)]
    before, start = run.cpu(), time.monotonic()
    run.port.paced(data, rate)
    wall = time.monotonic() - start
    used = run.cpu() - before
    rss, _ = run.memory()
    return {"cpu_%": used / wall * 100, "rss_kb": rss}


def flood(run, data, check=None):
    """`data` written as fast as the program reads it: how fast it takes it in, drawing
    included, until it's quiet again, and the CPU it takes per MB. `check` runs at the end"""
    before, start = run.cpu(), time.monotonic()
    run.port.flood(data)
    end = drained(run.pid)
    if end is None:
        raise RuntimeError("the program never went quiet after the flood")
    used = run.cpu() - before
    if check:
        check()
    rss, peak = run.memory()
    size = len(data) / 1_000_000
    return {"mb_per_s": size / (end - start), "cpu_s_per_mb": used / size,
            "rss_kb": rss, "peak_kb": peak}


def memory(run, lines=250_000):
    start, _ = run.memory()
    run.port.flood(log_lines(lines), timeout=300.0)
    if drained(run.pid, timeout=300.0) is None:
        raise RuntimeError("the program never went quiet after the lines")
    rss, peak = run.memory()
    return {"rss_start_kb": start, "rss_kb": rss, "peak_kb": peak}


def redraw(run, seconds=10.0, fps=10):
    """`fps` full-screen frames a second for `seconds`, each written at its time"""
    frames = [top_frame(i) for i in range(int(seconds * fps))]
    before, start = run.cpu(), time.monotonic()
    for i, frame in enumerate(frames):
        delay = start + i / fps - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        run.port.write(frame)
    wall = time.monotonic() - start
    used = run.cpu() - before
    rss, _ = run.memory()
    return {"cpu_%": used / wall * 100, "rss_kb": rss}


def resize(run, lines=10_000, times=6):
    """`lines` lines of 160 chars, then the window narrowed and widened back, `times` resizes:
    each one lays the whole history out again to count its rows"""
    run.port.flood(log_lines(lines, width=160))
    if drained(run.pid) is None:
        raise RuntimeError("the program never went quiet after the lines")
    used = 0.0
    for i in range(times):
        width = COLS - 50 if i % 2 == 0 else COLS
        before = run.cpu()
        tmux("resize-window", "-t", f"={SESSION}:", "-x", str(width), "-y", str(ROWS))
        if drained(run.pid, timeout=60.0) is None:
            raise RuntimeError("the program never went quiet after a resize")
        used += run.cpu() - before
    return {"cpu_ms_per_resize": used / times * 1000}


def search(run, lines=100_000, query="ready n=4"):
    """`lines` log lines, then a search typed one key at a time: each key searches the whole
    history again. The CPU each key takes, until the program is quiet again"""
    run.port.flood(log_lines(lines), timeout=300.0)
    if drained(run.pid, timeout=300.0) is None:
        raise RuntimeError("the program never went quiet after the lines")
    for key in ("C-a", "/"):
        tmux("send-keys", "-t", f"={SESSION}:", key)
        time.sleep(0.2)
    drained(run.pid)
    costs = []
    for char in query:
        before = run.cpu()
        tmux("send-keys", "-t", f"={SESSION}:", "-l", char)
        if drained(run.pid, quiet=0.3, timeout=60.0) is None:
            raise RuntimeError("the program never went quiet after a key")
        costs.append(run.cpu() - before)
    # the query, then its count on the same row: ` q (3/12)`, `/q ─── (3/12)`
    require(re.escape(query) + r"[^\n]*\(\d+/\d+\)", "search bar with the query's matches")
    return {"cpu_ms_per_key": statistics.mean(costs) * 1000,
            "cpu_ms_max_key": max(costs) * 1000}


class Scenario:
    """A scenario: the programs it runs on, what it measures, the config it needs"""

    def __init__(self, name, kinds, measure, about, config=None, files=None):
        self.name, self.kinds, self.measure, self.about = name, kinds, measure, about
        self.config, self.files = config or {}, files or {}


MAXICOM = {"maxicom"}
BOTH = {"maxicom", "minicom"}
LINE = {(None, "mode"): '"line"'}
TERMINAL = {(None, "mode"): '"terminal"'}
# by default both modes take in what is received; this one keeps line mode's history alone
LINE_ONLY = {**LINE, (None, "dual_mode_buffering"): '"active_only"'}
HEX = {**LINE, ("line_mode", "display_mode"): '"hex"'}
# a hex row: bytes as two digits, spaced
HEX_ROW = r"\b[0-9A-F]{2} [0-9A-F]{2} [0-9A-F]{2} [0-9A-F]{2}\b"
RAW_PANEL = {("raw_side", "view"): '"hex"'}
# the default plot: every key=value pair, here the lines' n= and t=
PLOT = {(None, "plot"): '"key-value"'}
HIGHLIGHTS = {"highlights.toml": many_highlights()}


def flood_10mb(check=None):
    return lambda run: flood(run, log_lines(140_000)[:10_000_000], check)

SCENARIOS = [
    Scenario("idle-line", MAXICOM, idle, "nothing received for 10 s: CPU, RSS", LINE),
    Scenario("idle-terminal", BOTH, idle, "nothing received for 10 s: CPU, RSS", TERMINAL),
    Scenario("steady-line", MAXICOM, steady, "log lines at 115200 baud for 10 s: CPU, RSS",
             LINE),
    Scenario("steady-terminal", BOTH, steady, "log lines at 115200 baud for 10 s: CPU, RSS",
             TERMINAL),
    Scenario("flood-line", MAXICOM, flood_10mb(),
             "10 MB of log lines as fast as read: MB/s, CPU s per MB, memory", LINE),
    Scenario("flood-terminal", BOTH, flood_10mb(),
             "10 MB of log lines as fast as read: MB/s, CPU s per MB, memory", TERMINAL),
    Scenario("memory-line", MAXICOM, memory,
             "250k log lines, both histories full: RSS at start, after, peak", LINE),
    Scenario("memory-line-only", MAXICOM, memory,
             "the same, line mode's history alone (active_only)", LINE_ONLY),
    Scenario("memory-terminal", BOTH, memory,
             "250k log lines, both histories full: RSS at start, after, peak", TERMINAL),
    Scenario("ansi-line", MAXICOM, lambda run: flood(run, ansi_lines(40_000)),
             "log lines with every word colored, as fast as read: MB/s, CPU s per MB", LINE),
    Scenario("ansi-terminal", BOTH, lambda run: flood(run, ansi_lines(40_000)),
             "log lines with every word colored, as fast as read: MB/s, CPU s per MB",
             TERMINAL),
    Scenario("long-line", MAXICOM, lambda run: flood(run, long_lines(8_000)),
             "600 char lines that wrap, as fast as read: MB/s, CPU s per MB", LINE),
    Scenario("long-terminal", BOTH, lambda run: flood(run, long_lines(8_000)),
             "600 char lines that wrap, as fast as read: MB/s, CPU s per MB", TERMINAL),
    Scenario("hex-line", MAXICOM,
             lambda run: flood(run, log_lines(30_000), lambda: require(HEX_ROW, "hex rows")),
             "log lines shown in Hex, as fast as read: MB/s, CPU s per MB", HEX),
    Scenario("redraw-terminal", BOTH, redraw,
             "a full-screen program (top) redrawing 10 times a second for 10 s: CPU", TERMINAL),
    Scenario("resize-line", MAXICOM, resize,
             "10k lines of 160 chars, the window resized 6 times: CPU per resize", LINE),
    Scenario("resize-line-plain", MAXICOM, resize,
             "the same without highlights (an empty highlights.toml)", LINE,
             {"highlights.toml": ""}),
    Scenario("search-line", MAXICOM, search,
             "100k lines, a 9 key search typed: CPU ms per key", LINE),
    Scenario("search-terminal", MAXICOM, search,
             "100k lines, a 9 key search typed: CPU ms per key", TERMINAL),
    Scenario("highlights-line", MAXICOM, flood_10mb(),
             "flood-line with 300 highlight words, 17 of them in every line", LINE, HIGHLIGHTS),
    Scenario("highlights-terminal", MAXICOM, flood_10mb(),
             "flood-terminal with 300 highlight words, 17 of them in every line", TERMINAL,
             HIGHLIGHTS),
    Scenario("raw-line", MAXICOM, flood_10mb(lambda: require(HEX_ROW, "raw panel")),
             "flood-line with the raw side panel in Hex", {**LINE, **RAW_PANEL}),
    Scenario("raw-terminal", MAXICOM, flood_10mb(lambda: require(HEX_ROW, "raw panel")),
             "flood-terminal with the raw side panel in Hex", {**TERMINAL, **RAW_PANEL}),
    Scenario("plot-line", MAXICOM, flood_10mb(lambda: require(r"key-value", "plot")),
             "flood-line with the key-value plot charting n= and t=", {**LINE, **PLOT}),
    Scenario("plot-terminal", MAXICOM, flood_10mb(lambda: require(r"key-value", "plot")),
             "flood-terminal with the key-value plot charting n= and t=",
             {**TERMINAL, **PLOT}),
]


def run_once(program, scenario):
    home = prepare_home(program, f"{WORK}/homes/{program.name}/{scenario.name}",
                        scenario.config, scenario.files)
    port = Port()
    run = Run(program, home, port)
    try:
        run.start()
        metrics = scenario.measure(run)
        if not run.alive():
            raise RuntimeError("the program exited during the scenario")
        return metrics
    finally:
        run.stop()
        port.close()


# ---- the report ----

def machine():
    model = ""
    try:
        for line in open("/proc/cpuinfo"):
            if line.startswith("model name"):
                model = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    release = platform.release()
    return {"cpu": model, "cores": os.cpu_count(), "kernel": release,
            "wsl": "microsoft" in release.lower(), "python": platform.python_version(),
            "tmux": tmux("-V", check=False).stdout.strip(), "size": f"{COLS}x{ROWS}"}


def summary(values):
    """The median and the spread (max - min) of a metric's repeats"""
    return statistics.median(values), max(values) - min(values)


def number(value):
    if abs(value) >= 100:
        return f"{value:,.0f}"
    if abs(value) >= 10:
        return f"{value:.1f}"
    return f"{value:.2f}"


def table(results, programs):
    """A table per scenario: a metric per row, a program per column, the median ± spread,
    then the change from the first program that has the scenario"""
    lines = []
    names = [p.name for p in programs]
    for scenario, by_program in results.items():
        lines += ["", f"## {scenario}", ""]
        lines.append("| metric | " + " | ".join(names) + " |")
        lines.append("| --- |" + " --- |" * len(names))
        metrics = []
        for values in by_program.values():
            metrics += [m for m in values if m not in metrics]
        for metric in metrics:
            cells, base = [], None
            for name in names:
                values = by_program.get(name, {}).get(metric)
                if not values:
                    cells.append("n/a")
                    continue
                median, spread = summary(values)
                cell = f"{number(median)} ±{number(spread)}"
                if base is None:
                    base = median
                elif base:
                    cell += f" ({(median - base) / base * 100:+.0f}%)"
                cells.append(cell)
            lines.append(f"| {metric} | " + " | ".join(cells) + " |")
    return "\n".join(lines)


# ---- saved results ----

def save(program, report):
    """`program`'s results into results/<save_as>.json, merged with what the file has: the
    scenarios run now replace theirs, the others stay. Its machine is now this one"""
    path = f"{RESULTS}/{program.save_as}.json"
    saved = {"results": {}, "not_available": {}}
    if os.path.exists(path):
        with open(path) as f:
            saved = json.load(f)
        if saved.get("machine", {}).get("cpu") != report["machine"]["cpu"]:
            print(f"warning: {path} was measured on another machine, merged anyway")
    saved.update({"date": report["date"], "machine": report["machine"],
                  "repeat": report["repeat"], "program": program.describe()})
    saved["program"].pop("binary", None)
    for scenario, by_program in report["results"].items():
        if program.name in by_program:
            saved["results"][scenario] = by_program[program.name]
            saved["not_available"].pop(scenario, None)
        reason = report["not_available"].get(scenario, {}).get(program.name)
        if reason:
            saved["not_available"][scenario] = reason
            saved["results"].pop(scenario, None)
    os.makedirs(RESULTS, exist_ok=True)
    with open(path, "w") as f:
        json.dump(saved, f, indent=2, sort_keys=True)
        f.write("\n")
    return path


def load(spec):
    """A saved file, by its name in results/ or its path: a column named after it"""
    path = spec if os.path.exists(spec) else f"{RESULTS}/{spec}.json"
    if not os.path.exists(path):
        sys.exit(f"--against {spec}: no {path} (see tools/perf/results/)")
    with open(path) as f:
        saved = json.load(f)
    name = f"{os.path.basename(path)[:-5]} (saved)"
    program = Program(name, None, saved["program"]["kind"], saved["program"]["version"])
    return program, saved


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--rev", action="append", default=[],
                        help="a git rev of maxicom to build and measure (repeatable)")
    parser.add_argument("--bin", action="append", default=[], metavar="NAME=PATH",
                        help="a maxicom binary to measure (repeatable)")
    parser.add_argument("--wip", action="store_true",
                        help="the working tree, built in release")
    parser.add_argument("--minicom", action="store_true", help="the installed minicom too")
    parser.add_argument("--repeat", type=int, default=3, help="runs of each (default 3)")
    parser.add_argument("--only", default="", help="scenarios to run, comma separated")
    parser.add_argument("--list", action="store_true", help="list the scenarios")
    parser.add_argument("--save", action="store_true",
                        help="keep each program's results in tools/perf/results/")
    parser.add_argument("--against", action="append", default=[], metavar="NAME",
                        help="a saved result as the first columns (repeatable)")
    args = parser.parse_args()

    if args.list:
        for scenario in SCENARIOS:
            kinds = ", ".join(sorted(scenario.kinds))
            print(f"{scenario.name:<18} {kinds:<16} {scenario.about}")
        return 0
    if not shutil.which("tmux") or not os.path.exists("/proc/self/stat"):
        sys.exit("needs Linux (/proc) and tmux")

    against = [load(spec) for spec in args.against]
    programs = [build_rev(rev) for rev in args.rev]
    for spec in args.bin:
        name, _, path = spec.partition("=")
        if not path or not os.path.exists(path):
            sys.exit(f"--bin {spec}: NAME=PATH to an existing binary")
        programs.append(Program(name, os.path.abspath(path), version="binary", save_as=name))
    if args.wip or not programs:
        programs.append(build_wip())
    if args.minicom:
        programs.append(minicom())

    only = {name for name in args.only.split(",") if name}
    unknown = only - {s.name for s in SCENARIOS}
    if unknown:
        sys.exit(f"unknown scenarios: {', '.join(sorted(unknown))} (see --list)")
    scenarios = [s for s in SCENARIOS if not only or s.name in only]

    results, missing = {}, {}
    for scenario in scenarios:
        name, kinds = scenario.name, scenario.kinds
        results[name] = {}
        # the programs in turns, so a drift of the machine spreads over all of them
        for i in range(args.repeat):
            for program in programs:
                if program.kind not in kinds or program.name in missing.get(name, {}):
                    continue
                print(f"{name} {program.name} {i + 1}/{args.repeat}...", flush=True)
                try:
                    metrics = run_once(program, scenario)
                except NotAvailable as why:
                    print(f"  n/a: {why}", flush=True)
                    missing.setdefault(name, {})[program.name] = str(why)
                    continue
                except (RuntimeError, OSError, subprocess.CalledProcessError) as err:
                    print(f"  failed: {err}", flush=True)
                    continue
                by_metric = results[name].setdefault(program.name, {})
                for metric, value in metrics.items():
                    by_metric.setdefault(metric, []).append(value)

    report = {
        "date": datetime.now().isoformat(timespec="seconds"),
        "machine": machine(),
        "repeat": args.repeat,
        "programs": {p.name: p.describe() for p in programs},
        "results": results,
        "not_available": missing,
    }
    os.makedirs(f"{WORK}/runs", exist_ok=True)
    path = f"{WORK}/runs/{datetime.now():%Y%m%d_%H%M%S}.json"
    with open(path, "w") as f:
        json.dump(report, f, indent=2)

    # the saved columns first: they are the history the run is compared with
    shown = {name: dict(by_program) for name, by_program in results.items()}
    for program, saved in against:
        if saved["machine"].get("cpu") != report["machine"]["cpu"]:
            print(f"warning: {program.name} was measured on another machine: "
                  f"{saved['machine'].get('cpu')}")
        for name in shown:
            if name in saved["results"]:
                shown[name][program.name] = saved["results"][name]
    print(table(shown, [program for program, _ in against] + programs))
    print(f"\n{path}")
    if args.save:
        for program in programs:
            if program.save_as:
                print(f"saved {save(program, report)}")
            else:
                print(f"not saved: {program.name} (the working tree can't be told apart later)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
