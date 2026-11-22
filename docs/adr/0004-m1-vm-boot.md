# ADR 0004: M1 VM boot strategy

Status: **Accepted** on 2026-11-22. The maintainer chose an installed QEMU; the live
ISO boot was verified against the pinned image and QEMU 11.1.0.

## Context

M1 must boot one VM headless, report `sys.boot_completed=1`, control QEMU through QMP
and leave no QEMU process behind when the CLI dies. ADR 0001 proved the architecture on
an installed `base.qcow2`, but that disk was spike scratch space and is not part of the
repository or the M0 dependency cache. M0 pinned the Android-x86 9.0-r2 ISO instead.

Booting the ISO through its own GRUB menu cannot pass `VIRT_WIFI=0`: QEMU can add a
kernel command line only with `-kernel`/`-append`, not when the guest firmware boots the
ISO. Without `VIRT_WIFI=0`, Android-x86 renames `eth0` to `wifi_eth`, gets no DHCP lease
and adb reports the device offline (ADR 0001, finding 2).

## Decision

- **QEMU:** use the installed QEMU 11.1.0 build. M1 does not bundle QEMU. The bundling
  decision moves to M7, when the installer and GPL source-offer obligations are designed.
- **Guest:** boot the pinned Android-x86 ISO live. `kernel` and `initrd.img` are extracted
  from the verified ISO into `%LOCALAPPDATA%\rsdroid\images\android-x86_64-9.0-r2\`, and
  QEMU receives:
  `-kernel kernel -initrd initrd.img -append "root=/dev/ram0 quiet VIRT_WIFI=0 SRC= DATA="`.
  No installed base image or qcow2 overlay is used in M1.
- **Control:** QMP listens on `127.0.0.1` only. `run` holds the QEMU child in a Windows
  Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; `stop` sends `system_powerdown`,
  waits, then falls back to `quit`.
- **ADB:** M1 wraps `adb.exe` only to poll `sys.boot_completed`. M2 still writes the ADR
  that decides between a maintained wrapper and a native ADB client.
- **Extraction:** `tools/extract-boot-files.ps1` uses 7-Zip or NanaZip and leaves the ISO
  untouched. The extracted files are a cache, not source: the verified ISO remains the
  source of truth.

## Verification

Measured on the reference development machine (4 logical cores, 8 GB RAM, WHPX) with
QEMU 11.1.0 and the pinned ISO:

| Check | Result |
| --- | --- |
| `rsdroid run demo` | booted in 44.9 s and 46.6 s; `sys.boot_completed=1` |
| `rsdroid stop demo` | QMP `system_powerdown`, QEMU exited with code 0, state removed |
| Hard kill of the `rsdroid` CLI | QEMU terminated by the Job Object |
| `qemu-system-x86_64 -help` | every flag in `rsdroid-vm` is present; `sdl` is in `-display help` |
| Guest networking | with `VIRT_WIFI=0`, adb reports `device` without a manual step |

The live boot is slower than the installed image in ADR 0001 on its faster host, but it
stays inside the 60 s boot budget and needs no installer automation in M1.

## Consequences

- `rsdroid run` needs extracted boot files. Missing files produce an actionable error
  naming `tools/extract-boot-files.ps1`.
- Live mode is ephemeral: Android data does not persist across runs. M3 adds qcow2
  overlays and M5 builds a reproducible installed image; M1 does not pretend to have one.
- The QEMU dependency remains a normal application install. A clean machine still needs
  the pinned installer from `tools/fetch-deps.ps1`; the script does not run it.
- The Job Object is the hard-kill guarantee. A graceful `system_powerdown` is the normal
  path and is exercised by `rsdroid stop`.
- Extracting the same kernel/initrd is deterministic for the pinned ISO. The cached files
  can be regenerated with `-Force`.
