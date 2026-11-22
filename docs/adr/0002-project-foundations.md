# ADR 0002: Project foundations

Status: **Accepted** on 2026-11-22. The name, license and base image were approved by
the maintainer; the remaining items are the M0 engineering decisions that implement
those choices.

## Context

M0 creates the workspace, CI, dependency fetching and `rsdroid doctor`. Before the
workspace can be laid out, the project needs a name, a license, a base image and a
toolchain policy. ADR 0001 established that QEMU + WHPX + scrcpy-server is feasible and
recorded the exact command line, the image that was tested and the known guest fixes.
This ADR records the choices that everything else builds on.

## Decision

### Name and repository

The project is **rsdroid**, hosted at `https://github.com/n0uzer/rsdroid`. The shipped
binaries are `rsdroid` (CLI) and `rsdroid-app` (manager UI). The workspace version
starts at `0.1.0-alpha.0`; nothing is published before v0.1.

### License

Project code is licensed under **GPL-3.0-or-later**. The full text is in `LICENSE`.
Contributions are accepted under the same license; there is no CLA. Every third-party
component is tracked in `THIRD_PARTY.md`, and `cargo deny check` enforces the license
allowlist in `deny.toml`.

GPL-3.0-or-later keeps the emulator and every derivative open. QEMU is GPLv2 and runs
as a separate process, so there is no link-time conflict; a release that bundles QEMU
must still ship its license texts and a source offer.

### Base image

The base image is **Android-x86 9.0-r2** (`android-x86_64-9.0-r2.iso`, Android 9,
API 28), pinned by SHA-256 in `tools/deps.json`. It is downloaded by the user, never
committed.

The image is provisioned with the two fixes verified in ADR 0001:

- `VIRT_WIFI=0` on the kernel command line, so `eth0` keeps its name and Android's
  Ethernet stack runs DHCP; this removes the per-boot network workaround and the
  ADB-offline state.
- The host ADB public key in `/data/misc/adb/adb_keys`, written through the root
  serial console while building the base image, because the Android-x86 image has no
  authorization dialog.

Each instance will use a qcow2 overlay on this read-only base. The reproducible image
build belongs to M5; M1 may use the locally provisioned image recorded in ADR 0001.

BlissOS was not installed during M-1, so choosing it would add an untested risk with
no compatibility data. Android 9 is old, but it is the image that passed all four go
criteria. The choice is revisited when M5 produces a clean image or when a target app
requires a newer API level.

### Pinned external tools

| Tool | Pin | Why |
| --- | --- | --- |
| QEMU for Windows | 11.1.0, build 20260811 | The build verified with WHPX in ADR 0001. |
| scrcpy-server | 5.0.1 | The client and server protocol are not stable across releases. |
| Android-x86 | 9.0-r2 | The base image that passed M-1. |

QEMU is downloaded and checksum-verified by `tools/fetch-deps.ps1` but is not executed
by it. Whether rsdroid bundles QEMU or requires a user install is an M1 decision.
The default CPU model for a VM is
`qemu64,+ssse3,+sse4.1,+sse4.2,+popcnt`, with per-instance overrides, because the
plain `qemu64` model leaves Android 9 on a black screen under WHPX (ADR 0001).

### Rust and workspace

- Rust **1.100.0**, MSVC host, pinned in `rust-toolchain.toml`.
- Edition 2021, workspace MSRV 1.89 (raised by `zip` 9.x when needed).
- One virtual workspace with ten crates: the nine libraries and the manager binary
  listed in `PLAN.md`. `rsdroid-vm`, `rsdroid-adb`, `rsdroid-scrcpy`,
  `rsdroid-decode`, `rsdroid-render` and `rsdroid-keymap` are stubs in M0.
- `rsdroid-cli` is a thin client; checks and business logic live in `rsdroid-core`.
  The GUI is a client of the same API that `rsdroid-ipc` will expose in M3.
- Lints: workspace-level `clippy` and `rustc` configuration in `Cargo.toml`,
  `rustfmt.toml` and `clippy.toml`; CI treats warnings as errors.

### Dependencies and repository hygiene

- `tools/deps.json` is the single source of truth for downloaded binaries: version,
  HTTPS URL, size, SHA-256/SHA-512 and destination.
- `tools/fetch-deps.ps1` downloads, resumes, verifies and caches; it never executes a
  downloaded installer and never writes outside the chosen data root.
- No images, installers or archives in git. `.gitignore` excludes them.
- Direct Rust dependencies are listed in `THIRD_PARTY.md`; transitive licenses are
  checked by `cargo deny check`.

### Diagnostics contract

`rsdroid doctor` is the only M0 command. It emits:

- stable check ids: `windows_version`, `cpu_virtualization`, `hypervisor_platform`,
  `qemu`, `adb`, `conflicting_software`, `data_directory`;
- statuses `ok`, `warning`, `error`, `skipped`;
- exit code 0 when usable, 5 when any check is an error;
- `--json` with `schema_version: 1`, and `--bundle` for a local ZIP that is never
  uploaded.

All state lives under `%LOCALAPPDATA%\rsdroid` (override with `--data-dir` or
`RSDROID_DATA_DIR`). Nothing outside that directory is modified.

### CI and release hygiene

- GitHub Actions on `windows-latest`: `cargo fmt --check`, clippy with
  `-D warnings`, `cargo test --workspace`, `cargo build --workspace --locked`,
  `cargo deny check` and manifest validation.
- Third-party actions are pinned to full commit SHAs; the workflow defaults to
  `permissions: contents: read`.
- Dependabot watches `cargo` and `github-actions`.
- Release workflow and signing are M7; tags and releases are pushed only by the
  maintainer.

## Consequences

- M1 starts from the verified QEMU 11.1.0 command line and the provisioned Android 9
  image; changing the base image needs a new ADR.
- Android 9 limits app compatibility and lacks current security patches. The clean
  image and any API-level upgrade are explicit M5 work.
- The guest network and ADB-key fixes must be reproducible; until the M5 image build,
  M1 depends on the manual provisioning recorded in ADR 0001.
- Pinning Rust 1.100.0 means toolchain updates are deliberate, reviewed changes.
- GPL-3.0-or-later plus bundled QEMU creates concrete release obligations (license
  texts, source offer), tracked from M0 even though releases start in M7.
