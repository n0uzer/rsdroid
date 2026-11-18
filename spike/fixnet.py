#!/usr/bin/env python3
"""Bring the guest network up so QEMU's hostfwd reaches adbd.

THROWAWAY CODE for the `spike/` branch (PLAN.md, M-1). It exists because Android-x86
9.0-r2 does not bring its own network up under QEMU, and without an IP the host never
reaches adbd even though adbd is listening.

Why this is needed:
  * `/system/etc/init.sh` renames the QEMU NIC `eth0` -> `wifi_eth` and creates a
    `virt_wifi` `wlan0` on top;
  * nothing runs DHCP on `wifi_eth`;
  * Android 9 resolves default traffic through policy-routing tables 97/98/99, which
    netd leaves empty, so even a manual address is not enough.

It runs commands on the guest's root serial console. `--ifname auto` picks `eth0` or
`wifi_eth`, whichever exists (the rename has been observed both ways).

    python spike/fixnet.py --port 14555
"""
from __future__ import annotations

import argparse
import socket
import sys
import time


def connect(port: int, timeout: float = 30) -> socket.socket:
    deadline = time.time() + timeout
    last: OSError | None = None
    while time.time() < deadline:
        try:
            return socket.create_connection(("127.0.0.1", port), timeout=5)
        except OSError as e:
            last = e
            time.sleep(0.5)
    raise SystemExit(f"cannot connect to serial on {port}: {last}")


def drain(s: socket.socket, seconds: float) -> str:
    end, out = time.time() + seconds, b""
    while time.time() < end:
        s.settimeout(max(0.2, end - time.time()))
        try:
            d = s.recv(65536)
        except socket.timeout:
            break
        if not d:
            break
        out += d
    return out.decode("utf-8", "replace")


def run(s: socket.socket, line: str, settle: float = 2.0) -> str:
    s.sendall((line + "\n").encode())
    return drain(s, settle)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=14555)
    ap.add_argument("--ifname", dest="iface", default="auto",
                    help="guest NIC name, or 'auto' to pick eth0/wifi_eth")
    ap.add_argument("--wait", type=float, default=0,
                    help="seconds to wait for the guest shell before running")
    ap.add_argument("--ip", default="10.0.2.15")
    ap.add_argument("--prefix", default="255.255.255.0")
    ap.add_argument("--gw", default="10.0.2.2")
    a = ap.parse_args()

    s = connect(a.port)
    drain(s, 2)
    if a.wait:
        drain(s, a.wait)

    if a.iface == "auto":
        for cand in ("eth0", "wifi_eth"):
            if "does not exist" not in run(s, f"ip -4 addr show {cand} | grep inet", 1.5):
                a.iface = cand
                break
        else:
            sys.exit("no guest NIC found: neither eth0 nor wifi_eth exists\n"
                     + run(s, "ip -o link", 2))
        print(f"using interface {a.iface}")

    print(run(s, "id", 1.5).strip())

    # Address on the real NIC.
    run(s, f"ip link set {a.iface} up", 1.0)
    run(s, f"ifconfig {a.iface} {a.ip} netmask {a.prefix} up", 2.0)

    # Android 9 policy routing: fill every table netd consults.
    net = f"{a.ip.rsplit('.', 1)[0]}.0/24"
    for t in (97, 98, 99):
        run(s, f"ip route add {net} dev {a.iface} table {t} 2>/dev/null", 0.6)
        run(s, f"ip route add default via {a.gw} dev {a.iface} table {t} 2>/dev/null", 0.6)

    # wlan0 can shadow those routes with an unusable default.
    run(s, "ip route del default dev wlan0 2>/dev/null", 0.6)

    print(run(s, f"ip -4 addr show {a.iface} | grep inet", 1.5).strip())
    print(run(s, "ip route show table 99 | tr '\\n' ' '", 1.5).strip())
    ping = run(s, f"ping -c1 -W2 -I {a.iface} {a.gw} | tail -2", 5.0)
    print(ping.strip())
    if "1 received" not in ping:
        print("WARNING: gateway is not reachable; hostfwd will not work either")
    s.close()


if __name__ == "__main__":
    main()
