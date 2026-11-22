# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Cargo workspace with stubs for `rsdroid-core`, `rsdroid-vm`, `rsdroid-adb`,
  `rsdroid-scrcpy`, `rsdroid-decode`, `rsdroid-render`, `rsdroid-keymap`,
  `rsdroid-ipc`, `rsdroid-cli` and `rsdroid-app`.
- `rsdroid doctor` with human and JSON output, exit code 5 for environment problems
  and an optional local ZIP bundle of the report and logs.
- Windows probes for the OS version, CPU virtualization, WHPX, QEMU with the WHPX
  accelerator, adb, conflicting hypervisor software and a writable data directory.
- `tools/fetch-deps.ps1` with pinned URLs and SHA-256/SHA-512 verification for QEMU,
  scrcpy-server and Android-x86, resume support, `-CheckOnly` and `-VerifyManifest`.
- GitHub Actions CI (fmt, clippy, test, build, `cargo deny`, manifest validation),
  Dependabot, issue and pull request templates, and `SECURITY.md`.
- ADR 0001 accepted (feasibility), ADR 0002 (project foundations), ADR 0003
  (performance budget, accepted with a 60 s boot / 65 s ADB-ready gate) and ADR 0004
  (M1 uses an installed QEMU and boots the pinned Android-x86 ISO live).
- M1: `rsdroid run <instance>` boots one headless Android-x86 VM from a typed config,
  forwards ADB over loopback, waits for `sys.boot_completed=1`, holds QEMU in a
  kill-on-close Job Object and powers down through QMP.
- M1: `rsdroid stop <instance>` sends `system_powerdown`, waits for a clean shutdown and
  falls back to QMP `quit`.
- M1: a loopback QMP client with `query-status`, `system_powerdown`, `system_reset`,
  `stop`, `cont` and `quit`, plus recorded-response tests.
- M1: `tools/extract-boot-files.ps1` extracts the pinned ISO's kernel and initrd so
  `VIRT_WIFI=0` can be passed on the kernel command line.
- Shared tool discovery in `rsdroid-core` so `doctor`, `run` and `stop` agree on the
  QEMU and adb paths.

### Changed

- Adopted the name **rsdroid**, the GPL-3.0-or-later license and Android-x86 9.0-r2
  as the base image.

[Unreleased]: https://github.com/n0uzer/rsdroid/commits/main
