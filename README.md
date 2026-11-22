# rsdroid

Open-source, ad-free, multi-window Android emulator for Windows, written in Rust.

rsdroid runs Android in QEMU with Windows Hypervisor Platform (WHPX) acceleration,
streams video and input through a pinned scrcpy-server, and shows every instance in
its own native window with keyboard, mouse and gamepad mapping. No accounts, no ads,
no telemetry and no bundled store.

## Status

Milestone **M0 (foundations)**. The workspace, CI and `rsdroid doctor` exist. There
is no VM launcher yet: `rsdroid doctor` is the only useful command until M1.

See [`PLAN.md`](PLAN.md) for the roadmap and [`docs/adr/`](docs/adr/) for decisions.

## Requirements

- Windows 10 22H2 (build 19045) or Windows 11, x86_64.
- Windows Hypervisor Platform enabled (the doctor explains how).
- Rust **1.100.0** with the MSVC host; [`rust-toolchain.toml`](rust-toolchain.toml)
  pins it. Install Visual Studio Build Tools with the "Desktop development with C++"
  workload.
- PowerShell 7 (`pwsh`) for the dependency script. Windows PowerShell 5.1 works too.

## Build and run

```powershell
git clone https://github.com/n0uzer/rsdroid.git
cd rsdroid
cargo build --workspace
cargo run -p rsdroid-cli -- doctor
```

`doctor` exits 0 when the environment is usable and 5 when a blocking problem
exists, so scripts can gate on it:

```powershell
cargo run -p rsdroid-cli -- doctor --json
cargo run -p rsdroid-cli -- doctor --bundle --bundle-dir .\reports
```

The bundle is a local ZIP with the report and existing logs. It is never uploaded;
review it before attaching it to an issue.

## Pinned dependencies

QEMU, scrcpy-server and the Android-x86 image are not stored in git. Versions, URLs
and checksums live in [`tools/deps.json`](tools/deps.json):

```powershell
# Download and verify everything into %LOCALAPPDATA%\rsdroid
pwsh tools/fetch-deps.ps1

# Just QEMU and scrcpy-server
pwsh tools/fetch-deps.ps1 -Component qemu,scrcpy-server

# Verify what is already on disk
pwsh tools/fetch-deps.ps1 -CheckOnly
```

QEMU is downloaded and verified but not executed; install it to
`C:\Program Files\qemu` or pass `--qemu <path>` to `doctor`.

## Repository layout

| Path | Purpose |
| --- | --- |
| `crates/rsdroid-core` | instance model, config, events, diagnostics |
| `crates/rsdroid-vm` | QEMU launch, QMP, qcow2 overlays, Job Objects |
| `crates/rsdroid-adb` | ADB client, port allocation, boot waits |
| `crates/rsdroid-scrcpy` | server deployment, video and control protocol |
| `crates/rsdroid-decode` | H.264 decoder trait and backends |
| `crates/rsdroid-render` | winit window and wgpu video surface |
| `crates/rsdroid-keymap` | profiles, mapping engine, gamepad |
| `crates/rsdroid-ipc` | JSON-RPC API shared by GUI, CLI and scripts |
| `crates/rsdroid-cli` | the `rsdroid` command |
| `crates/rsdroid-app` | manager UI binary |
| `tools/` | pinned dependency fetch and verification |
| `docs/adr/` | architecture decision records |

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md). CI must be
green, every significant decision needs an ADR, and large binaries never enter git.

## License

rsdroid is licensed under the **GNU General Public License v3.0 or later**
(GPL-3.0-or-later). See [`LICENSE`](LICENSE). Third-party components and their
licenses are listed in [`THIRD_PARTY.md`](THIRD_PARTY.md).
