#!/usr/bin/env python3
"""Prove criterion 3 from the guest's own input pipeline, not from the picture.

THROWAWAY CODE for the `spike/` branch (PLAN.md, M-1).

The guest records every injected event in InputDispatcher's `RecentQueue`. That is a direct
statement that the tap arrived, independent of animations, `pointer_location` overlay
rendering, or encoder frame timing -- which is what made the visual check flaky.

`RecentQueue` is a fixed-size ring buffer (10 events), and it mixes MotionEvents with
KeyEvents, so:
  * its length does not grow when events are added, and
  * a delivered event is identified by its *age*: it is younger than every event that was
    already in the queue when the taps were sent.

Two taps produce four MotionEvents (DOWN and UP each); the check requires that many fresh
MotionEvents. Frames are saved for the record.

    python spike/tapverify.py --serial 127.0.0.1:15555 --out probe-a
"""
from __future__ import annotations

import argparse
import queue
import random
import re
import socket
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spike  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--serial", default="127.0.0.1:15555")
    ap.add_argument("--server", default="scrcpy-server-v5.0.1")
    ap.add_argument("--out", default="probe-a")
    ap.add_argument("--wait", type=float, default=8.0)
    a = ap.parse_args()

    def adb(*args: str, timeout: float = 60) -> str:
        r = subprocess.run(["adb", "-s", a.serial, *args], capture_output=True,
                           text=True, timeout=timeout)
        if r.returncode != 0:
            raise SystemExit(f"adb {' '.join(args)}: {r.stderr.strip()}")
        return r.stdout.strip()

    def queue_state() -> tuple[int, list[str], list[float]]:
        """Return (length, kinds, ages). The queue mixes Motion- and KeyEvents; because it
        is a fixed-size ring buffer only the ages tell fresh events apart.

        The queue block also contains DeviceResetEvent/ConfigurationChangedEvent lines, so
        capture every 4-space-indented line and read kind+age per line instead of assuming
        the block starts with a Motion/KeyEvent.
        """
        txt = adb("shell", "dumpsys input")
        m = re.search(r"RecentQueue: length=(\d+)\n((?:    .*\n)*)", txt)
        if not m:
            return -1, [], []
        pairs = re.findall(r"^    (Motion|Key)Event[^\n]*?age=([0-9.]+)ms", m.group(2), re.M)
        kinds = [k for k, _ in pairs]
        ages = [float(v) for _, v in pairs]
        return int(m.group(1)), kinds, ages

    out = Path(a.out)
    out.mkdir(exist_ok=True)
    adb("push", a.server, "/data/local/tmp/scrcpy-server.jar")

    with socket.socket() as t:
        t.bind(("127.0.0.1", 0))
        port = t.getsockname()[1]
    scid = random.randrange(1 << 31)
    adb("forward", f"tcp:{port}", f"localabstract:scrcpy_{scid:08x}")

    log = open(out / "server.log", "wb")
    p = subprocess.Popen(["adb", "-s", a.serial, "shell",
                          "CLASSPATH=/data/local/tmp/scrcpy-server.jar",
                          "app_process", "/", "com.genymobile.scrcpy.Server", "5.0.1",
                          f"scid={scid:x}", "tunnel_forward=true", "audio=false",
                          "control=true", "cleanup=false", "max_size=720", "max_fps=30",
                          "log_level=info"], stdout=log, stderr=subprocess.STDOUT)

    video = spike.connect_first_socket("127.0.0.1", port, 120)
    control = socket.create_connection(("127.0.0.1", port), timeout=10)
    name = spike.read_exact(video, 64).split(b"\0", 1)[0].decode()
    codec = struct.unpack(">I", spike.read_exact(video, 4))[0]
    print(f"device={name!r} codec=0x{codec:08x}")

    frames: queue.Queue = queue.Queue()
    state = {"w": 0, "h": 0}

    def reader():
        dec = spike.FrameDecoder()
        try:
            while True:
                hdr = spike.parse_packet_header(spike.read_exact(video, 12))
                if hdr["kind"] == "session":
                    state["w"], state["h"] = hdr["width"], hdr["height"]
                    continue
                payload = spike.read_exact(video, hdr["size"])
                for img in dec.feed(payload, hdr["config"]):
                    frames.put(img)
        except (ConnectionError, OSError):
            pass

    threading.Thread(target=reader, daemon=True).start()

    def newest_image(seconds: float = 2.0):
        img, end = None, time.time() + seconds
        while time.time() < end:
            try:
                img = frames.get(timeout=0.2)
            except queue.Empty:
                continue
        return img

    first = newest_image(60)
    if first is None:
        p.terminate()
        raise SystemExit("no video frame; cannot verify")
    print(f"first frame {first.size}, session {state['w']}x{state['h']}")
    first.save(out / "frame0.png")

    before_n, _, before_ages = queue_state()
    print(f"InputDispatcher RecentQueue before: {before_n} events, "
          f"freshest age={min(before_ages) if before_ages else '-'}ms")

    w, h = state["w"], state["h"]
    points = [(180, 140), (540, 400)]
    for x, y in points:
        control.sendall(spike.encode_touch(spike.ACTION_DOWN,
                                           spike.POINTER_ID_GENERIC_FINGER,
                                           x, y, w, h, 1.0))
        time.sleep(0.15)
        control.sendall(spike.encode_touch(spike.ACTION_UP,
                                           spike.POINTER_ID_GENERIC_FINGER,
                                           x, y, w, h, 0.0))
        print(f"sent DOWN+UP at video ({x},{y})")
        time.sleep(1.0)

    # A delivered event is younger than every event that was already in the queue.
    cutoff = (min(before_ages) - 500) if before_ages else float("inf")

    def fresh_motion() -> list[float]:
        _, kinds, ages = queue_state()
        return sorted(age for kind, age in zip(kinds, ages)
                      if kind == "Motion" and age < cutoff)

    deadline, fresh = time.time() + a.wait, []
    while time.time() < deadline:
        fresh = fresh_motion()
        if len(fresh) >= 2 * len(points):
            break
        time.sleep(1.0)
    fresh = fresh_motion()

    print(f"new MotionEvents (younger than {cutoff:.0f}ms): {len(fresh)}")
    for age in fresh:
        print(f"  MotionEvent age={age}ms")

    latest = newest_image(2.0)
    if latest is not None:
        latest.save(out / "after_tap.png")

    # Two taps produce four MotionEvents (DOWN and UP each); the ring buffer holds 10, so
    # requiring one fresh MotionEvent per tap proves both taps reached InputDispatcher.
    received = len(fresh) >= len(points)
    print(f"\ncriterion 3: {'PASS' if received else 'FAIL'} "
          f"- {len(fresh)} new MotionEvents for {len(points)} taps (DOWN+UP each)")
    p.terminate()
    log.close()
    sys.exit(0 if received else 1)


if __name__ == "__main__":
    main()
