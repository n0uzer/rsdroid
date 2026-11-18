# AGENTS.md: rsdroid

Instructions for AI coding agents working in this repository. The roadmap and milestones live in `PLAN.md`; this file explains how to work.

## Project in one paragraph

rsdroid is an open-source, ad-free, multi-window Android emulator for Windows, written in Rust. It runs Android-x86 or BlissOS in QEMU with WHPX acceleration, streams video and input through scrcpy-server, and shows each instance in its own native window with keyboard, mouse and gamepad mapping.

## Commands

These exist once milestone M0 is done. Until then, creating them is part of M0.

- Build: `cargo build --workspace`
- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Test: `cargo test --workspace`
- VM tests (need WHPX, run manually): `cargo test --workspace -- --ignored`
- Fetch pinned tools and images: `pwsh tools/fetch-deps.ps1`
- Environment check: `cargo run -p rsdroid-cli -- doctor`

## Repository map

- `crates/rsdroid-core`: instance model, TOML config, events, manager
- `crates/rsdroid-vm`: QEMU launch, QMP client, qcow2 overlays, Job Object
- `crates/rsdroid-adb`: ADB client, port allocation, wait-for-device
- `crates/rsdroid-scrcpy`: server deploy, video stream parsing, control messages
- `crates/rsdroid-decode`: H.264 decoder trait and backends
- `crates/rsdroid-render`: winit window and wgpu video surface
- `crates/rsdroid-keymap`: profiles, mapping engine, gamepad
- `crates/rsdroid-app`: egui manager UI (the GUI binary)
- `crates/rsdroid-ipc`: JSON-RPC API types, named-pipe transport, OpenRPC schema
- `crates/rsdroid-cli`: the `rsdroid` command (clap), a thin API client
- `tools/`: dependency fetch scripts
- `docs/adr/`: architecture decision records

## Code conventions

- Stable Rust, edition 2021 or newer, one workspace, no nightly features.
- Libraries use `thiserror`; `anyhow` is allowed only in `rsdroid-app`.
- Logging goes through `tracing`. Never use `println!` for diagnostics in library crates.
- No `unwrap()` or `expect()` in non-test code unless a comment explains why it cannot fail.
- Keep platform-specific code (Win32, WHPX, Job Objects) behind small modules with `cfg(windows)` so the rest stays testable.
- Pure logic (config, protocol parsing, keymap resolution) must have unit tests. QEMU and ADB are faked behind traits in tests.
- Commit messages follow Conventional Commits. Keep commits small and focused.

## Working rules

- Follow `PLAN.md` milestone by milestone. Do not start a milestone until the previous acceptance criteria pass.
- Verify uncertain facts against the real tool or source before coding: run `qemu-system-x86_64 -help`, read the pinned scrcpy-server source, check crate docs. Never invent QEMU flags, protocol fields or crate APIs.
- Record every significant decision as a short ADR in `docs/adr/` (context, decision, consequences).
- End each milestone with a tagged commit and a `CHANGELOG.md` entry.
- If you are blocked after several honest attempts, stop, write down what you tried and observed, and ask the human.

## Starting a task

Every task or GitHub issue (`.github/ISSUE_TEMPLATE/task.yml`) has the same five parts:

1. **Goal:** one sentence.
2. **Context:** the files, ADRs and `PLAN.md` sections to read first.
3. **Constraints:** anything from the Boundaries section that applies.
4. **Acceptance criteria:** checks you can run, not opinions.
5. **Out of scope:** what not to touch.

If a brief has no acceptance criteria, derive them from `PLAN.md`, state them back in your first message, and only then start coding. Kickoff prompts for each milestone are at the end of `PLAN.md`.

## Final report format

End every task with these five items, in this order:

1. **Changed:** files and crates touched.
2. **Verified:** the exact commands run and their results.
3. **Not done:** anything skipped or only partly working.
4. **Decisions needed:** questions for the human, with your recommendation.
5. **Follow-ups:** issues worth opening.

## Performance budget

- The budget (boot time, clone time, host overhead, idle CPU, fps, input latency) is defined in `PLAN.md` and fixed by an ADR in M0.
- For changes to the decode, render, input or VM configuration path, run `rsdroid bench` and put before and after numbers in the pull request.
- Avoid per-frame allocations and locks held across frames in the decode and render path.
- Never loosen a budget or the baseline file to make a check pass. Propose a change through an ADR and ask the human.

## CLI and automation

- The GUI, the `rsdroid` CLI and user scripts are thin clients of one JSON-RPC API in `rsdroid-ipc`. A new feature starts as an API method with a golden contract test, then gets a CLI command and a GUI control. Never put business logic in `rsdroid-cli` or in GUI event handlers.
- Every new CLI command supports `--json`, uses the stable exit codes defined in `PLAN.md`, and has an `assert_cmd` test against the fake backend.
- Changing an API method or its schema means regenerating `docs/api/openrpc.json` and bumping the API version. CI fails otherwise.
- The API listens on a named pipe limited to the current user. Never add a non-loopback listener, an unauthenticated TCP endpoint or any remote-control feature.
- Treat all API input as untrusted: validate instance names, paths and sizes.
- Macros and scripts run only when the user invokes them. Do not add auto-run or background scripting.
- Ask the human before embedding a scripting language, and write an ADR first. This is not planned before v0.1.
- New crates `rsdroid-ipc` and `rsdroid-cli` are added at M0 as stubs and filled in as the milestones in `PLAN.md` require.

## Boundaries

### Always

- Run `cargo fmt`, `cargo clippy` and `cargo test` before every commit.
- Pin every external download by version and SHA-256 checksum.
- Update `THIRD_PARTY.md` when adding or changing a dependency.
- Bind QMP, ADB and scrcpy listeners to `127.0.0.1` only.
- Keep all QEMU child processes inside the Windows Job Object.
- Store instance data under `%LOCALAPPDATA%/rsdroid` and never touch anything outside it.

### Ask the human first

- Project name, license and base image choice (M0).
- Bundling QEMU versus requiring a user install (M1).
- Wrapping `adb.exe` versus a native ADB implementation, and the decoder backend (M2).
- Which GPU acceleration path to pursue after the M6 spike.
- Adding a dependency with a non-permissive license.
- Any feature that makes a network request, including update checks.
- Changing a public API shared between crates in a breaking way.

### Never

- Add telemetry, analytics, ads or any phone-home behavior.
- Commit binaries, disk images, keys or credentials. Large files are fetched, not stored.
- Bundle Google Play Services, the Play Store or proprietary ARM translators.
- Weaken, skip or delete a test to make a build pass.
- Kill processes or modify VMs that rsdroid did not create.
- Hide a failure: report errors honestly in logs, ADRs and the final summary.

## Platform notes

- Target: Windows 10 22H2 and Windows 11, x86_64, with the Windows Hypervisor Platform enabled. It must coexist with Hyper-V and WSL2.
- GitHub-hosted Windows runners most likely cannot run WHPX guests. VM tests are marked `#[ignore]` and run manually or on a self-hosted runner.
- The scrcpy protocol changes between releases. Use the pinned version only, and keep recorded-stream tests for the parser.
- QEMU is GPLv2. Any bundled build needs license texts and a source offer.

## CI and releases (GitHub)

- CI lives in `.github/workflows/ci.yml` and must be green before any merge. Never edit a workflow to skip, loosen or silence a check; fix the cause instead.
- Pin every third-party action to a full commit SHA, and keep `permissions` at `contents: read` unless a job truly needs more.
- Never use `pull_request_target` to run code from a pull request. Never print secrets or pass them to steps that run fork code.
- Self-hosted runners (the `whpx` smoke test) run only on `schedule` or `workflow_dispatch`, never for fork pull requests.
- You may push branches and open pull requests. Do not push release tags, publish GitHub Releases or touch signing configuration; the maintainer does that.
- Release notes are built from `CHANGELOG.md` and Conventional Commits, so keep both accurate.
- Ask the human before adding a new third-party action, changing workflow permissions or secrets, or changing the release pipeline.

## Definition of done for a task

1. The code builds on Windows and `fmt`, `clippy` and `test` pass.
2. New logic has tests, and new decisions have an ADR.
3. The relevant acceptance criteria in `PLAN.md` are verified, not assumed.
4. `CHANGELOG.md` and docs are updated.
5. The final summary states what was done, what was not, and what the human must decide.
