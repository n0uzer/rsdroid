# M-1 feasibility spike: runbook (Windows 11)

Throwaway code for `PLAN.md` milestone M-1. It lives on the `spike/m-1-feasibility`
branch and is **never merged**. Its job is to prove four assumptions and produce the
numbers for `docs/adr/0001-feasibility.md`.

Go criteria (all four must hold):

1. Android boots headless under QEMU with `-accel whpx`.
2. One H.264 frame is received from scrcpy-server, decoded and saved as PNG.
3. One tap injected through the scrcpy control socket is visible in the guest.
4. Two instances run at the same time from qcow2 overlays.

Also repeat everything with Hyper-V and WSL2 enabled, and record the measurements.

## What was verified before you start, and what was not

| Item | Status |
| --- | --- |
| scrcpy-server v5.0.1 URL and SHA-256 | Verified (downloaded, hash matches `doc/build.md`) |
| scrcpy wire format (dummy byte, 64-byte device name, codec id, 12-byte session packet, 12-byte frame header, 32-byte touch message) | Read from the v5.0.1 source; touch bytes match the reference C test |
| Probe pipeline (parse, decode, PNG, tap) | Tested against `fake_scrcpy_server.py`, **not** against a real device |
| QEMU flags: `-machine q35`, `-display none`, `-qmp tcp:...,server=on,wait=off`, `hostfwd`, qcow2 overlay | Verified on QEMU 8.2.2 (Linux, TCG) |
| `-accel whpx`, `kernel-irqchip`, which `-cpu` Android needs, `ide` vs `virtio` disk, whether adbd listens on TCP | **Not verified.** Candidates below, you find out which work |

Do not treat the candidates as facts. Record what actually boots.

## Prerequisites

- Windows 11, virtualization enabled in firmware, Windows Hypervisor Platform enabled
  (`optionalfeatures.exe` > Windows Hypervisor Platform).
- QEMU for Windows. Note the version and where you got it. Check what it offers:
  `qemu-system-x86_64 -accel help`, and read the QEMU documentation for the WHPX options
  of that exact build before trying any `whpx,...` variant.
- Android platform-tools (`adb.exe`) on `PATH`. Note `adb version`.
- Python 3.10+ and `pip install av pillow psutil`.
- `scrcpy-server-v5.0.1`, from
  `https://github.com/Genymobile/scrcpy/releases/download/v5.0.1/scrcpy-server-v5.0.1`
  (do not rename it; `probe` refuses a file whose SHA-256 is not
  `764eb6f79811d5211fe9df341120882ba9994c7a61b897d7bf3fb662e53bc536`).
- A BlissOS or Android-x86 ISO. Note its name, version and `Get-FileHash` SHA-256.
  The base image is a decision for the maintainer at M0, so ideally try both.

Self-test of the helper (no guest needed): `python -m unittest discover -s spike -v`.

All commands below run from the repository root. `--workdir work` is where disks,
logs, pid files and the recorded command lines go.

## 1. Record the environment

CPU model, RAM, Windows build (`winver`), QEMU version, adb version, and whether
Hyper-V and WSL2 are on (`Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All`,
`wsl --status`).

## 2. Install the guest into the base image

```
python spike/spike.py disk --workdir work --size 16G
python spike/spike.py launch --workdir work --name install --iso C:\path\to\image.iso --display sdl --accel whpx
```

Install to `work\base.qcow2` with the guest installer, power off, then **never boot
`base.qcow2` directly again**: it is the read-only base for the overlays.

Find the working combination and record every extra option. Candidates to try, one at
a time (add with `--accel`, `--cpu`, `--disk-if`, `--vga`, `--extra`):

- `--accel whpx`, then `--accel whpx,kernel-irqchip=off`
- no `--cpu` (on one AMD host the guest then hung on a black screen), then
  `--cpu qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt` (worked there), then `--cpu max` (crashed
  QEMU under WHPX on that host: `failed to get xsave state`)
- `--disk-if ide` (AHCI on q35, the default), then `--disk-if virtio`
- guest kernel command-line options, if the image needs any

`launch` writes the exact command line to `work\<name>.cmdline.txt`. Copy the working
one into the ADR.

ADB over TCP: on Android-x86 9.0-r2 adbd already listened on port 5555 from early boot, so
no `setprop` was needed. What did break (see `docs/adr/0001-feasibility.md`, finding 2): the
image had no `adb_keys` and showed no authorization dialog, and the guest got no IP address,
so `adb` reported the device offline while adbd was running. If `boot` times out, check the
guest's network and ADB authorization first. Record what was needed, and make it persistent
in the base image before the final power off, if possible.

## 3. Boot one headless instance (criterion 1)

```
python spike/spike.py disk --workdir work --overlay a
python spike/spike.py launch --workdir work --name a --accel whpx --adb-port 15555 --qmp-port 14444
python spike/spike.py boot --workdir work --name a --serial 127.0.0.1:15555
```

`boot` prints seconds from the `launch` call to `sys.boot_completed == 1`. That is the
cold boot number. Clean shutdown: `python spike/spike.py qmp --qmp-port 14444 powerdown`.
Hard stop: `... qmp --qmp-port 14444 quit`.

If the guest hangs, relaunch with `--display sdl` to see the console.

## 4. Frame and tap through scrcpy (criteria 2 and 3)

```
python spike/spike.py probe --workdir work --name a --serial 127.0.0.1:15555 --server scrcpy-server-v5.0.1 --out probe-a
```

It pushes the server, turns on the Android pointer-location overlay, starts the server
with `tunnel_forward=true`, receives frames, saves `frame0.png`, sends DOWN and UP at the
screen centre and saves `after_tap.png` when the picture changes. Check `after_tap.png`
shows the touch crosshair and coordinates near the centre. `probe.json` holds device name,
frame size, time to first frame and a tap-to-changed-frame time. **Do not trust that time
or the "frame changed" result as proof of a tap**: any animation on the home screen counts
as a change, which gave a false positive in the first Windows run. Confirm the tap in the
guest instead, for example by counting new MotionEvents in the InputDispatcher
`RecentQueue` (a ring buffer of 10 events, so compare event ages, not its length) before
and after. `server.log` holds the server output; keep it if anything fails.

## 5. Two instances (criterion 4)

```
python spike/spike.py disk --workdir work --overlay b
python spike/spike.py launch --workdir work --name b --accel whpx --adb-port 15556 --qmp-port 14445
python spike/spike.py boot --workdir work --name b --serial 127.0.0.1:15556
python spike/spike.py probe --workdir work --name b --serial 127.0.0.1:15556 --server scrcpy-server-v5.0.1 --out probe-b
```

Keep `a` running. Time how long `disk --overlay` takes (it prints it): this seeds the
clone-time budget.

## 6. Measurements

With the guest at the idle home screen for a minute, per instance:

```
python spike/spike.py measure --workdir work --name a --seconds 60
```

Idle RAM here is the QEMU process working set, so it includes guest RAM. The M0 budget
separates guest RAM from host overhead; note both `--ram` and the result.

## 7. Hyper-V and WSL2

Turn both on, reboot, check with the commands from step 1, then repeat steps 3 to 6.
Record any difference in boot time, CPU use or stability.

## 8. Write the ADR

Fill in `docs/adr/0001-feasibility.md` (a draft is already there with the verified facts).
If any go criterion fails, write down the alternatives (another image, the Android Emulator
as a backend, a different acceleration path) and stop for the maintainer before M0.

## Known limits of this spike

- No Job Object. If a script dies, QEMU keeps running: `Get-Process qemu*`, or `qmp quit`.
- `adb.exe` and QEMU are not pinned or checksummed here. That is M0 work.
- Python instead of Rust. A decision for the maintainer is listed in the ADR.
- `probe` sets `settings put system pointer_location 1` on the guest and leaves it on.
