#!/usr/bin/env python3
"""Measure tap-to-pixel latency on the host side, and prove delivery in the guest.

Two different things have to be kept apart:

  * **Delivery.** Did the tap reach the guest? Read from the guest's own InputDispatcher
    `RecentQueue` (a 10-event ring buffer, so compare event ages, not its length) before and
    after the taps. This is a yes/no fact, not a latency.
  * **Latency.** How long from writing the touch message to the host decoding a frame that
    shows it? A crosshair (`pointer_location`) and a touch circle (`show_touches`) are drawn
    at the touch point, so the frame changes *around the touched coordinate*. Only a changed
    region within `--tol` px of the tap counts; a change elsewhere (clock, animation) is
    ignored. This is what `spike.py probe`'s "did any frame change" check failed to do.

    python spike/taplatency.py --serial 127.0.0.1:15560 --server scrcpy-server-v5.0.1 --out probe-lat
"""
from __future__ import annotations

import argparse
import json
import queue
import random
import re
import socket
import statistics
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "spike"))
import spike  # noqa: E402


def window_bbox(ref, img, x, y, half, threshold):
    """Changed bbox inside a window around (x, y), or None."""
    from PIL import ImageChops
    box = (max(0, x - half), max(0, y - half), x + half, y + half)
    a = ref.crop(box).convert("L")
    b = img.crop(box).convert("L")
    d = ImageChops.difference(a, b).point(lambda p: 255 if p > threshold else 0)
    return d.getbbox()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--serial", default="127.0.0.1:15555")
    ap.add_argument("--server", default="scrcpy-server-v5.0.1")
    ap.add_argument("--out", default="probe-lat")
    ap.add_argument("--max-size", type=int, default=720)
    ap.add_argument("--taps", type=int, default=12)
    ap.add_argument("--tol", type=int, default=60, help="window half-size in video pixels")
    ap.add_argument("--threshold", type=int, default=16, help="per-pixel grey delta")
    ap.add_argument("--wait", type=float, default=1.5, help="max s to wait for the frame")
    a = ap.parse_args()

    def adb(*args: str, timeout: float = 60) -> str:
        r = subprocess.run(["adb", "-s", a.serial, *args], capture_output=True,
                           text=True, timeout=timeout)
        if r.returncode != 0:
            raise SystemExit(f"adb {' '.join(args)}: {r.stderr.strip()}")
        return r.stdout.strip()

    def recents() -> list[tuple[str, float]]:
        """RecentQueue holds MotionEvents and KeyEvents mixed; keep both with their age.

        The block also contains DeviceResetEvent/ConfigurationChangedEvent lines, so capture
        every 4-space-indented line and read kind+age per line rather than assuming the block
        starts with a Motion/KeyEvent (otherwise the ages are missed and fresh events read 0).
        """
        txt = adb("shell", "dumpsys input")
        m = re.search(r"RecentQueue: length=\d+\n((?:    .*\n)*)", txt)
        if not m:
            return []
        return [(kind, float(age)) for kind, age in
                re.findall(r"^    (Motion|Key)Event[^\n]*?age=([0-9.]+)ms", m.group(1), re.M)]

    out = Path(a.out)
    out.mkdir(exist_ok=True)
    adb("push", a.server, "/data/local/tmp/scrcpy-server.jar")
    adb("shell", "settings", "put", "system", "pointer_location", "1")
    adb("shell", "settings", "put", "system", "show_touches", "1")

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
                          "control=true", "cleanup=false", f"max_size={a.max_size}",
                          "max_fps=30", "log_level=info"], stdout=log, stderr=subprocess.STDOUT)

    video = spike.connect_first_socket("127.0.0.1", port, 120)
    control = socket.create_connection(("127.0.0.1", port), timeout=10)
    name = spike.read_exact(video, 64).split(b"\0", 1)[0].decode()
    codec = struct.unpack(">I", spike.read_exact(video, 4))[0]
    print(f"device={name!r} codec=0x{codec:08x}")

    frames: queue.Queue = queue.Queue()
    state = {"w": 0, "h": 0}
    reader_error: dict = {}

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
                    frames.put((time.monotonic(), img))
        except (ConnectionError, OSError) as e:
            reader_error["end"] = f"{type(e).__name__}: {e}"
        except Exception as e:  # a decode error must not kill the thread silently
            import traceback
            reader_error["end"] = traceback.format_exc()
            frames.put(None)

    threading.Thread(target=reader, daemon=True).start()

    first = frames.get(timeout=120)
    print(f"first frame {first[1].size}, session {state['w']}x{state['h']}")
    first[1].save(out / "frame0.png")

    def newest(seconds: float):
        img, end = None, time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                img = frames.get(timeout=0.2)[1]
            except queue.Empty:
                pass
        return img

    # A reference is only usable if the picture is steady; wait for two close frames to match.
    def steady_ref(seconds: float = 4.0):
        last_t, last_img = None, None
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                t, img = frames.get(timeout=0.2)
            except queue.Empty:
                continue
            if last_img is not None and t - last_t >= 0.25:
                if not spike.images_differ(last_img, img, threshold=8):
                    return img
            last_t, last_img = t, img
        return last_img

    w, h = state["w"], state["h"]
    cols, rows = 4, 3
    pts = [((2 * i + 1) * w // (2 * cols), (2 * j + 1) * h // (2 * rows))
           for j in range(rows) for i in range(cols)]
    print(f"tap points (video): {pts}")

    before_ages = recents()
    cutoff = (min(age for _, age in before_ages) - 500) if before_ages else float("inf")
    print(f"InputDispatcher RecentQueue before: {len(before_ages)} events, "
          f"freshest age={min(age for _, age in before_ages) if before_ages else '-'}ms")

    latencies, notes = [], []
    for n, (x, y) in enumerate(pts[:a.taps], 1):
        if "end" in reader_error:
            notes.append(f"{n}: video reader stopped: {reader_error['end'].strip()[-200:]}")
            break
        # Reset to a neutral screen. Use am start (no injected input events) so the
        # InputDispatcher ring buffer stays free of KeyEvents and the delivery count below
        # reflects the taps only.
        adb("shell", "am", "start", "-a", "android.intent.action.MAIN",
            "-c", "android.intent.category.HOME")
        time.sleep(1.0)
        ref = steady_ref()
        if ref is None:
            notes.append(f"{n}: no reference frame")
            continue
        t_send = time.monotonic()
        control.sendall(spike.encode_touch(spike.ACTION_DOWN,
                                           spike.POINTER_ID_GENERIC_FINGER, x, y, w, h, 1.0))
        hit = None
        deadline = t_send + a.wait
        while time.monotonic() < deadline:
            try:
                t, img = frames.get(timeout=0.2)
            except queue.Empty:
                continue
            if window_bbox(ref, img, x, y, a.tol, a.threshold):
                hit = (t, img)
                break
        control.sendall(spike.encode_touch(spike.ACTION_UP,
                                           spike.POINTER_ID_GENERIC_FINGER, x, y, w, h, 0.0))
        if hit:
            ms = (hit[0] - t_send) * 1000
            latencies.append(ms)
            hit[1].save(out / f"after_tap_{n:02d}.png")
            print(f"  tap {n:2d} at video ({x:3d},{y:3d}) -> changed within {a.tol}px "
                  f"after {ms:6.1f} ms")
        else:
            notes.append(f"{n}: no change within {a.tol}px of ({x},{y})")
            print(f"  tap {n:2d} at video ({x:3d},{y:3d}) -> no change near the tap")
        time.sleep(0.4)

    newest(0.5)
    after_ages = recents()
    fresh_motion = sorted(age for kind, age in after_ages if kind == "Motion" and age < cutoff)

    def stats(vals):
        if not vals:
            return None
        vals = sorted(vals)
        return {
            "n": len(vals),
            "min_ms": round(vals[0], 1),
            "p10_ms": round(vals[max(0, int(0.10 * (len(vals) - 1)))], 1),
            "median_ms": round(statistics.median(vals), 1),
            "p90_ms": round(vals[max(0, int(0.90 * (len(vals) - 1)))], 1),
            "max_ms": round(vals[-1], 1),
        }

    result = {
        "device_name": name,
        "session": [w, h],
        "taps_sent": len(pts[:a.taps]),
        "tap_to_pixel_host_ms": stats(latencies),
        "raw_latencies_ms": [round(v, 1) for v in latencies],
        "delivery_new_motionevents": len(fresh_motion),
        "delivery_expected": 2 * len(pts[:a.taps]),
        "delivery_note": "RecentQueue is a 10-event ring buffer, so at most 10 fresh "
                         "MotionEvents are observable; fewer than 2* taps is expected",
        "reader_error": reader_error.get("end"),
        "notes": notes,
    }
    (out / "latency.json").write_text(json.dumps(result, indent=2))
    print("\n" + json.dumps(result, indent=2))

    p.terminate()
    log.close()


if __name__ == "__main__":
    main()
