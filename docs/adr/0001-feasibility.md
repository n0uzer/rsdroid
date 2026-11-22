# ADR 0001: Feasibility of QEMU + WHPX + scrcpy-server

Status: **Accepted** on 2026-11-22 by the maintainer. All four go criteria pass on the
target platform, both with Hyper-V, WSL2 and Virtual Machine Platform (VMP) off and with
them on. The recommendation is **go**; see "Decision".

## Context

`PLAN.md` M-1 asks to prove four go criteria before any workspace exists: Android boots
under WHPX, one frame is received and decoded, one tap works, two instances coexist. The
spike code (`spike/`) lives on `spike/m-1-feasibility` and is not merged. This ADR records
the verified facts, the command lines, the measurements, the failures and a go/adjust/stop
recommendation.

## Target machine

| Item | Value |
| --- | --- |
| OS | Windows 11 Pro, build 10.0.26200 |
| CPU | AMD Ryzen 5 5600, 6 cores / 12 threads |
| RAM | 16,305 MB (2 x 8 GB Kingston 3200) |
| GPU | NVIDIA GeForce GTX 1660 SUPER |
| QEMU | 11.1.0 (`v11.1.0-12130-ge470268ff4`), winget `SoftwareFreedomConservancy.QEMU`, `C:\Program Files\qemu` |
| QEMU accelerators | `tcg`, `whpx` |
| adb | 1.0.41, version `37.0.1-15733141` (winget `Google.PlatformTools`) |
| Python | 3.12.10, `av` 19.0.2, `pillow` 13.0.0, `psutil` 7.2.2 |

Both Hyper-V and VMP were enabled, the machine was rebooted, and the repeat was run with
them active (see "Results under Hyper-V + WSL2"). Hypervisor state afterwards:
`HypervisorPresent=True`, `vmcompute`/`vmms`/`HvHost`/`WSLService` running,
`RebootPending=False`, `wsl --status` clean, Ubuntu running as WSL2 on kernel
`6.18.54.1-microsoft-standard-WSL2` (12 CPUs, 7.9 GB).

## Facts verified against the real tools

### scrcpy-server

- Pinned version: **v5.0.1** (newest tag at the time of writing; commit `a60891a`).
- Asset: `scrcpy-server-v5.0.1`, 733,930 bytes.
- SHA-256: `764eb6f79811d5211fe9df341120882ba9994c7a61b897d7bf3fb662e53bc536`
  (computed from the downloaded release asset; equals the value in the v5.0.1 `doc/build.md`).
- The server refuses to start if the client version argument differs from its own version
  (`Options.java`), which is why the version is pinned.
- Wire format read from the v5.0.1 sources (`DesktopConnection.java`, `Streamer.java`,
  `ControlMessageReader.java`) and checked against `doc/develop.md`:
  - Forward tunnel: the device accepts sockets in the order video, audio, control. The
    first accepted socket receives a dummy `0x00` byte, then a 64-byte NUL-padded device
    name. The device name and the codec id are sent only *after* every socket is accepted,
    so a client that reads the name before connecting the control socket deadlocks.
  - Video socket: `u32` codec id (`0x68323634` = h264), then a 12-byte session packet
    (`u32` flags with the MSB set, `u32` width, `u32` height) for each capture session,
    then media packets with a 12-byte header (`u64` flags and PTS, `u32` size). Bit 62 marks
    a config packet, bit 61 a key frame.
  - Touch injection: 32 bytes, big endian. The byte layout matches the reference vector in
    `app/tests/test_control_msg_serialize.c`, which is reused in `spike/test_spike.py`.
  - The encoder sets `KEY_REPEAT_PREVIOUS_FRAME_AFTER`, so a static screen still produces
    frames.
- `doc/develop.md` describes the protocol "in scrcpy 4.0". The source of v5.0.1 agrees on
  everything listed above, but the document is not authoritative. The recorded-stream
  tests planned for the parser should be generated from v5.0.1.

### QEMU command line

Verified on this Windows build (QEMU 11.1.0, WHPX): `-machine q35`, `-accel whpx`,
`-cpu qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt`, `-smp 2`, `-m 3072`,
`-drive ...,if=ide,format=qcow2`, `-netdev user,...,hostfwd=tcp:127.0.0.1:P-:5555`,
`-device virtio-net-pci`, `-vga std`, `-display none`,
`-qmp tcp:127.0.0.1:P,server=on,wait=off`, `-pidfile`, `-serial tcp:127.0.0.1:P,...`, and
`qemu-img create -f qcow2 -b base -F qcow2 overlay`. QMP `query-status`, `cont`, `stop` and
`quit` work; QEMU drops the connection on `quit` without replying.

The exact working command line is `work/a.cmdline.txt`:

```
"C:\Program Files\qemu\qemu-system-x86_64.exe" -machine q35 -accel whpx
-cpu qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt -smp 2 -m 3072
-drive file=work\a.qcow2,if=ide,format=qcow2
-netdev user,id=n0,hostfwd=tcp:127.0.0.1:15555-:5555 -device virtio-net-pci,netdev=n0
-vga std -display none -qmp tcp:127.0.0.1:14444,server=on,wait=off
-pidfile work\a.pid -serial tcp:127.0.0.1:14555,server=on,wait=off
```

The acceleration/CPU matrix was measured with `work/whpx_matrix.py`; the same results were
seen with Hyper-V active (see finding 1).

### Spike pipeline

`spike/test_spike.py` passes against a fake server that speaks the format above and serves
real H.264 produced by ffmpeg: handshake, session and config packets, decode to PNG, touch
DOWN and UP received by the server. `python -m unittest discover -s spike` is 7 tests, OK
(1 skipped when QEMU is absent). This shows the client agrees with our reading of the
protocol; the real-device runs below show a real guest produces such a stream.

## Results

### Baseline (WHPX; Hyper-V, WSL2 and VMP disabled)

Image: **android-x86 9.0-r2** (Android 9), ISO `android-x86_64-9.0-r2.iso`, 921 MB,
SHA-256 `f7eb8fc56f29ad5432335dc054183acf086c539f3990f0b6e9ff58bd6df4604e`. Installed
unattended to `work/base.qcow2` (16 GiB virtual, ~1.22 GiB used); `work/base-clean.qcow2`
is a pristine copy. BlissOS was **not** installed (see open questions).

| # | Criterion | Result |
| --- | --- | --- |
| 1 | boots headless under WHPX | **Pass.** `sys.boot_completed=1` |
| 2 | one H.264 frame received, decoded, saved as PNG | **Pass.** 720x540 h264, first frame after 2.06 s |
| 3 | one tap via the control socket visible in the guest | **Pass.** 2 taps -> exactly 4 fresh MotionEvents in the guest InputDispatcher `RecentQueue` |
| 4 | two instances from qcow2 overlays at once | **Pass.** Instance b booted while a ran; frame from b after 0.83 s |

| Measurement | Value |
| --- | --- |
| Guest boot, launch -> `sys.boot_completed=1` | 51.6 s |
| Manual network step (`spike/fixnet.py`) | 10.2 s |
| Launch -> ADB reports the device (guest + manual step + connect) | 61.9 s |
| Overlay creation | 0.09 s |
| First frame after `probe` start | 2.06 s (a), 0.83 s (b) |
| Tap-to-pixel latency, host side, 12 taps | median 63.0 ms, p10 47, p90 78, min 47, max 94 |
| Idle CPU per instance, 60 s | 1.9% / 0.9% of one core |
| Idle RAM (QEMU working set) per instance, `-m 3072` | 3140 MB |

The earlier "131 s" boot figure was wrong: it included the manual network step and the
`adb connect` retry loop. Splitting guest boot from the manual step with
`spike/boottiming.py` gives the 51.6 s / 10.2 s / 61.9 s rows above.

### Results under Hyper-V + WSL2 (repeat of all four criteria)

All four criteria were repeated twice with Hyper-V, WSL2 and VMP active: run 1 with Ubuntu
WSL2 idle, run 2 with Ubuntu under CPU load (6 x `sha1sum /dev/zero`). Both runs pass all
four criteria. The acceleration/CPU matrix was re-measured first and was identical to the
baseline: every combination starts except `whpx` + `-cpu max` + `kernel-irqchip=off`.

Run 1, Ubuntu idle:

| Instance | Guest boot | ADB ready | First frame |
| --- | --- | --- | --- |
| a2 | 48.9 s | 61.3 s (manual step 10.3 s) | 2.09 s |
| b2 (a2 running) | 48.6 s | 59.0 s | 0.83 s |

Run 2, Ubuntu under load:

| Instance | Guest boot | ADB ready | First frame |
| --- | --- | --- | --- |
| a3 | 54.7 s | 65.1 s | 0.92 s |
| b3 (a3 running) | 54.6 s | 65.0 s | 1.08 s |

| Measurement | Run 1 (idle) | Run 2 (load) |
| --- | --- | --- |
| Tap-to-pixel, host, 12 taps | median 54.5 ms, p10 31, p90 63, min 31, max 63 | median 62.0 ms, p10 47, p90 78, min 47, max 500 (one first-tap outlier) |
| Tap delivery in the guest | 4 fresh MotionEvents | 6 fresh MotionEvents |
| Idle CPU per instance | a2 1.6%, b2 2.0% of one core | a3 2.1%, b3 1.8% of one core |
| Idle RAM per instance | ~3140 MB | ~3130 MB |

Artifacts: `probe-hv/`, `probe-hv-b/` (run 1), `probe-hv2/`, `probe-hv2-b/` (run 2),
each with `probe.json` and, for the a instances, `latency.json`.

Conclusion: Hyper-V/WSL2 coexistence does not change any criterion. Guest boot is
49-55 s depending on Ubuntu load; ADB-ready is 59-65 s; latency stays near the 100 ms
budget. All four criteria hold.

## Findings

1. **CPU model under WHPX.** On this AMD host `-cpu max` crashes QEMU
   (`failed to get xsave state`, then `WHPX: Unexpected VP exit code 4`). The default
   `qemu64`, which lacks SSE4.2, leaves Android 9 x86_64 on a black screen.
   `qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt` works and was used for every result. The same
   matrix was re-run with Hyper-V active with the same outcome. CPU flags must be a per-VM
   setting and still need testing on Intel (the WHPX-quirks risk in `PLAN.md`).
2. **Guest networking, cause and fix (task 7).** android-x86's `/system/etc/init.sh`
   (lines 43-59) reads `net.virt_wifi` (default `eth0`) and, unless `VIRT_WIFI` is `0`,
   renames `eth0` to `wifi_eth` and creates a `virt_wifi` `wlan0` on top. Android 9's
   `EthernetTracker` interface filter is `eth\d`, so it cannot manage `wifi_eth`, and no
   DHCP client runs on it: the guest comes up with no IP. Android 9 then resolves default
   traffic through policy-routing tables 97/98/99 (`local_network`, `legacy_network`,
   `legacy_system`), which `netd` leaves empty, so `adb` sees the device as `offline` even
   though `adbd` is alive on `:::5555`.
   - The manual workaround (`spike/fixnet.py`) puts a static address on `wifi_eth` and adds
     routes to tables 97/98/99; it works but must run on every boot.
   - A cleaner fix was verified on a throwaway image: `init.sh` (lines 532-548) `eval`s the
     kernel command-line `KEY=VALUE` tokens but skips any key containing a dot, so adding
     `VIRT_WIFI=0` (no dot) to the GRUB kernel line disables the rename. With `eth0` kept,
     Android's own Ethernet stack runs DHCP, gets `10.0.2.15`, installs the default route in
     table `eth0` (score 70) and ADB connects with **no manual step**.
   - `adb_keys`: the image has no `/data/misc/adb/adb_keys` and the authorization dialog
     never appears on Android-x86, so the host key was written over the root serial console
     and persists in `base.qcow2`.
   - NIC models present in this QEMU build (`qemu-system-x86_64 -device help`):
     `virtio-net-pci`, `e1000`, `e1000e`, `i8255x`, `rtl8139` and others.
   The choice of method (a `VIRT_WIFI=0` kernel line versus a baked-in boot hook) is the
   maintainer's; both are recorded here so M0 can pick one.
3. **Tap verification.** `probe`'s own check "did any frame change after the tap" gave a
   false positive on home-screen animation, so the earlier 156/172 ms figures did not
   measure tap latency. The valid check reads the guest InputDispatcher `RecentQueue`
   (a 10-event ring buffer mixing Motion- and KeyEvents), comparing event *ages* rather
   than the queue length, and the latency number comes from `spike/taplatency.py`, which
   requires a change within 60 px of the tap. Both `spike/tapverify.py` and
   `spike/taplatency.py` had to be hardened: the queue block also contains
   `DeviceResetEvent`/`ConfigurationChangedEvent` lines, so the parser now captures every
   4-space-indented line instead of assuming the block starts with a Motion/KeyEvent.
4. **Boot time versus the 45 s target.** Guest boot is 48.6-54.7 s, i.e. slightly over the
   proposed 45 s, and the ADB-ready figure adds the per-boot network step. This is not a
   go-criterion failure (the criterion is "boots"), but the budget in `PLAN.md` is not
   loosened here. Per `AGENTS.md` the budget is rebased in the M0 ADR with the maintainer;
   the `VIRT_WIFI=0` fix removes the manual step from the critical path and should bring
   ADB-ready close to the guest-boot figure.
5. **Idle RAM.** The QEMU working set is almost exactly the guest RAM, so it says nothing
   about the 200 MB host-overhead target. Measuring host overhead needs a split between
   guest RAM and everything else (for example RSS minus `-m`), or the rsdroid processes on
   their own once they exist.
6. **Coexistence cost.** With Hyper-V/WSL2 active and Ubuntu under full CPU load, guest
   boot grows by ~6 s and latency by ~8 ms; CPU and RAM are unchanged. No stability
   problem was seen; all four criteria still pass.

## Decision

Recommendation: **go.**

All four go criteria pass, on the WHPX baseline and again with Hyper-V, WSL2 and VMP
active (twice: Ubuntu idle and Ubuntu under load). The QEMU + WHPX + scrcpy-server
architecture in `PLAN.md` is therefore feasible on the target platform.

Three things must be settled before or during M1 and are not themselves go criteria:

- the base image needs working guest networking and ADB authorization at boot; the
  `VIRT_WIFI=0` kernel line is verified to do the network half (finding 2);
- QEMU CPU flags must be a per-VM setting, default
  `qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt`, with a `doctor` hint for the black-screen case
  (finding 1);
- the performance budget is rebased on the measured numbers in M0, and the boot-time
  target is met by tuning or changed by an ADR with the maintainer (finding 4).

Because every criterion passes, the stop alternatives (another image, the Android Emulator
as a backend, a different acceleration path) are not needed.

## Consequences

- The base image choice in M0 must include how the guest gets its network and ADB keys. A
  reproducible image build (planned for M5) is likely needed earlier, or at least a minimal
  boot-time change baked into the image.
- `rsdroid-vm` exposes the CPU flag set in its typed config and `doctor` should say what to
  try when the guest hangs on a black screen.
- Tap latency and host overhead need new measurements before they can enter the budget; the
  spike's frame-change check must not be reused as a latency measure.
- The spike is Python, not Rust, and QEMU/`adb.exe` are not pinned or checksummed here;
  that is M0 work.

## Open questions

- BlissOS v14.10.3 (SHA-256 `97a59efca5bd675d1b062931cd3642d8262bac392e9adfba63a38fb43ff0f255`)
  was not installed. It is not needed to accept this ADR, but the base image choice is the
  maintainer's at M0, and Android 9 is old; a time-boxed boot test of BlissOS at the start
  of M0 would inform that choice.
- Which guest-networking method to standardise on (the `VIRT_WIFI=0` kernel line, a
  baked-in boot hook, or another approach) is left to the maintainer.
