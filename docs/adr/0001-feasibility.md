# ADR 0001: Feasibility of QEMU + WHPX + scrcpy-server

Status: **Draft. Measurements on Windows are pending.** Sections marked TBD must be
filled from the runbook in `spike/README.md` before this ADR is accepted.

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

## Results (TBD, Windows 11)

| Item | Result |
| --- | --- |
| QEMU build and version | TBD |
| Image(s) tried (name, version, SHA-256) | TBD |
| Working QEMU command line | TBD (copy `work/<name>.cmdline.txt`) |
| Extra options the guest needed (accel, cpu, disk bus, kernel cmdline, adbd on TCP) | TBD |
| Criterion 1: boots headless under WHPX | TBD |
| Criterion 2: frame received, decoded, saved as PNG | TBD |
| Criterion 3: tap observed in the guest | TBD |
| Criterion 4: two instances from overlays at once | TBD |
| Same with Hyper-V and WSL2 enabled | TBD |

| Measurement | Value |
| --- | --- |
| Cold boot to `sys.boot_completed` | TBD |
| Overlay creation time | TBD |
| Idle RAM (QEMU working set) per instance, with `--ram` | TBD |
| Idle CPU per instance (60 s) | TBD |
| Tap to changed frame, host side | TBD |
| Hardware (CPU, RAM, GPU) | TBD |

## Decision

TBD: go, adjust or stop. If a criterion fails, list the alternatives (another image, the
Android Emulator as a backend, a different acceleration path) and stop for the maintainer.

## Consequences

TBD. The measurements above should replace the proposed targets in the performance budget
table in `PLAN.md`, and the budget is then fixed by its own ADR in M0.

## Questions for the maintainer

- The spike helper is Python because it needs a decoder and a PNG writer quickly. Acceptable
  for throwaway code, or should the spike be redone in Rust?
- Which base image should be tried first, BlissOS or Android-x86 (final choice is M0)?
