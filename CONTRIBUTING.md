# Contributing to rsdroid

Thanks for helping build an open, ad-free Android emulator. This document covers
the practical steps; the roadmap is in [`PLAN.md`](PLAN.md) and the working rules
for humans and AI agents are in [`AGENTS.md`](AGENTS.md).

## Before you start

- Check [`PLAN.md`](PLAN.md) for the current milestone. Work that belongs to a later
  milestone is usually rejected, even when it is good.
- For anything larger than a typo, open an issue with the task template first. Every
  task has a goal, context, constraints, acceptance criteria and out-of-scope list.
- Significant decisions need a short ADR in [`docs/adr/`](docs/adr/) before the code
  lands.
- Ask before adding a dependency with a non-permissive license, a network request,
  a new GitHub Action, or a breaking API change.

## Setup

1. Install Rust 1.100.0 with the MSVC host. [`rust-toolchain.toml`](rust-toolchain.toml)
   selects it automatically once installed.
2. Install Visual Studio Build Tools with the "Desktop development with C++" workload
   (the MSVC linker is required).
3. Install PowerShell 7 (`pwsh`).
4. Optionally fetch the pinned external tools:

   ```powershell
   pwsh tools/fetch-deps.ps1 -Component qemu,scrcpy-server
   ```

5. Check the machine:

   ```powershell
   cargo run -p rsdroid-cli -- doctor
   ```

## Required checks

Run these before every commit; CI runs the same commands on `windows-latest`:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --locked
pwsh tools/fetch-deps.ps1 -VerifyManifest
cargo deny check
```

`cargo deny check` needs `cargo-deny` installed (`cargo install cargo-deny --locked`).
CI also runs it; the policy lives in [`deny.toml`](deny.toml).

Tests that need a real hypervisor are marked `#[ignore]` and run manually:

```powershell
cargo test --workspace -- --ignored
```

## Code conventions

- Stable Rust, edition 2021, one workspace, no nightly features. MSRV is 1.89 unless
  an ADR changes it.
- Libraries use `thiserror`. `anyhow` is allowed only in `rsdroid-app`.
- Diagnostics go through `tracing`; library crates do not print to stdout.
- No `unwrap()` or `expect()` in non-test code without a comment explaining why it
  cannot fail. `clippy.toml` allows them in tests.
- Keep Win32, WHPX and Job Object code in small `cfg(windows)` modules so the rest
  stays testable on any host.
- Pure logic needs unit tests. QEMU and ADB are faked behind traits; tests never touch
  a real VM unless they are explicitly `#[ignore]` VM tests.
- Prefer clear code over clever code. Public items need doc comments.

## Commit and pull request style

- Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`,
  `docs:`, `test:`, `refactor:`, `build:`, `ci:`, `chore:`.
- Keep commits small and focused; do not mix formatting with behavior changes.
- A pull request states what changed, how it was verified, what is not done and which
  decisions are needed.
- Update [`CHANGELOG.md`](CHANGELOG.md) for user-visible changes.
- Update [`THIRD_PARTY.md`](THIRD_PARTY.md) when adding or changing a dependency.

## Dependencies and downloads

- Pin every external download by version and SHA-256/SHA-512 checksum in
  [`tools/deps.json`](tools/deps.json). The fetch script never executes installers.
- Never commit binaries, images, disk files or credentials. `.gitignore` already
  excludes the common extensions; do not weaken it.
- New Rust dependencies must pass `cargo deny check` (licenses, advisories, sources).
  Ask before adding anything copyleft-incompatible or unmaintained.
- QEMU is GPLv2 and scrcpy-server is Apache-2.0. Any release that bundles them needs
  the corresponding license texts and, for QEMU, a source offer.

## Privacy and security

- No telemetry, analytics, ads or background phone-home. The only network calls are
  user-initiated downloads and an opt-in update check added in M7.
- Bind QMP, ADB and scrcpy listeners to `127.0.0.1` only.
- Never start a non-loopback listener, an unauthenticated TCP endpoint or anything
  that resembles remote control.
- Report vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md).

## License of contributions

By submitting a contribution you agree that it is licensed under
**GPL-3.0-or-later**, the license of this project. Do not copy code from projects
whose license is incompatible with the GPL.
