"""Tests for the spike helpers. Run: python -m unittest discover -s spike -v

Needs `pip install av pillow psutil` for the pipeline test, ffmpeg on PATH for the
fake server, and qemu-system-x86_64 on PATH for the QEMU test. Tests skip when a
tool is missing.
"""

import argparse
import json
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import spike  # noqa: E402


class ProtocolTests(unittest.TestCase):
    def test_touch_matches_reference_vector(self):
        # Expected bytes copied from app/tests/test_control_msg_serialize.c (scrcpy v5.0.1),
        # test_serialize_inject_touch_event.
        expected = bytes([
            0x02, 0x00,
            0x12, 0x34, 0x56, 0x78, 0x87, 0x65, 0x43, 0x21,
            0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0xC8,
            0x04, 0x38, 0x07, 0x80,
            0xFF, 0xFF,
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x01,
        ])
        got = spike.encode_touch(spike.ACTION_DOWN, 0x1234567887654321, 100, 200, 1080, 1920,
                                 1.0, action_button=1, buttons=1)
        self.assertEqual(got, expected)
        self.assertEqual(len(got), 32)

    def test_generic_finger_pointer_id(self):
        msg = spike.encode_touch(spike.ACTION_UP, spike.POINTER_ID_GENERIC_FINGER, 1, 2, 3, 4, 0.0)
        self.assertEqual(msg[2:10], b"\xff" * 7 + b"\xfe")
        self.assertEqual(msg[22:24], b"\x00\x00")  # pressure 0

    def test_session_packet(self):
        h = struct.pack(">III", 0x80000000, 720, 1280)
        self.assertEqual(spike.parse_packet_header(h),
                         {"kind": "session", "width": 720, "height": 1280, "client_resize": False})
        h = struct.pack(">III", 0x80000001, 1, 2)
        self.assertTrue(spike.parse_packet_header(h)["client_resize"])

    def test_media_packet_flags(self):
        cfg = spike.parse_packet_header(struct.pack(">QI", 1 << 62, 40))
        self.assertTrue(cfg["config"] and not cfg["key"])
        key = spike.parse_packet_header(struct.pack(">QI", (1 << 61) | 12345, 999))
        self.assertTrue(key["key"] and not key["config"])
        self.assertEqual((key["pts_us"], key["size"]), (12345, 999))

    def test_bad_header_length(self):
        with self.assertRaises(ValueError):
            spike.parse_packet_header(b"\x00" * 11)


@unittest.skipUnless(shutil.which("ffmpeg"), "ffmpeg missing")
class PipelineTest(unittest.TestCase):
    def test_probe_against_fake_server(self):
        try:
            import av  # noqa: F401
            import PIL  # noqa: F401
        except ImportError:
            self.skipTest("av/pillow missing")
        from fake_scrcpy_server import FakeScrcpyServer, W, H

        with tempfile.TemporaryDirectory() as d:
            srv = FakeScrcpyServer(Path(d) / "clips")
            srv.start()
            out = Path(d) / "out"
            args = argparse.Namespace(out=str(out), direct=f"127.0.0.1:{srv.port}", timeout=20.0,
                                      tap_timeout=5.0, max_size=720, adb="adb", serial="x", server="x")
            spike.cmd_probe(args)
            srv.close()

            res = json.loads((out / "probe.json").read_text())
            self.assertEqual(res["device_name"], "FakeDevice")
            self.assertEqual(res["frame_size"], [W, H])
            self.assertTrue((out / "frame0.png").stat().st_size > 0)
            self.assertIsNotNone(res["tap_to_changed_frame_ms"], res)
            self.assertTrue((out / "after_tap.png").exists())

            # the server must have received DOWN then UP at the screen centre, generic finger
            self.assertEqual([t[1] for t in srv.touches], [spike.ACTION_DOWN, spike.ACTION_UP])
            self.assertEqual(srv.touches[0], spike.encode_touch(
                spike.ACTION_DOWN, spike.POINTER_ID_GENERIC_FINGER, W // 2, H // 2, W, H, 1.0))


@unittest.skipUnless(shutil.which("qemu-system-x86_64") and shutil.which("qemu-img"), "qemu missing")
class QemuTest(unittest.TestCase):
    def test_disk_launch_qmp_cycle(self):
        # TCG + paused (-S): validates our command line, the qcow2 overlay and the QMP client
        # on a machine without WHPX. It does NOT validate -accel whpx.
        with tempfile.TemporaryDirectory() as d:
            common = dict(workdir=d, name="t", qemu=None)
            spike.cmd_disk(argparse.Namespace(**common, size="1G", overlay=None))
            spike.cmd_disk(argparse.Namespace(**common, size="1G", overlay="t"))
            info = subprocess.run(["qemu-img", "info", str(Path(d) / "t.qcow2")],
                                  capture_output=True, text=True, check=True).stdout
            self.assertIn("backing file format: qcow2", info)

            port = spike.free_port()
            spike.cmd_launch(argparse.Namespace(
                **common, qmp_port=port, accel="tcg", cpu="qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt",
                cpus=1, ram=128, disk_if="ide", vga="std", display="none",
                adb_port=spike.free_port(), iso=None, extra="-S"))
            try:
                q = spike.Qmp(port)
                self.assertEqual(q.execute("query-status")["return"]["status"], "prelaunch")
                q.execute("cont")
                self.assertTrue(q.execute("query-status")["return"]["running"])
                q.execute("stop")
                self.assertFalse(q.execute("query-status")["return"]["running"])
                q.execute("quit")
            finally:
                pid = Path(d) / "t.pid"
                if pid.exists():
                    subprocess.run(["kill", pid.read_text().strip()], check=False)
            time.sleep(0.5)
            cmdline = (Path(d) / "t.cmdline.txt").read_text()
            self.assertIn("-machine q35 -accel tcg", cmdline)
            self.assertIn("hostfwd=tcp:127.0.0.1:", cmdline)


if __name__ == "__main__":
    unittest.main()
