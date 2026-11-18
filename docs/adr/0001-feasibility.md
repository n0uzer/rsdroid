# ADR 0001: Feasibility of QEMU + WHPX + scrcpy-server

Status: **Draft.** The four go criteria pass on the WHPX baseline (Hyper-V off). The repeat
with Hyper-V, WSL2 and Virtual Machine Platform enabled is still pending a reboot, so this
ADR is not accepted yet. Sections marked TBD are still open.

## Context

`PLAN.md` M-1 asks to prove four go criteria before any workspace exists: Android boots
under WHPX, one frame is received and decoded, one tap works, two instances coexist.
The spike code (`spike/`) lives on `spike/m-1-feasibility` and is not merged.

## Facts verified so far

Verified in a Linux sandbox, not on the target platform.

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
    name.
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

Checked against QEMU 8.2.2 on Linux with TCG: `-machine q35`, `-cpu qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt`,
`-drive ...,if=ide|virtio,format=qcow2`, `-netdev user,...,hostfwd=tcp:127.0.0.1:P-:5555`,
`-qmp tcp:127.0.0.1:P,server=on,wait=off`, `-pidfile`, `-display none`, and
`qemu-img create -f qcow2 -b base -F qcow2 overlay`. QMP `query-status`, `cont`, `stop` and
`quit` work; QEMU drops the connection on `quit` without replying.

**Not verified:** `-accel whpx` and its options exist only in Windows builds.

### Spike pipeline

`spike/test_spike.py` passes against a fake server that speaks the format above and serves
real H.264 produced by ffmpeg: handshake, session and config packets, decode to PNG, touch
DOWN and UP received by the server. This shows the client agrees with our reading of the
protocol. It does not show that a real Android guest produces such a stream.

## Results (Windows, WHPX baseline)

Source: the maintainer's own run of `spike/README.md`, reported as text. The artifacts
(`probe-a/`, `probe-b/`, `work/notes.md`, `work/NEXT.md`, `work/fixnet.py`,
`work/tapverify.py`) were not reviewed when this section was written; the figures below are
copied from that report. Hyper-V was off and WHPX on.

| Item | Result |
| --- | --- |
| QEMU build and version | Not reported (TBD) |
| Image tried | Android-x86 9.0-r2 (Android 9). SHA-256 not reported (TBD). BlissOS not tried |
| Criterion 1: boots headless under WHPX | **Pass.** `sys.boot_completed` = 1 after 131.0 s |
| Criterion 2: frame received, decoded, saved as PNG | **Pass.** 720x540 h264, first frame after 2.06 s |
| Criterion 3: tap observed in the guest | **Pass.** Two taps produced exactly 4 new MotionEvents (DOWN and UP each) in the guest's InputDispatcher `RecentQueue`. See finding 3 |
| Criterion 4: two instances from overlays at once | **Pass.** Instance b booted in 135.3 s while a was running |
| Same with Hyper-V, WSL2 and Virtual Machine Platform enabled | **Pending.** Features are enabled but need a reboot (VMP showed `EnablePending`) |

Working command line, as reported (the report abbreviates the QMP options; the full line is
in `work/a.cmdline.txt`):

```
-machine q35 -accel whpx -cpu qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt -smp 2 -m 3072
-drive file=work\a.qcow2,if=ide,format=qcow2 -netdev user,id=n0,hostfwd=tcp:127.0.0.1:15555-:5555
-device virtio-net-pci,netdev=n0 -vga std -display none -qmp tcp:127.0.0.1:14444,...
```

| Measurement | Value |
| --- | --- |
| Cold boot to `sys.boot_completed` | 131.0 s and 135.3 s (a, b). Proposed target in `PLAN.md`: 45 s. See finding 4 |
| Overlay creation time | 0.09 s |
| Idle RAM per instance | 3140 MB RSS with `--ram 3072`. Includes guest RAM, so it says nothing about the 200 MB host-overhead target |
| Idle CPU per instance | 1.9% and 0.9% of one core (two figures reported, presumably a and b) |
| Tap to changed frame, host side | 156 ms and 172 ms. **Not a valid latency**, see finding 3 |
| Hardware | An AMD CPU on Windows 11. Model, RAM and GPU not reported (TBD) |

## Findings

1. **CPU model under WHPX.** `-cpu max` fails on this AMD host (`failed to get xsave state`,
   then `WHPX: Unexpected VP exit code 4`). The default `qemu64`, which lacks SSE4.2, leaves
   the guest on a black screen. `qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt` works. The runbook
   listed `--cpu max` as a candidate, so it is not usable here. CPU flags must be a per-VM
   setting and need testing on Intel too (the WHPX-quirks risk in `PLAN.md`).
2. **The guest has no network, which the ADB design needs.** `adbd` listens on `:::5555`
   from early boot, so the `setprop` step in the runbook was not needed. What broke:
   - the image has no `adb_keys` and the authorization dialog never appears on Android-x86
     (how this was worked around is not stated in the report);
   - the guest gets no IP: `init.sh` renames `eth0` to `wifi_eth`, nothing runs DHCP, and
     Android 9 consults empty policy-routing tables 97, 98 and 99, so `adb` shows the device
     as offline while `adbd` is alive.

   The workaround is a static address on `wifi_eth` plus routes in tables 97/98/99
   (`work/fixnet.py`), applied by hand on every boot. `/system` is a read-only squashfs and
   the image has no standard autostart hook. The key decision "ADB through a QEMU-forwarded
   port" therefore holds only if the base image gets a way to configure its network at boot.
3. **Tap verification.** The `probe` check "did any frame change after the tap" produced a
   false positive on home-screen activity, so the 156/172 ms figures do not measure tap
   latency. The valid check reads the `RecentQueue` ring buffer (10 events) of the guest's
   InputDispatcher and compares event ages, not its length. Criterion 3 rests on that check.
   Tap-to-pixel latency still has to be measured properly (see the budget table in `PLAN.md`).
4. **Boot time is about three times the proposed target.** 131 s against 45 s. It is not
   clear how much of this is the guest and how much is measurement: `boot` waits on ADB,
   which stayed offline until the manual network fix, so the figure may include that delay.
   Per `AGENTS.md`, the budget is not loosened here; it is rebased in the M0 ADR with the
   maintainer.
5. **Idle RAM.** The QEMU working set is almost exactly the guest RAM. Measuring host
   overhead needs a split between guest RAM and everything else, for example RSS minus `-m`,
   or the rsdroid processes on their own once they exist.

## Decision

Recommendation: **go, with adjustments, and final only after the Hyper-V and WSL2 repeat.**
All four go criteria pass on the WHPX baseline, which is what the plan asks for. Three things
have to change before M1 and are not covered by the criteria:

- the base image needs working guest networking and ADB authorization at boot (finding 2);
- QEMU CPU flags must be a per-VM setting, default `qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt`
  (finding 1);
- the performance budget is rebased on measured numbers, and the boot-time target is either
  met by tuning or changed by an ADR with the maintainer (finding 4).

If the Hyper-V and WSL2 repeat fails, the verdict becomes stop, and the alternatives are
another image, the Android Emulator as a backend, or a different acceleration path.

## Consequences

- The base image choice in M0 must include how the guest gets its network and ADB keys. A
  reproducible image build (planned for M5) is likely needed earlier, or at least a minimal
  boot-time init hook baked into the image.
- `rsdroid-vm` exposes the CPU flag set in its typed config and `doctor` should say what to
  try when the guest hangs on a black screen.
- Tap latency and host overhead need new measurements before they can enter the budget.
- The spike's frame-change check must not be reused as a latency measure.

## Open questions

- Guest network fix: the report proposes trying virtio-net with standard DHCP. The working
  command line already uses `virtio-net-pci`, so which change is meant (another NIC model,
  a guest-side change, something else) needs to be clarified before it is tried.
- Does the 131 s boot figure include the manual network fix?
- BlissOS was not tried. It is not needed to accept this ADR, but the base image choice is
  the maintainer's at M0, and Android 9 is old; a time-boxed boot test of BlissOS at the
  start of M0 would inform that choice.
- QEMU version, image checksum and hardware details are missing from the report.
