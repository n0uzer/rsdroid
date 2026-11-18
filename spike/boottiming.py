#!/usr/bin/env python3
"""Separate the guest's own boot time from the manual network step.

The first Windows run reported "cold boot 131 s", but that number came from `spike.py boot`,
which polls ADB. ADB cannot reach the guest until it has an IP, and the guest gets an IP only
after the manual fix (`spike/fixnet.py`). So 131 s mixes two different things:

  * how long the guest itself takes to reach `sys.boot_completed = 1`, and
  * how long the manual `fixnet` step takes.

This script reads `sys.boot_completed` over the *serial console*, which needs no network, and
times `fixnet` between the serial shell becoming usable and ADB reporting the device ready.

    python spike/boottiming.py --name a --serial-port 14555 --adb-port 15555 --qmp-port 14444
"""
from __future__ import annotations

import argparse
import json
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "spike"))
import spike  # noqa: E402

LOOPBACK = "127.0.0.1"


def serial_connect(port: int, timeout: float = 120) -> socket.socket:
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            return socket.create_connection((LOOPBACK, port), timeout=5)
        except OSError as e:  # QEMU not listening yet
            last = e
            time.sleep(0.5)
    raise SystemExit(f"cannot open serial on {port}: {last}")


def drain(s: socket.socket, seconds: float) -> str:
    end, out = time.time() + seconds, b""
    while time.time() < end:
        s.settimeout(max(0.2, end - time.time()))
        try:
            data = s.recv(65536)
        except socket.timeout:
            break
        if not data:
            break
        out += data
    return out.decode("utf-8", "replace")


def send(s: socket.socket, line: str, settle: float = 1.5) -> str:
    s.sendall((line + "\n").encode())
    return drain(s, settle)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--workdir", default="work")
    ap.add_argument("--name", default="a")
    ap.add_argument("--accel", default="whpx")
    ap.add_argument("--cpu", default="qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--ram", type=int, default=3072)
    # fields build_qemu_argv() reads; kept identical to spike.py's launch defaults
    ap.add_argument("--qemu")
    ap.add_argument("--disk-if", default="ide")
    ap.add_argument("--vga", default="std")
    ap.add_argument("--display", default="none")
    ap.add_argument("--iso")
    ap.add_argument("--extra")
    ap.add_argument("--serial-port", type=int, default=14555)
    ap.add_argument("--adb-port", type=int, default=15555)
    ap.add_argument("--qmp-port", type=int, default=14444)
    ap.add_argument("--ip", default="10.0.2.15")
    ap.add_argument("--prefix", default="255.255.255.0")
    ap.add_argument("--gw", default="10.0.2.2")
    ap.add_argument("--poll", type=float, default=3.0)
    ap.add_argument("--timeout", type=float, default=420)
    a = ap.parse_args()

    wd = Path(a.workdir)
    argv = spike.build_qemu_argv(a) + ["-serial",
                                       f"tcp:{LOOPBACK}:{a.serial_port},server=on,wait=off"]
    (wd / f"{a.name}.cmdline.txt").write_text(subprocess.list2cmdline(argv) + "\n")
    log = open(wd / f"{a.name}.qemu.log", "wb")
    t_launch = time.monotonic()
    p = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT,
                         creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
    time.sleep(2)
    if p.poll() is not None:
        sys.exit(f"QEMU exited immediately with code {p.returncode}")

    s = serial_connect(a.serial_port)
    t_shell = t_boot = None
    deadline = t_launch + a.timeout
    while time.monotonic() < deadline:
        out = send(s, "echo ALIVE_MARKER; getprop sys.boot_completed", 1.5)
        if "ALIVE_MARKER" in out:
            if t_shell is None:
                t_shell = time.monotonic()
            # the value of getprop is the line after the echoed command's output
            for line in out.splitlines():
                if line.strip() == "1":
                    t_boot = time.monotonic()
                    break
        if t_boot is not None:
            break
        time.sleep(max(0.0, a.poll - 1.5))
    if t_shell is None:
        sys.exit("the guest shell never answered on the serial console")
    if t_boot is None:
        sys.exit("sys.boot_completed never reached 1 over the serial console")

    # --- the manual network step, timed separately ---
    t_fix_start = time.monotonic()
    iface = None
    for cand in ("eth0", "wifi_eth"):
        if "does not exist" not in send(s, f"ip -4 addr show {cand} | grep inet"):
            iface = cand
            break
    if iface is None:
        sys.exit("no guest NIC (neither eth0 nor wifi_eth)")
    net = f"{a.ip.rsplit('.', 1)[0]}.0/24"
    send(s, f"ip link set {iface} up", 1.0)
    send(s, f"ifconfig {iface} {a.ip} netmask {a.prefix} up", 2.0)
    for t in (97, 98, 99):
        send(s, f"ip route add {net} dev {iface} table {t} 2>/dev/null", 0.6)
        send(s, f"ip route add default via {a.gw} dev {iface} table {t} 2>/dev/null", 0.6)
    send(s, "ip route del default dev wlan0 2>/dev/null", 0.6)
    t_fix_end = time.monotonic()
    s.close()

    # --- ADB sees the device only now ---
    subprocess.run(["adb", "disconnect", f"{LOOPBACK}:{a.adb_port}"],
                   capture_output=True, text=True)
    t_adb = None
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        subprocess.run(["adb", "connect", f"{LOOPBACK}:{a.adb_port}"],
                       capture_output=True, text=True)
        r = subprocess.run(["adb", "-s", f"{LOOPBACK}:{a.adb_port}", "shell",
                            "getprop", "sys.boot_completed"], capture_output=True, text=True)
        if r.stdout.strip() == "1":
            t_adb = time.monotonic()
            break
        time.sleep(2)

    res = {
        "iface": iface,
        "t_launch_to_serial_shell_s": round(t_shell - t_launch, 1),
        "t_launch_to_guest_boot_completed_s": round(t_boot - t_launch, 1),
        "manual_network_step_s": round(t_fix_end - t_fix_start, 1),
        "t_launch_to_adb_ready_s": round(t_adb - t_launch, 1) if t_adb else None,
    }
    print(json.dumps(res, indent=2))
    print(f"\npid {p.pid} (leave it running for the probe/measure steps, or kill it yourself)")


if __name__ == "__main__":
    main()
