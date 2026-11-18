#!/usr/bin/env python3
"""rsdroid M-1 feasibility spike helper.

THROWAWAY CODE. It lives on the spike/ branch and is never merged (PLAN.md, M-1).
Python is used here only because the spike needs an H.264 decoder and a PNG
writer in a few lines; the real host code is Rust (see AGENTS.md).

Subcommands:
  disk     create the base qcow2, or an overlay on top of it
  launch   start one headless (or windowed) QEMU instance, log the exact command line
  qmp      talk to a running instance over QMP (status, powerdown, ...)
  boot     connect with ADB and wait for sys.boot_completed, report the time
  probe    push the pinned scrcpy-server, receive one frame, save PNG, inject one tap
  measure  sample idle RAM and CPU of a running QEMU process

Every QEMU flag used here was checked against qemu-system-x86_64 8.2.2 on Linux
(except `-accel whpx...`, which only exists in Windows builds and must be checked
on the Windows machine: `qemu-system-x86_64 -accel help`).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import random
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

# Pinned scrcpy-server. Protocol facts below were read from the v5.0.1 source tree:
#   server/.../device/DesktopConnection.java, Streamer.java, control/ControlMessageReader.java
SCRCPY_VERSION = "5.0.1"
SCRCPY_SERVER_SHA256 = "764eb6f79811d5211fe9df341120882ba9994c7a61b897d7bf3fb662e53bc536"
DEVICE_JAR = "/data/local/tmp/scrcpy-server.jar"

DEVICE_NAME_FIELD_LENGTH = 64
CODEC_H264 = 0x68323634  # "h264"
CTRL_INJECT_TOUCH_EVENT = 2
ACTION_DOWN, ACTION_UP, ACTION_MOVE = 0, 1, 2  # android MotionEvent actions
POINTER_ID_GENERIC_FINGER = (1 << 64) - 2  # UINT64_C(-2) in app/src/control_msg.h


# --------------------------------------------------------------------------- #
# Pure protocol helpers (unit-tested)
# --------------------------------------------------------------------------- #

def encode_touch(action: int, pointer_id: int, x: int, y: int, width: int, height: int,
                 pressure: float, action_button: int = 0, buttons: int = 0) -> bytes:
    """Serialize an inject-touch-event control message (32 bytes, big endian)."""
    p = 0xFFFF if pressure >= 1.0 else int(pressure * 65536)
    return struct.pack(">BBQiiHHHII", CTRL_INJECT_TOUCH_EVENT, action,
                       pointer_id & ((1 << 64) - 1), x, y, width, height, p,
                       action_button, buttons)


def parse_packet_header(h: bytes) -> dict:
    """Parse a 12-byte header from the video socket.

    Session packet (MSB of byte 0 set): u32 flags, u32 width, u32 height.
    Media packet: u64 (config/key flags + PTS), u32 payload size.
    """
    if len(h) != 12:
        raise ValueError("header must be 12 bytes")
    if h[0] & 0x80:
        flags, w, hgt = struct.unpack(">III", h)
        return {"kind": "session", "width": w, "height": hgt, "client_resize": bool(flags & 1)}
    ptsf, size = struct.unpack(">QI", h)
    return {"kind": "media", "size": size, "config": bool(ptsf >> 62 & 1),
            "key": bool(ptsf >> 61 & 1), "pts_us": ptsf & ((1 << 61) - 1)}


def read_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError(f"socket closed after {len(buf)} of {n} bytes")
        buf += chunk
    return bytes(buf)


# --------------------------------------------------------------------------- #
# QMP
# --------------------------------------------------------------------------- #

class Qmp:
    def __init__(self, port: int, timeout: float = 10.0):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        self.f = self.sock.makefile("rw")
        self.greeting = json.loads(self.f.readline())
        self.execute("qmp_capabilities")

    def execute(self, name: str, **args):
        msg = {"execute": name}
        if args:
            msg["arguments"] = args
        self.f.write(json.dumps(msg) + "\n")
        self.f.flush()
        while True:
            try:
                line = self.f.readline()
            except ConnectionResetError:
                line = ""  # QEMU exits on `quit` before it can answer
            if not line:
                return {"return": {}, "note": "connection closed (expected after quit)"}
            m = json.loads(line)
            if "return" in m or "error" in m:
                return m


# --------------------------------------------------------------------------- #
# QEMU command line
# --------------------------------------------------------------------------- #

def find_qemu(explicit: str | None) -> str:
    cands = [explicit] if explicit else []
    cands += [shutil.which("qemu-system-x86_64"), r"C:\Program Files\qemu\qemu-system-x86_64.exe"]
    for c in cands:
        if c and (shutil.which(c) or Path(c).exists()):
            return c
    sys.exit("qemu-system-x86_64 not found; pass --qemu")


def qemu_img(a) -> str:
    q = Path(find_qemu(a.qemu))
    exe = q.with_name("qemu-img.exe" if q.suffix.lower() == ".exe" else "qemu-img")
    return str(exe) if exe.exists() else (shutil.which("qemu-img") or "qemu-img")


def build_qemu_argv(a) -> list[str]:
    wd = Path(a.workdir)
    disk = wd / ("base.qcow2" if a.iso else f"{a.name}.qcow2")
    argv = [find_qemu(a.qemu), "-machine", "q35", "-accel", a.accel]
    if a.cpu:
        argv += ["-cpu", a.cpu]
    argv += ["-smp", str(a.cpus), "-m", str(a.ram)]
    argv += ["-drive", f"file={disk},if={a.disk_if},format=qcow2"]
    if a.iso:
        argv += ["-cdrom", a.iso, "-boot", "d"]
    argv += ["-netdev", f"user,id=n0,hostfwd=tcp:127.0.0.1:{a.adb_port}-:5555",
             "-device", "virtio-net-pci,netdev=n0"]
    argv += ["-vga", a.vga, "-display", a.display]
    argv += ["-qmp", f"tcp:127.0.0.1:{a.qmp_port},server=on,wait=off"]
    argv += ["-pidfile", str(wd / f"{a.name}.pid")]
    if a.extra:
        argv += a.extra.split()
    return argv


# --------------------------------------------------------------------------- #
# Subcommands
# --------------------------------------------------------------------------- #

def cmd_disk(a) -> None:
    wd = Path(a.workdir)
    wd.mkdir(parents=True, exist_ok=True)
    qi = qemu_img(a)
    if a.overlay:
        base = wd / "base.qcow2"
        out = wd / f"{a.overlay}.qcow2"
        cmd = [qi, "create", "-f", "qcow2", "-b", str(base), "-F", "qcow2", str(out)]
    else:
        cmd = [qi, "create", "-f", "qcow2", str(wd / "base.qcow2"), a.size]
    t = time.monotonic()
    subprocess.run(cmd, check=True)
    print(f"{' '.join(cmd)}\ncreated in {time.monotonic() - t:.3f}s")


def cmd_launch(a) -> None:
    wd = Path(a.workdir)
    argv = build_qemu_argv(a)
    line = subprocess.list2cmdline(argv) if os.name == "nt" else " ".join(argv)
    (wd / f"{a.name}.cmdline.txt").write_text(line + "\n")
    (wd / f"{a.name}.launched").write_text(repr(time.time()))
    log = open(wd / f"{a.name}.qemu.log", "wb")
    kw = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt"
          else {"start_new_session": True})
    p = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, **kw)
    time.sleep(2)
    if p.poll() is not None:
        sys.exit(f"QEMU exited immediately with code {p.returncode}; see {wd / (a.name + '.qemu.log')}")
    print(f"started pid {p.pid}\n{line}")


def cmd_qmp(a) -> None:
    q = Qmp(a.qmp_port)
    name = {"status": "query-status", "powerdown": "system_powerdown", "reset": "system_reset",
            "stop": "stop", "cont": "cont", "quit": "quit"}[a.action]
    print(json.dumps(q.execute(name)))


def adb(a, *args: str, check: bool = True, timeout: float | None = 60) -> str:
    cmd = [a.adb, *args]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed: {r.stderr.strip() or r.stdout.strip()}")
    return r.stdout.strip()


def cmd_boot(a) -> None:
    start = time.time()
    marker = Path(a.workdir) / f"{a.name}.launched"
    launched = float(marker.read_text()) if marker.exists() else start
    deadline = start + a.timeout
    while time.time() < deadline:
        adb(a, "connect", a.serial, check=False, timeout=10)
        out = adb(a, "-s", a.serial, "shell", "getprop", "sys.boot_completed", check=False, timeout=15)
        if out == "1":
            res = {"serial": a.serial, "boot_completed_after_launch_s": round(time.time() - launched, 1)}
            print(json.dumps(res))
            return
        time.sleep(2)
    sys.exit(f"sys.boot_completed did not reach 1 within {a.timeout}s (adbd listening on tcp 5555 in the guest?)")


class FrameDecoder:
    """Decode scrcpy media packets (one access unit each) with PyAV."""

    def __init__(self):
        import av
        self.av = av
        self.ctx = av.CodecContext.create("h264", "r")
        self.ctx.thread_count = 1
        self.pending_config = b""

    def feed(self, payload: bytes, config: bool):
        if config:
            self.pending_config = payload
            return []
        data, self.pending_config = self.pending_config + payload, b""
        return [f.to_image() for f in self.ctx.decode(self.av.Packet(data))]


def images_differ(a, b, threshold: int = 32) -> bool:
    from PIL import ImageChops
    d = ImageChops.difference(a.convert("L"), b.convert("L")).point(lambda p: 255 if p > threshold else 0)
    return d.getbbox() is not None


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def connect_first_socket(host: str, port: int, timeout: float) -> socket.socket:
    """In forward mode the TCP connect succeeds even if the server is not up yet;
    the dummy byte tells us whether something is really listening on the device."""
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        s = socket.create_connection((host, port), timeout=5)
        try:
            if read_exact(s, 1) == b"\x00":
                return s
        except (ConnectionError, OSError) as e:
            last = e
        s.close()
        time.sleep(0.3)
    raise TimeoutError(f"no dummy byte from the server: {last}")


def cmd_probe(a) -> None:
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    result: dict = {"scrcpy_version": SCRCPY_VERSION}
    server_proc = None
    t0 = time.monotonic()

    if a.direct:  # test mode against fake_scrcpy_server.py, no adb involved
        host, port = a.direct.split(":")
        port = int(port)
    else:
        jar = Path(a.server)
        digest = hashlib.sha256(jar.read_bytes()).hexdigest()
        if digest != SCRCPY_SERVER_SHA256:
            sys.exit(f"scrcpy-server checksum mismatch: got {digest}, pinned {SCRCPY_SERVER_SHA256}")
        adb(a, "-s", a.serial, "push", str(jar), DEVICE_JAR)
        adb(a, "-s", a.serial, "shell", "settings", "put", "system", "pointer_location", "1", check=False)
        scid = random.randrange(1 << 31)
        port, host = free_port(), "127.0.0.1"
        adb(a, "-s", a.serial, "forward", f"tcp:{port}", f"localabstract:scrcpy_{scid:08x}")
        server_cmd = [a.adb, "-s", a.serial, "shell",
                      f"CLASSPATH={DEVICE_JAR}", "app_process", "/", "com.genymobile.scrcpy.Server",
                      SCRCPY_VERSION, f"scid={scid:x}", "tunnel_forward=true", "audio=false",
                      "control=true", "cleanup=false", f"max_size={a.max_size}", "max_fps=30",
                      "log_level=info"]
        server_proc = subprocess.Popen(server_cmd, stdout=open(out / "server.log", "wb"),
                                       stderr=subprocess.STDOUT)
        result["server_cmdline"] = " ".join(server_cmd)

    try:
        video = connect_first_socket(host, port, a.timeout)
        name = read_exact(video, DEVICE_NAME_FIELD_LENGTH).split(b"\0", 1)[0].decode("utf-8", "replace")
        codec = struct.unpack(">I", read_exact(video, 4))[0]
        if codec != CODEC_H264:
            sys.exit(f"unexpected codec id 0x{codec:08x}")
        control = socket.create_connection((host, port), timeout=10)
        result.update(device_name=name, codec="h264")

        frames: queue.Queue = queue.Queue()
        state = {"width": 0, "height": 0}

        def reader():
            dec = FrameDecoder()
            try:
                while True:
                    hdr = parse_packet_header(read_exact(video, 12))
                    if hdr["kind"] == "session":
                        state["width"], state["height"] = hdr["width"], hdr["height"]
                        continue
                    payload = read_exact(video, hdr["size"])
                    for img in dec.feed(payload, hdr["config"]):
                        frames.put((time.monotonic(), img))
            except (ConnectionError, OSError):
                frames.put(None)

        threading.Thread(target=reader, daemon=True).start()

        first = frames.get(timeout=a.timeout)
        if first is None:
            sys.exit("video socket closed before the first frame")
        result["time_to_first_frame_s"] = round(first[0] - t0, 2)
        result["frame_size"] = list(first[1].size)
        first[1].save(out / "frame0.png")

        # settle: use the newest frame within ~1.5 s as the "before tap" reference
        ref_img, settle_end = first[1], time.monotonic() + 1.5
        while time.monotonic() < settle_end:
            try:
                item = frames.get(timeout=0.3)
            except queue.Empty:
                continue
            if item is None:
                break
            ref_img = item[1]
        ref_img.save(out / "before_tap.png")

        w, h = state["width"], state["height"]
        x, y = w // 2, h // 2
        t_tap = time.monotonic()
        control.sendall(encode_touch(ACTION_DOWN, POINTER_ID_GENERIC_FINGER, x, y, w, h, 1.0))
        control.sendall(encode_touch(ACTION_UP, POINTER_ID_GENERIC_FINGER, x, y, w, h, 0.0))
        result["tap_at"] = [x, y, w, h]

        deadline = t_tap + a.tap_timeout
        while time.monotonic() < deadline:
            try:
                item = frames.get(timeout=0.2)
            except queue.Empty:
                continue
            if item is None:
                break
            if images_differ(ref_img, item[1]):
                item[1].save(out / "after_tap.png")
                result["tap_to_changed_frame_ms"] = round((item[0] - t_tap) * 1000)
                break
        else:
            result["tap_to_changed_frame_ms"] = None
            result["note"] = "no visibly different frame after the tap; check pointer_location / the guest manually"
    finally:
        if server_proc:
            server_proc.terminate()

    (out / "probe.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


def cmd_measure(a) -> None:
    import psutil
    pid = int((Path(a.workdir) / f"{a.name}.pid").read_text().strip())
    p = psutil.Process(pid)
    p.cpu_percent(None)
    samples, rss = [], []
    end = time.time() + a.seconds
    while time.time() < end:
        time.sleep(min(5, max(0.1, end - time.time())))
        samples.append(p.cpu_percent(None))
        rss.append(p.memory_info().rss / 2**20)
    res = {"pid": pid, "seconds": a.seconds,
           "cpu_percent_of_one_core_mean": round(sum(samples) / len(samples), 1),
           "cpu_percent_of_one_core_max": round(max(samples), 1),
           "rss_mb_mean": round(sum(rss) / len(rss)), "rss_mb_max": round(max(rss))}
    print(json.dumps(res))


# --------------------------------------------------------------------------- #

def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def common(p, qemu=False, qmp=False, adbopt=False):
        p.add_argument("--workdir", default="work")
        p.add_argument("--name", default="a", help="instance name (file prefix)")
        if qemu:
            p.add_argument("--qemu", help="path to qemu-system-x86_64")
        if qmp:
            p.add_argument("--qmp-port", type=int, default=14444)
        if adbopt:
            p.add_argument("--adb", default=shutil.which("adb") or "adb")
            p.add_argument("--serial", default="127.0.0.1:15555")

    p = sub.add_parser("disk")
    common(p, qemu=True)
    p.add_argument("--size", default="16G", help="base image size")
    p.add_argument("--overlay", metavar="NAME", help="create NAME.qcow2 on top of base.qcow2")
    p.set_defaults(fn=cmd_disk)

    p = sub.add_parser("launch")
    common(p, qemu=True, qmp=True)
    p.add_argument("--accel", default="whpx", help="e.g. whpx, whpx,kernel-irqchip=off, tcg (candidates, verify on Windows)")
    p.add_argument("--cpu", default=None, help="e.g. qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt (candidate, verify)")
    p.add_argument("--cpus", type=int, default=2)
    p.add_argument("--ram", type=int, default=3072, help="MiB")
    p.add_argument("--disk-if", default="ide", help="ide (AHCI on q35) or virtio; record which one the guest needs")
    p.add_argument("--vga", default="std")
    p.add_argument("--display", default="none", help="none for headless; sdl or gtk for a window")
    p.add_argument("--adb-port", type=int, default=15555)
    p.add_argument("--iso", help="install mode: boot this ISO and write to base.qcow2")
    p.add_argument("--extra", help="extra QEMU arguments, whitespace separated")
    p.set_defaults(fn=cmd_launch)

    p = sub.add_parser("qmp")
    common(p, qmp=True)
    p.add_argument("action", choices=["status", "powerdown", "reset", "stop", "cont", "quit"])
    p.set_defaults(fn=cmd_qmp)

    p = sub.add_parser("boot")
    common(p, adbopt=True)
    p.add_argument("--timeout", type=int, default=300)
    p.set_defaults(fn=cmd_boot)

    p = sub.add_parser("probe")
    common(p, adbopt=True)
    p.add_argument("--server", default=f"scrcpy-server-v{SCRCPY_VERSION}")
    p.add_argument("--out", default="probe-out")
    p.add_argument("--max-size", type=int, default=720)
    p.add_argument("--timeout", type=float, default=60)
    p.add_argument("--tap-timeout", type=float, default=5)
    p.add_argument("--direct", metavar="HOST:PORT", help="skip adb, talk to a fake server (tests)")
    p.set_defaults(fn=cmd_probe)

    p = sub.add_parser("measure")
    common(p)
    p.add_argument("--seconds", type=int, default=60)
    p.set_defaults(fn=cmd_measure)
    return ap


if __name__ == "__main__":
    args = build_parser().parse_args()
    args.fn(args)
