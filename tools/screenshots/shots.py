#!/usr/bin/env python3
"""shots.py [NAME...]: the guide's screenshots, made by driving maxicom in tmux.

Each scenario runs maxicom on fake devices (ptys fed by threads, a TCP listener) with a
throwaway HOME under target/screenshots/, on a tmux server of its own, sends keys, captures
the pane in color, then renders it: tokyonight-night colors, freeze, the bottom trimmed,
1400 px wide, into docs/guide/images/<name>.png. See README.md next to this file.
"""

import os
import pty
import random
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime, timedelta

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WORK = f"{REPO}/target/screenshots"  # every run's HOME is a folder in it, never the real one
IMAGES = f"{REPO}/docs/guide/images"
BIN = f"{REPO}/target/debug/maxicom"
SERVER = "maxicom-shots"  # a tmux server of its own: nothing reaches the user's sessions
COLS, ROWS = 100, 28
HOST = socket.gethostname()


# ---- tmux, only ever on the SERVER server, exact targets ----

def tmux(*args, check=True):
    return subprocess.run(["tmux", "-L", SERVER, *args], capture_output=True, text=True,
                          check=check)


def alive():
    return tmux("has-session", "-t", "=shots", check=False).returncode == 0


def keys(*names):
    if not alive():
        raise RuntimeError("the maxicom session is gone")
    for name in names:
        tmux("send-keys", "-t", "=shots:", name)
        time.sleep(0.25)


def text(literal):
    tmux("send-keys", "-t", "=shots:", "-l", literal)
    time.sleep(0.4)


# ---- fake devices ----

class Pty:
    """A fake serial port: its feed is written once `start` is called, what comes in is read"""

    def __init__(self, feed=(), every=0.08):
        self.master, slave = pty.openpty()
        self.path = os.ttyname(slave)
        self.slave = slave
        self.feed, self.every = list(feed), every
        self.done = threading.Event()

    def start(self):
        threading.Thread(target=self._write, daemon=True).start()
        threading.Thread(target=self._drain, daemon=True).start()

    def _write(self):
        for chunk in self.feed:
            data = chunk if isinstance(chunk, bytes) else (chunk + "\r\n").encode()
            os.write(self.master, data)
            time.sleep(self.every)
        self.done.set()

    def _drain(self):
        while True:
            try:
                os.read(self.master, 4096)
            except OSError:
                time.sleep(0.2)


def tcp_listener():
    """A port that accepts and keeps connections: a test reads it as reachable"""
    server = socket.socket()
    server.bind(("127.0.0.1", 0))
    server.listen()

    def serve():
        held = []
        while True:
            conn, _ = server.accept()
            held.append(conn)

    threading.Thread(target=serve, daemon=True).start()
    return server.getsockname()[1]


def closed_port():
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return port


# ---- feeds ----

G, Y, R, N = "\x1b[0;32m", "\x1b[0;33m", "\x1b[0;31m", "\x1b[0m"
ESP_BOOT = [
    "ets Jul 29 2019 12:21:46",
    "rst:0x1 (POWERON_RESET),boot:0x13 (SPI_FAST_FLASH_BOOT)",
    "load:0x3fff0030,len:7112",
    "entry 0x40080674",
    f"{G}I (29) boot: ESP-IDF v5.2.1 2nd stage bootloader{N}",
    f"{G}I (29) boot: compile time Oct 28 2026 14:02:11{N}",
    f"{G}I (412) cpu_start: Pro cpu start user code{N}",
    f"{G}I (418) cpu_start: cpu freq: 240000000 Hz{N}",
    f"{G}I (431) heap_init: At 3FFAE6E0 len 00001920 (6 KiB): DRAM{N}",
    f"{G}I (512) wifi:mode : sta (24:6f:28:a1:3c:90){N}",
    f"{Y}W (1523) wifi:Haven't to connect to a suitable AP now!{N}",
    f"{G}I (2104) wifi:connected with lab-iot, aid = 3, channel 6{N}",
    f"{G}I (3120) esp_netif_handler: sta ip: 192.168.1.42{N}",
    f"{G}I (3125) app: sensor ready, sampling every 1000 ms{N}",
    f"{G}I (4125) app: temp=21.4C hum=48%{N}",
    f"{R}E (5130) i2c: i2c_master_cmd_begin(1480): I2C bus timeout{N}",
    f"{Y}W (5131) app: sensor read failed, retrying{N}",
    f"{G}I (6126) app: temp=21.5C hum=47%{N}",
    f"{G}I (7126) app: temp=21.5C hum=47%{N}",
]
ESP_MORE = [f"{G}I ({8126 + 1000 * i}) app: temp=21.{5 + i % 3}C hum=4{7 - i % 2}%{N}"
            for i in range(4)]

def pid_feed(samples=300):
    """A heater's PID loop logging each step: the setpoint steps up then down, the temperature
    overshoots and settles, the output saturates then eases. Other log lines come between
    them, which the plot's `line_contains` skips"""
    rng = random.Random(7)
    temp, integral, prev = 22.0, 0.0, 0.0
    delayed = [0.0] * 8  # the heater's dead time, in steps
    lines = ESP_BOOT[:14]
    for i in range(samples):
        setpoint = 40.0 if i < 40 else 65.0 if i < 170 else 50.0
        err = setpoint - temp
        integral = max(-400.0, min(400.0, integral + err))
        out = max(0.0, min(100.0, 4.0 * err + 0.3 * integral + 2.0 * (err - prev)))
        prev = err
        delayed.append(out)
        temp += 0.03 * delayed.pop(0) - 0.035 * (temp - 22.0) + rng.gauss(0, 0.12)
        ms = 4200 + 100 * i
        lines.append(f"{G}I ({ms}) pid: setpoint={setpoint:.1f} temp={temp:.2f}C "
                     f"out={out:.0f}%{N}")
        if i % 60 == 30:
            lines.append(f"{G}I ({ms + 40}) mqtt: published heater/state (42 bytes){N}")
    return lines


PLOTS = """[pid]
separator = "="
keys = ["setpoint", "temp", "out"]
line_contains = "pid:"
window = 300
colors = { setpoint = "Yellow", temp = "LightRed" }
"""

PROMPT = "\x1b[1;32mroot@gateway\x1b[0m:\x1b[1;34m~\x1b[0m# "
BD, CY = "\x1b[1;34m", "\x1b[1;36m"
SHELL = [
    "",
    "Welcome to Buildroot 2026.02",
    "gateway login: root",
    "Password:",
    PROMPT + "uname -a",
    "Linux gateway 6.6.52 #1 SMP PREEMPT Tue Oct 28 14:02:11 CET 2026 armv7l GNU/Linux",
    PROMPT + "ip -br -c addr",
    "lo               UNKNOWN        \x1b[35m127.0.0.1\x1b[0m/8 \x1b[34m::1\x1b[0m/128",
    "\x1b[36meth0            \x1b[0m \x1b[32mUP            \x1b[0m \x1b[35m192.168.1.42\x1b[0m/24"
    " \x1b[34mfe80::2c1:41ff:fe0a:91b2\x1b[0m/64",
    "\x1b[36mwlan0           \x1b[0m \x1b[31mDOWN          \x1b[0m",
    PROMPT + "ls --color /",
    f"{CY}bin{N}  {BD}dev{N}  {BD}etc{N}  {BD}home{N}  {CY}lib{N}  {BD}media{N}  {BD}mnt{N}  "
    f"{BD}opt{N}  {BD}proc{N}  {BD}root{N}  {BD}run{N}  {CY}sbin{N}  {BD}sys{N}  "
    f"\x1b[30;42mtmp{N}  {BD}usr{N}  {BD}var{N}",
    PROMPT + "dmesg | tail -4",
    "\x1b[32m[    2.114233]\x1b[0m cp210x 1-1:1.0: cp210x converter detected",
    "\x1b[32m[    2.118410]\x1b[0m usb 1-1: cp210x converter now attached to ttyUSB0",
    "\x1b[32m[    3.402117]\x1b[0m IPv4: ADDRCONF(NETDEV_CHANGE): eth0: link becomes ready",
    "\x1b[32m[   12.990054]\x1b[0m random: crng init done",
    PROMPT + "uptime",
    " 14:22:05 up 3 days,  2:41,  1 user,  load average: 0.08, 0.03, 0.01",
    PROMPT.encode(),
]


def top_screen(cols=98, rows=25):
    """A full-screen process viewer, drawn with cursor moves, for terminal mode"""
    out = ["\x1b[?1049h\x1b[H\x1b[2J"]

    def at(row, line):
        out.append(f"\x1b[{row};1H{line}")

    def meter(label, used, kernel, total_width, right):
        inner = total_width - len(label) - 2
        bar_n = used + kernel
        shown = right.rjust(inner - bar_n) if inner - bar_n > len(right) else ""
        return (f"\x1b[36m{label}\x1b[1;37m[\x1b[0;32m{'|' * used}\x1b[31m{'|' * kernel}"
                f"\x1b[90m{shown}\x1b[1;37m]\x1b[0m")

    half = 46
    at(1, "  " + meter("0", 14, 4, half, "23.4%") + "   \x1b[36mTasks: \x1b[1;37m38\x1b[0;36m, "
       "\x1b[1;37m52\x1b[0;36m thr; \x1b[1;32m1\x1b[0;36m running\x1b[0m")
    at(2, "  " + meter("1", 6, 2, half, "9.8%") + "   \x1b[36mLoad average: \x1b[1;37m0.42 "
       "\x1b[0;37m0.31 \x1b[90m0.18\x1b[0m")
    at(3, "  " + meter("Mem", 12, 0, half, "212M/1.94G") + "   \x1b[36mUptime: "
       "\x1b[1;37m3 days, 02:41:12\x1b[0m")
    at(4, "  " + meter("Swp", 0, 0, half, "0K/0K"))
    header = "    PID USER       PRI  NI  VIRT   RES   SHR S  CPU% MEM%   TIME+  Command"
    at(6, "\x1b[30;42m" + header.ljust(cols) + "\x1b[0m")
    procs = [
        (412, "root", "S", 11.2, 2.1, "1:22.41", "/usr/bin/sensor-daemon --rate 1000"),
        (377, "mosquitto", "S", 2.0, 0.6, "0:41.07", "/usr/sbin/mosquitto -c /etc/mosquitto.conf"),
        (1022, "root", "R", 1.3, 0.2, "0:00.85", "top"),
        (298, "root", "S", 0.7, 0.3, "0:12.30", "/usr/sbin/dropbear -R"),
        (1, "root", "S", 0.0, 0.1, "0:03.12", "/sbin/init"),
        (211, "root", "S", 0.0, 0.1, "0:00.44", "/sbin/syslogd -n"),
        (214, "root", "S", 0.0, 0.0, "0:00.21", "/sbin/klogd -n"),
        (256, "root", "S", 0.0, 0.1, "0:01.77", "udhcpc -i eth0 -b"),
        (301, "root", "S", 0.0, 0.0, "0:00.09", "/usr/sbin/crond -f"),
        (318, "ntp", "S", 0.0, 0.2, "0:02.58", "/usr/sbin/ntpd -g -n"),
        (1004, "root", "S", 0.0, 0.1, "0:00.31", "-sh"),
    ]
    for i, (pid, user, state, cpu, mem, cputime, cmd) in enumerate(procs):
        line = (f"{pid:7d} {user:<9.9s}   20   0 {pid * 13 % 9000 + 2100:5d}K "
                f"{pid * 7 % 4000 + 900:5d}K {pid * 3 % 2000 + 400:5d}K {state} "
                f"{cpu:5.1f} {mem:4.1f} {cputime:>8s}  {cmd}")
        if i == 0:
            at(7 + i, "\x1b[30;46m" + line.ljust(cols) + "\x1b[0m")
        else:
            color = "\x1b[1;32m" if state == "R" else ""
            at(7 + i, f"{color}{line}\x1b[0m")
    fkeys = ["Help", "Setup", "Search", "Filter", "Tree", "SortBy", "Nice -", "Nice +",
             "Kill", "Quit"]
    bar = "".join(f"\x1b[0mF{n + 1}\x1b[30;46m{label:<6s}" for n, label in enumerate(fkeys))
    at(rows, bar + "\x1b[30;46m" + " " * 8 + "\x1b[0m")
    at(rows, "")
    return ["".join(out).encode()]


# ---- sessions written by hand, for the session browser and the restore ----

def session(cache, started, port, lines, ended=None, meta=True, baud=115200):
    """A past session folder; its end is the .raw's modified time"""
    tag = re.sub(r"[^A-Za-z0-9.-]", "_", port.rstrip("/").split("/")[-1])
    stem = f"{started:%Y%m%d_%H%M%S}{started.microsecond // 1000:03d}_{tag}"
    folder = f"{cache}/maxicom/sessions/{stem}"
    os.makedirs(folder, exist_ok=True)
    raw = "".join(line + "\r\n" for line in lines).encode()
    open(f"{folder}/{stem}.raw", "wb").write(raw)
    ms = int(started.timestamp() * 1000)
    open(f"{folder}/{stem}.times", "w").write(f"0 {ms}\n")
    plain = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in lines]
    stamp = started.strftime("[%H:%M:%S.%f")[:-3] + "]"
    open(f"{folder}/{stem}.log", "w").write("".join(f"{stamp} {line}\n" for line in plain))
    if meta:
        open(f"{folder}/{stem}.meta", "w").write(
            f'version = 1\nstarted = {int(started.timestamp())}\n\n[device]\nhost = "{HOST}"\n'
            f'port = "{port}"\n'
            + ("" if port.startswith("tcp://") else
               f'baud = {baud}\nchar_size = "bits8"\nparity = "none"\nstop_bits = "one"\n')
            + 'encoding = "utf8"\n\n[text]\n'
            f'timestamp = "[%H:%M:%S%.3f]"\ntx_mark = "> "\n\n[terminal]\nrows = 25\ncols = 98\n')
    end = (ended or started + timedelta(minutes=40)).timestamp()
    os.utime(f"{folder}/{stem}.raw", (end, end))


# ---- config ----

def set_key(toml, section, key, value):
    """`key = value` in `[section]` (None: the top level), replaced or added"""
    lines = toml.split("\n")
    start = 0
    if section:
        start = lines.index(f"[{section}]") + 1
    end = next((i for i in range(start, len(lines)) if lines[i].startswith("[")), len(lines))
    for i in range(start, end):
        if re.match(rf"{re.escape(key)}\s*=", lines[i]):
            lines[i] = f"{key} = {value}"
            return "\n".join(lines)
    lines.insert(start, f"{key} = {value}")
    return "\n".join(lines)


def prepare(run, config, links, triggers, plots=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith("XDG_")}
    env["HOME"] = run
    # the first start writes the default files
    subprocess.run([BIN, "--cache-dir"], env=env, capture_output=True, check=True)
    path = f"{run}/.config/maxicom/config.toml"
    toml = open(path).read()
    toml = set_key(toml, "capture", "dir", '"~/captures"')  # HOME is the run folder
    toml = set_key(toml, "screenshot", "dir", '"~/captures"')
    for (section, key), value in config.items():
        toml = set_key(toml, section, key, value)
    open(path, "w").write(toml)
    if links:
        open(f"{run}/.config/maxicom/links.toml", "w").write(links)
    if triggers:
        open(f"{run}/.config/maxicom/triggers.toml", "w").write(triggers)
    if plots:
        open(f"{run}/.config/maxicom/plots.toml", "w").write(plots)
    return env


# ---- render ----

PALETTE = ["15161e", "f7768e", "9ece6a", "e0af68", "7aa2f7", "bb9af7", "7dcfff", "a9b1d6",
           "414868", "ff899d", "9fe044", "faba4a", "8db0ff", "c7a9ff", "a4daff", "c0caf5"]
FG = ["38", "2", "192", "202", "245"]
BG = ["48", "2", "26", "27", "38"]


def rgb(n, ground):
    h = PALETTE[n]
    return [ground, "2", str(int(h[0:2], 16)), str(int(h[2:4], 16)), str(int(h[4:6], 16))]


def state_sgr(state):
    """The SGR of `state`, whole: freeze ignores reverse video and keeps the previous background
    on 49, so the colors are written out, swapped while reversed. The UI's background is left
    out: freeze draws each background half a character too wide, which would cut into the next
    colored one"""
    fg, bg = state["fg"], state["bg"]
    if state["reverse"]:
        fg, bg = bg, fg
    background = [] if bg == BG[1:] else ["48"] + bg
    return "\x1b[" + ";".join(["0", *state["attrs"], "38", *fg, *background]) + "m"


def recolor_sgr(match, state):
    """One SGR read into `state`, with the theme's colors, and written back whole"""
    p, i = match.group(1).split(";"), 0
    undone = {"22": ["1", "2"], "23": ["3"], "24": ["4"], "25": ["5"], "29": ["9"]}
    while i < len(p):
        v = p[i]
        ground = {"38": "fg", "48": "bg"}.get(v)
        if ground and i + 2 < len(p) and p[i + 1] == "5" and int(p[i + 2]) < 16:
            state[ground] = rgb(int(p[i + 2]), v)[1:]
            i += 3
            continue
        if ground and i + 1 < len(p) and p[i + 1] in ("2", "5"):
            n = 5 if p[i + 1] == "2" else 3
            state[ground] = p[i + 1:i + n]
            i += n
            continue
        if v.isdigit() and (30 <= int(v) <= 37 or 40 <= int(v) <= 47):
            state["fg" if int(v) < 40 else "bg"] = rgb(int(v) % 10, "")[1:]
        elif v.isdigit() and (90 <= int(v) <= 97 or 100 <= int(v) <= 107):
            state["fg" if int(v) < 100 else "bg"] = rgb(int(v) % 10 + 8, "")[1:]
        elif v == "39":
            state["fg"] = FG[1:]
        elif v == "49":
            state["bg"] = BG[1:]
        elif v == "7":
            state["reverse"] = True
        elif v == "27":
            state["reverse"] = False
        elif v in undone:
            state["attrs"] = [a for a in state["attrs"] if a not in undone[v]]
        elif v in ("", "0"):
            state.update(fg=FG[1:], bg=BG[1:], reverse=False, attrs=[])
        elif v not in state["attrs"]:
            state["attrs"].append(v)
        i += 1
    return state_sgr(state)


def recolor(ansi):
    """The capture with the theme's colors. tmux only writes a color when it changes, so a line
    can start in the color of the line before: each line starts with the colors in use"""
    state = {"fg": FG[1:], "bg": BG[1:], "reverse": False, "attrs": []}
    lines = []
    for line in ansi.rstrip("\n").split("\n"):
        start = state_sgr(state)
        lines.append(start + re.sub(r"\x1b\[([0-9;]*)m", lambda m: recolor_sgr(m, state), line))
    return "\n".join(lines)


def trim_and_scale(src, dst, width=1400):
    """Crops the extra bottom freeze adds, so it matches the top margin"""
    probe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                            "stream=width,height", "-of", "csv=p=0", src],
                           capture_output=True, text=True, check=True)
    w, h = map(int, probe.stdout.strip().split(","))
    raw = subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-i", src, "-f",
                          "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True,
                         check=True).stdout
    bg = raw[0:3]

    def busy(y):
        row = raw[y * w * 3:(y + 1) * w * 3]
        return any(row[x:x + 3] != bg for x in range(0, len(row), 6))

    top = next(y for y in range(h) if busy(y))
    bottom = next(y for y in range(h - 1, -1, -1) if busy(y))
    height = min(h, bottom + 1 + top)
    subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", src, "-vf",
                    f"crop={w}:{height}:0:0,scale={width}:-1", dst], check=True)


def as_user_home(ansi, run):
    """The run folder, shown in full by a popup, as /home/me: same width, so the box stays"""
    sessions = f"{run}/.cache/maxicom/sessions"

    def fix(match):
        token = match.group(0)
        # a popup cuts a long path: the session browser's folder, cut, is shown whole
        full = sessions if sessions.startswith(token) else token
        shown = full.replace(run, "/home/me")
        return shown[:len(token)].ljust(len(token))
    return re.sub(re.escape(REPO) + r"[^ │\x1b]*", fix, ansi)


# monospace fonts with the braille block, for the plot's dots: Windows Terminal's (WSL), the
# Debian / Ubuntu package fonts-cascadia-code, macOS; SHOTS_BRAILLE_FONT names another file
BRAILLE_FONTS = ["/mnt/c/Windows/Fonts/CascadiaMono.ttf",
                 "/usr/share/fonts/truetype/cascadia-code/CascadiaMono.ttf",
                 os.path.expanduser("~/Library/Fonts/CascadiaMono.ttf")]


def braille_font():
    """A monospace font file with braille, and its family name"""
    given = os.environ.get("SHOTS_BRAILLE_FONT")
    path = given or next((f for f in BRAILLE_FONTS if os.path.exists(f)), None)
    if not path or not os.path.exists(path):
        raise RuntimeError("no monospace font with braille: install fonts-cascadia-code, or "
                           "set SHOTS_BRAILLE_FONT to one")
    # a variable font lists its family once per instance
    family = subprocess.run(["fc-query", "-f", "%{family[0]}\n", path], capture_output=True,
                            text=True, check=True).stdout.split("\n")[0]
    return path, family


def freeze_braille(ans, png, run):
    """freeze's PNG draws braille as boxes, its font has none and it doesn't fall back. So:
    freeze's SVG, its font swapped for one with braille, drawn by ffmpeg's librsvg, which
    finds that font through a fontconfig file of the run's"""
    path, family = braille_font()
    svg = png[:-4] + ".svg"
    subprocess.run(["freeze", ans, "--language", "ansi", "--theme", "tokyonight-night", "-o",
                    svg, "--window=false", "--padding", "10", "--margin", "0", "--font.size",
                    "14", "--line-height", "1.2"], stdin=subprocess.DEVNULL,
                   capture_output=True, check=True, timeout=60)
    text = open(svg).read().replace('font-family="JetBrains Mono"', f'font-family="{family}"')
    open(svg, "w").write(text)
    conf = f"{run}/fonts.conf"
    open(conf, "w").write(
        '<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "fonts.dtd">\n<fontconfig>\n'
        '  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>\n'
        f'  <dir>{os.path.dirname(path)}</dir>\n  <cachedir>{run}/fontcache</cachedir>\n'
        '</fontconfig>\n')
    subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-width", "2800",
                    "-keep_ar", "1", "-i", svg, png], env={**os.environ, "FONTCONFIG_FILE": conf},
                   check=True, timeout=120)


def render(name, ansi, run, rows=None, braille=False):
    ansi = as_user_home(ansi, run)
    if rows:
        ansi = "\n".join(ansi.rstrip("\n").split("\n")[rows[0]:rows[1]])
    open(f"{run}/{name}.ans", "w").write(ansi)
    open(f"{run}/{name}.tn.ans", "w").write(recolor(ansi))
    if braille:
        freeze_braille(f"{run}/{name}.tn.ans", f"{run}/{name}.png", run)
    else:
        subprocess.run(["freeze", f"{run}/{name}.tn.ans", "--language", "ansi", "--theme",
                        "tokyonight-night", "-o", f"{run}/{name}.png", "--window=false",
                        "--padding", "10", "--margin", "0", "--font.size", "14",
                        "--line-height", "1.2"], stdin=subprocess.DEVNULL,
                       capture_output=True, check=True, timeout=60)
    os.makedirs(IMAGES, exist_ok=True)
    trim_and_scale(f"{run}/{name}.png", f"{IMAGES}/{name}.png")


# ---- scenarios ----

LINE = {(None, "mode"): '"line"', (None, "show_timestamps"): "true"}


def esp_link(devices):
    return f'[[link]]\nname = "esp32"\naddress = "{devices[0].path}"\n'


def scenario(name, devices, steps, *, config=None, links=None, triggers=None, plots=None,
             args=None, restore=False, rows=None, fixtures=None, size=(COLS, ROWS),
             braille=False):
    run = f"{WORK}/{name}"
    if not os.path.isabs(run) or not run.startswith(f"{REPO}/target/"):
        sys.exit(f"refusing: {run} isn't under target/")  # it becomes HOME
    shutil.rmtree(run, ignore_errors=True)
    os.makedirs(f"{run}/rec")
    env = prepare(run, config or {}, links(devices) if callable(links) else links, triggers,
                  plots)
    if fixtures:
        fixtures(f"{run}/.cache", devices)
    argv = [BIN, *(args(devices) if callable(args) else args or ["esp32"])]
    if not restore:
        argv.append("--no-restore")
    envs = "-u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME HOME=" + run
    tmux("kill-server", check=False)
    tmux("new-session", "-d", "-s", "shots", "-x", str(size[0]), "-y", str(size[1]),
         f"cd {run} && env {envs} {' '.join(argv)}; echo EXIT=$?; sleep 60")
    time.sleep(1.5)
    for dev in devices:
        dev.start()
    try:
        for step in steps:
            step(devices)
        if not alive():
            raise RuntimeError("the maxicom session is gone")
        ansi = tmux("capture-pane", "-e", "-p", "-t", "=shots:").stdout
    finally:
        tmux("send-keys", "-t", "=shots:", "C-q", check=False)
        time.sleep(0.3)
        tmux("kill-server", check=False)
    render(name, ansi, run, rows, braille)
    plain = re.sub(r"\x1b\[[0-9;]*m", "", ansi)
    open(f"{run}/{name}.txt", "w").write(plain)
    print(f"{name}: done")


def wait(seconds):
    return lambda devices: time.sleep(seconds)


def fed(extra=0.4):
    def step(devices):
        for dev in devices:
            dev.done.wait(30)
        time.sleep(extra)
    return step


def press(*names):
    return lambda devices: keys(*names)


def typed(literal):
    return lambda devices: text(literal)


def esp(every=0.08, more=False):
    return Pty(ESP_BOOT + (ESP_MORE if more else []), every)


def directory_links(devices):
    esp32, nucleo = devices
    return (f'[[link]]\nname = "esp32"\naddress = "{esp32.path}"\n\n'
            f'[[link]]\nname = "nucleo-f401"\naddress = "{nucleo.path}"\nbaud = 921600\n\n'
            f'[[link]]\nname = "gateway console"\naddress = "tcp://127.0.0.1:{tcp_listener()}"\n\n'
            f'[[link]]\nname = "router"\naddress = "tcp://127.0.0.1:{closed_port()}"\n\n'
            f'[[link]]\nname = "lab psu"\naddress = "/dev/ttyUSB7"\nbaud = 9600\n')


def browser_sessions(cache, devices):
    now = datetime.now().replace(second=12, microsecond=345000)
    esp32 = devices[0].path
    plain_boot = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in ESP_BOOT]
    session(cache, now - timedelta(days=3, hours=2), "/dev/ttyACM0", [
        "U-Boot 2024.01 (Jan 08 2026 - 10:12:44 +0100)", "DRAM:  512 MiB",
        "Hit any key to stop autoboot:  0", "Starting kernel ..."])
    session(cache, now - timedelta(days=2, hours=5), "tcp://192.168.1.50:4000",
            ["gateway login: root", "root@gateway:~# uptime",
             " 14:22:05 up 3 days,  2:41,  1 user,  load average: 0.08, 0.03, 0.01"])
    session(cache, now - timedelta(days=1, hours=7), esp32, plain_boot[:12])
    session(cache, now - timedelta(hours=3), esp32, plain_boot, ended=now - timedelta(hours=2))
    session(cache, now - timedelta(days=9), "/dev/ttyUSB1", ["OK", "AT+GMR", "1.7.4.0"],
            meta=False)


def restore_session(cache, devices):
    now = datetime.now()
    session(cache, now - timedelta(hours=1, minutes=12), devices[0].path, ESP_BOOT[-10:],
            ended=now - timedelta(minutes=25))


TRIGGERS = """[u-boot]
match = "Hit any key to stop autoboot"
run = "send ' '"

[login]
match = "login: "
run = 'send root\\r'
rearm = true
armed = true

[wifi-retry]
match = "Haven't to connect to a suitable AP"
run = "send 'wifi reconnect\\r'"
"""

SCENARIOS = {
    "line-mode": lambda: scenario(
        "line-mode", [esp()], [fed(), typed("AT+GMR")], config=LINE, links=esp_link),
    "overview": lambda: scenario(
        "overview", [Pty(SHELL, 0.05)], [fed(0.6)], links=esp_link),
    "terminal-mode": lambda: scenario(
        "terminal-mode", [Pty(top_screen(), 0.05)], [fed(0.6)], links=esp_link),
    "raw-side-panel": lambda: scenario(
        "raw-side-panel", [esp()], [fed()], config={**LINE, ("raw_side", "view"): '"hex"'},
        links=esp_link, size=(130, ROWS)),
    "directory": lambda: scenario(
        "directory", [esp(), Pty()], [fed(), press("C-a", "d"), press("t"), wait(2.8)],
        links=directory_links),
    "session-browser": lambda: scenario(
        "session-browser", [esp()], [fed(), press("C-a", "j"), wait(0.6)],
        config=LINE, links=esp_link, fixtures=browser_sessions),
    "settings-popup": lambda: scenario(
        "settings-popup", [esp()], [fed(), press("C-a", "o"), wait(0.5)], config=LINE,
        links=esp_link),
    "status-bar": lambda: scenario(
        "status-bar", [Pty(SHELL, 0.05)],
        [fed(), press("C-a", "l"), wait(0.5), press("Enter"), wait(0.5), press("C-a", "U"),
         wait(4.5)],
        links=esp_link, rows=(-1, None)),
    "search": lambda: scenario(
        "search", [esp()], [fed(), press("C-a", "/"), typed("temp"), press("C-p"), wait(0.5)],
        config=LINE, links=esp_link),
    "command-line": lambda: scenario(
        "command-line", [esp()], [fed(), press("C-a", ":"), typed("re"), wait(0.5)],
        config=LINE, links=esp_link),
    "copy-mode": lambda: scenario(
        "copy-mode", [esp()],
        [fed(), press("C-a", "b"), press("k", "k", "k", "0", "v", "j", "e", "e"), wait(0.4)],
        config=LINE, links=esp_link),
    # added: one per chapter that shows a popup or a state the text describes
    "help": lambda: scenario(
        "help", [esp()], [fed(), press("C-a", "?"), wait(0.5)], config=LINE, links=esp_link),
    "triggers": lambda: scenario(
        "triggers", [esp()], [fed(), press("C-a", "T"), press("Down"), wait(0.5)],
        config=LINE, links=esp_link, triggers=TRIGGERS),
    "record-dialog": lambda: scenario(
        "record-dialog", [esp()], [fed(), press("C-a", "l"), wait(0.5)], config=LINE,
        links=esp_link),
    "line-settings": lambda: scenario(
        "line-settings", [esp()], [fed(), press("C-a", "p"), wait(0.5)], config=LINE,
        links=esp_link),
    "restore": lambda: scenario(
        "restore", [Pty(ESP_MORE, 0.3)], [fed(0.6), press("PPage"), wait(0.4)], config=LINE,
        links=esp_link,
        restore=True, fixtures=restore_session),
    "plots": lambda: scenario(
        "plots", [Pty(pid_feed(), 0.02)], [fed(0.8)],
        config={("raw_side", "split_percent"): "55"}, links=esp_link, plots=PLOTS,
        args=["esp32", "--plot", "pid"], size=(130, ROWS), braille=True),
}

if __name__ == "__main__":
    names = sys.argv[1:] or list(SCENARIOS)
    subprocess.run(["cargo", "build", "--quiet"], cwd=REPO, check=True)
    failed = []
    for name in names:
        try:
            SCENARIOS[name]()
        except Exception as err:  # one bad scenario doesn't stop the others
            print(f"{name}: FAILED: {err}")
            failed.append(name)
    sys.exit(1 if failed else 0)
