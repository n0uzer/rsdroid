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
- ADR 0001 accepted (feasibility), ADR 0002 (project foundations) and ADR 0003
  (performance budget).

### Changed

- Adopted the name **rsdroid**, the GPL-3.0-or-later license and Android-x86 9.0-r2
  as the base image.

[Unreleased]: https://github.com/n0uzer/rsdroid/commits/main
