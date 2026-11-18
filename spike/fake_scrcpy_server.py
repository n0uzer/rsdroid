#!/usr/bin/env python3
"""Fake scrcpy-server (v5.0.1 wire format, forward-tunnel mode) for testing spike.py
without a guest. THROWAWAY CODE.

It speaks the protocol as read from the v5.0.1 sources, so a passing test proves
that spike.py agrees with *our reading* of the protocol, not with a real device.
The real check is the probe run against the guest (see spike/README.md).

Connection order mirrors DesktopConnection.open(): first accepted socket = video
(dummy byte, 64-byte device name, codec id, session packet, then packets), second
accepted socket = control (32-byte touch messages from the client).
"""

from __future__ import annotations

import socket
import struct
import subprocess
import threading
import time
from pathlib import Path

CODEC_H264 = 0x68323634
W, H, FPS = 320, 240, 10


def make_clip(path: Path, lavfi: str) -> None:
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lavfi, "-t", "2",
                    "-pix_fmt", "yuv420p", "-c:v", "libx264", "-profile:v", "baseline",
                    "-bf", "0", "-g", "5",
                    "-x264-params", "slices=1:repeat-headers=1",
                    "-f", "h264", str(path)], check=True)


def split_access_units(annexb: bytes) -> tuple[bytes, list[tuple[bytes, bool]]]:
    """Return (config, [(frame_bytes, is_key)]). SPS+PPS are the config packet;
    every slice NAL (type 1 or 5) closes one frame, with preceding SEI kept with it."""
    starts, i = [], 0
    while True:
        j = annexb.find(b"\x00\x00\x01", i)
        if j < 0:
            break
        starts.append(j)
        i = j + 3
    nals = []
    for k, s in enumerate(starts):
        end = starts[k + 1] if k + 1 < len(starts) else len(annexb)
        nal = annexb[s:end]
        if nal.endswith(b"\x00") and k + 1 < len(starts):  # 4-byte start codes
            nal = nal[:-1]
        nals.append(nal)
    config, frames, cur, seen_slice = b"", [], b"", False
    for nal in nals:
        nal = b"\x00" + nal  # normalise to 4-byte start codes
        t = nal[4] & 0x1F
        if t in (7, 8):
            # MediaCodec sends the codec config once, as its own packet. Keep only the
            # first SPS/PPS group and drop the copies x264 repeats before later keyframes.
            if not seen_slice:
                config += nal
        else:
            cur += nal
            if t in (1, 5):
                frames.append((cur, t == 5))
                cur, seen_slice = b"", True
    return config, frames


def packet(payload: bytes, pts_us: int = 0, config: bool = False, key: bool = False) -> bytes:
    f = (1 << 62) if config else (pts_us | ((1 << 61) if key else 0))
    return struct.pack(">QI", f, len(payload)) + payload


class FakeScrcpyServer(threading.Thread):
    def __init__(self, workdir: Path):
        super().__init__(daemon=True)
        workdir.mkdir(parents=True, exist_ok=True)
        make_clip(workdir / "a.h264", f"testsrc=size={W}x{H}:rate={FPS}")
        make_clip(workdir / "b.h264", f"color=c=white:s={W}x{H}:r={FPS}")
        self.clips = {k: split_access_units((workdir / f"{k}.h264").read_bytes()) for k in "ab"}
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(2)
        self.port = self.listener.getsockname()[1]
        self.touches: list[bytes] = []
        self.clip = "a"
        self.stop_flag = threading.Event()

    def run(self) -> None:
        video, _ = self.listener.accept()
        video.sendall(b"\x00")                                  # dummy byte (forward tunnel)
        video.sendall(b"FakeDevice".ljust(64, b"\0"))           # device meta
        video.sendall(struct.pack(">I", CODEC_H264))            # codec id
        video.sendall(struct.pack(">III", 0x80000000, W, H))    # session packet
        control, _ = self.listener.accept()
        threading.Thread(target=self._control_loop, args=(control,), daemon=True).start()
        pts, sent_clip = 0, None
        try:
            while not self.stop_flag.is_set():
                config, frames = self.clips[self.clip]
                if sent_clip != self.clip:
                    video.sendall(packet(config, config=True))
                    sent_clip = self.clip
                for data, key in frames:
                    if self.clip != sent_clip or self.stop_flag.is_set():
                        break
                    video.sendall(packet(data, pts, key=key))
                    pts += 1_000_000 // FPS
                    time.sleep(1 / FPS)
        except OSError:
            pass

    def _control_loop(self, sock: socket.socket) -> None:
        try:
            while True:
                buf = b""
                while len(buf) < 32:
                    chunk = sock.recv(32 - len(buf))
                    if not chunk:
                        return
                    buf += chunk
                self.touches.append(buf)
                if buf[1] == 0:  # ACTION_DOWN: switch to the "changed" clip
                    self.clip = "b"
        except OSError:
            return

    def close(self) -> None:
        self.stop_flag.set()
        self.listener.close()
