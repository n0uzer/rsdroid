# PLAN.md: rsdroid roadmap

Working title for an open-source, ad-free, multi-window Android emulator for Windows, written in Rust.

## 0. How to use this document

This file is the roadmap: what to build and in which order. How agents work in the repository (commands, conventions, boundaries, decision gates) is described in `AGENTS.md`. Work through milestones M-1 to M7 in order, and start the next one only when the acceptance criteria of the previous one pass.

## 1. Mission and non-goals

Build a Windows desktop app that runs several isolated Android instances side by side, each in its own window, with keyboard, mouse and gamepad mapping, no ads, no telemetry and no bundled bloatware.

Non-goals for v0.x:

- Writing a hypervisor from scratch
- An ARM translation layer
- Linux or macOS support
- Bypassing game anti-cheat
- Bundling Google Play Services or the Play Store

## 2. Hard constraints

- **Language:** stable Rust for all host code. PowerShell or batch only for packaging and dependency fetching.
- **Platform:** Windows 10 22H2 and Windows 11, x86_64. Virtualization through the Windows Hypervisor Platform (WHPX); it must coexist with Hyper-V and WSL2.
- **Privacy:** no telemetry, no analytics, no ads. The only network calls are user-initiated downloads and an opt-in update check.
- **Licensing:** GPL-3.0-or-later for project code unless the maintainer decides otherwise at M0. Track every third-party dependency and its license in `THIRD_PARTY.md`. QEMU is GPLv2 and must ship with a source offer.
- **Repository hygiene:** no large binaries in git. Images and tools are fetched by a script with pinned versions and SHA-256 checksums.
- **Protocol stability:** pin the scrcpy-server version; its protocol is not stable between releases.

## 3. Architecture

```
rsdroid-app (egui manager UI + winit/wgpu instance windows)
  |-- rsdroid-core     instance model, TOML config, events, manager
  |-- rsdroid-vm       spawns qemu-system-x86_64, QMP control, qcow2 overlays
  |-- rsdroid-adb      ADB client, port allocation, wait-for-device
  |-- rsdroid-scrcpy   scrcpy-server deploy, video stream, control messages
  |-- rsdroid-decode   H.264 decoder trait + backends
  |-- rsdroid-render   winit window + wgpu video surface
  |-- rsdroid-keymap   profiles, mapping engine, gamepad (gilrs)
  |-- rsdroid-ipc      JSON-RPC API shared by the GUI, CLI and scripts
  |-- rsdroid-cli      the rsdroid command (clap), a thin API client
```

Key decisions:

1. **VM:** QEMU as a subprocess with `-accel whpx`, headless (`-display none`), ADB port forwarded to the host, QMP on localhost TCP.
2. **Video and input:** scrcpy-server is pushed over ADB and streams H.264, which the host decodes. This avoids embedding QEMU's own window and makes multi-window, overlays and key mapping straightforward.
3. **Instances:** each instance is a qcow2 overlay on top of a read-only base image, so cloning takes seconds.
4. **Lifecycle:** all QEMU processes belong to a Windows Job Object so they die with the manager.
5. **GPU acceleration** is a separate, later track (M6). Software rendering is the baseline.

## 4. Repository layout

```
rsdroid/
  Cargo.toml              workspace
  crates/
    rsdroid-core/
    rsdroid-vm/
    rsdroid-adb/
    rsdroid-scrcpy/
    rsdroid-decode/
    rsdroid-render/
    rsdroid-keymap/
    rsdroid-ipc/
    rsdroid-cli/
    rsdroid-app/          binary entry point
  tools/
    fetch-deps.ps1        pinned QEMU, scrcpy-server, base image + checksums
  docs/
    adr/                  architecture decision records
  THIRD_PARTY.md
  CONTRIBUTING.md
  CHANGELOG.md
```

## 5. Milestones

### M-1. Feasibility spike (2 days, throwaway code)

The whole plan rests on a few assumptions. Prove them before any workspace exists. The code lives on a `spike/` branch and is never merged.

Tasks:

- Boot the chosen Android-x86 or BlissOS image headless in QEMU with `-accel whpx` on Windows 11. Record the exact command line and every extra option the guest needed.
- Connect through a forwarded port with ADB and wait for `sys.boot_completed` to reach 1.
- Push the pinned scrcpy-server, start it, receive at least one H.264 frame on the host, decode it and save it as a PNG.
- Inject one tap through the scrcpy control socket and observe it in the guest.
- Start two instances from qcow2 overlays at the same time.
- Repeat with Hyper-V and WSL2 enabled.
- Measure cold boot time, idle RAM, idle CPU and rough tap-to-frame latency. These numbers seed the performance budget below.

Deliverable: `docs/adr/0001-feasibility.md` with command lines, measurements, failures, the images tried and a go, adjust or stop recommendation.

Acceptance (go criteria): Android boots under WHPX, one frame is received and decoded, one tap works, and two instances coexist. If any of these fail, write down an alternative (another image, the Android Emulator as a backend, a different acceleration path) and stop for the human before M0.

### M0. Foundations

Tasks:

- Create the workspace, crate stubs, GitHub Actions CI (see the CI and releases subsection after M7), rustfmt and clippy config, license files, `CONTRIBUTING.md` and `THIRD_PARTY.md`.
- Write `tools/fetch-deps.ps1`: download pinned QEMU for Windows, scrcpy-server and a base Android-x86 image (BlissOS or Android-x86) with checksum verification.
- Implement `rsdroid doctor`: detects whether Windows Hypervisor Platform is enabled, whether CPU virtualization is on, whether QEMU is present, and whether conflicting software is installed. Print actionable errors.

Acceptance: `cargo build` succeeds on a clean Windows machine and `rsdroid doctor` prints a correct report.

### M1. Boot one VM headless

Tasks:

- In `rsdroid-vm`, build the QEMU command line from a typed config (CPU count, RAM, disk, ports). Start from `-machine q35 -accel whpx` and verify empirically which extra options Android needs under WHPX (for example `kernel-irqchip=off`). Check every flag against `qemu-system-x86_64 -help`.
- Forward the guest ADB port (5555) to a free host port.
- Implement a QMP client over localhost TCP: `query-status`, `system_powerdown`, `system_reset`, `stop`, `cont`.
- Attach QEMU to a Job Object so it is killed when the manager exits.
- Detect boot completion with `adb shell getprop sys.boot_completed`.
- Add a `--debug-window` flag that shows QEMU's own display for troubleshooting.

Acceptance: `rsdroid run <instance>` boots Android and reports boot completed. Killing the CLI kills QEMU. A clean shutdown through QMP works.

### M2. Video and input through scrcpy

Tasks:

- `rsdroid-adb`: connect, push a file, run a shell command, set up port forwarding. An ADR decides between wrapping `adb.exe` and a native implementation.
- `rsdroid-scrcpy`: push the server, start it via `app_process`, open the video and control sockets, parse the frame stream, send touch, key and scroll control messages.
- `rsdroid-decode`: define a `Decoder` trait. Ship a software backend first (ffmpeg bindings, with LGPL dynamic-linking rules respected). Add a hardware backend (Media Foundation or D3D11VA) as an optimization. Do not use openh264 unless an ADR confirms the patent license covers the distribution path.
- `rsdroid-render`: winit window, wgpu texture upload, aspect-ratio and resize handling, mouse to touch, keyboard to keycodes, clipboard sync.

Acceptance: one window shows live Android at 30 fps or better on a mid-range laptop. Taps, typing, resizing and rotation work.

### M3. Multi-instance manager

Tasks:

- Instance model: id, name and `instance.toml` (CPU, RAM, resolution, DPI), stored next to a qcow2 overlay created with `qemu-img create -f qcow2 -b <base> -F qcow2`.
- Lifecycle: create, clone, start, stop, pause and resume (QMP `stop` and `cont`), delete, snapshot.
- A port allocator that never collides across instances.
- egui manager UI: instance list, status, resource usage, create and clone dialogs, per-instance settings.
- Each running instance gets its own window. Closing a window does not kill the VM unless configured to.
- Sync mode: broadcast input from one window to selected others.

Acceptance: four instances run concurrently on a 16 GB machine, each cloned from the same base in under 5 seconds, with isolated storage and independent windows. A manager crash leaves no orphan QEMU processes.

### M4. Keymapping and gamepad

Tasks:

- JSON profile format, one profile per Android package name. Bindings: key to tap at (x, y), WASD to virtual joystick, mouse movement to drag region (aim), button to swipe, macro sequences.
- A mapping engine that turns bindings into scrcpy touch events with correct multitouch pointer IDs.
- An overlay editor: semi-transparent layer over the window with drag-and-drop widgets, save and load.
- Gamepad support through `gilrs`, mapped onto the same actions.
- Profile import and export. Auto-switch the profile based on the foreground package (detected via `dumpsys`).

Acceptance: a shooter-style and a MOBA-style profile can be built in the editor and play correctly. Unit tests cover binding resolution.

### M5. Quality of life and a clean image

Tasks:

- APK install by drag and drop, including split APK and XAPK bundles.
- Shared folder, screenshots, screen recording (mux the scrcpy stream to mp4).
- Location, battery, network and sensor emulation through ADB or guest properties where supported.
- A reproducible, documented build of the base image with ad and analytics packages removed.
- Optional app sources: F-Droid, Aurora Store, microG. Never bundle Google Play without a proper license.

Acceptance: every feature has a settings toggle and a documentation entry.

### M6. GPU acceleration (research track)

Start with a time-boxed spike of two to three weeks that ends in a written report and a decision gate.

- Candidate paths: gfxstream + ANGLE (D3D11) from the AOSP emulator; virtio-gpu + rutabaga in QEMU on Windows; the crosvm Windows backend.
- Build a benchmark: open-source GL and Vulkan demos plus a few real games. Measure fps, input latency and CPU use on Intel, AMD and NVIDIA hardware.
- Output: an ADR with the recommended path, effort estimate and risks.
- If a path is viable, implement it behind a `gpu-accel` feature flag and keep software rendering as the fallback.

Acceptance: a measurable fps improvement over software rendering on at least two GPU vendors.

### M7. Packaging and the 0.1 release

Tasks:

- Installer (WiX MSI or MSIX), a portable zip and a winget manifest.
- Opt-in update check against GitHub Releases, with signed checksums.
- Documentation site, user guide and troubleshooting (WHPX, Hyper-V and VirtualBox conflicts), issue templates.
- Bundle QEMU with license texts and the source offer.

Acceptance: on a clean machine, install, create an instance and run an APK with no manual steps beyond enabling WHPX.

### Cross-cutting: CI and releases on GitHub

CI is set up in M0 and the release pipeline is added in M7. Everything runs in GitHub Actions.

**CI workflow** (`.github/workflows/ci.yml`)

- Triggers: `pull_request` and pushes to `main`. Never use `pull_request_target` to run code from a pull request.
- One job on `windows-latest`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`. Pin the toolchain in `rust-toolchain.toml` and cache with `Swatinem/rust-cache`.
- Supply chain: `cargo deny check` for licenses, advisories and banned crates, which enforces the policy in `THIRD_PARTY.md`.
- Dependabot for `cargo` and `github-actions`.
- Pin third-party actions to full commit SHAs. Default to `permissions: contents: read` and raise it per job only when needed.
- Branch protection on `main`: pull requests required, status checks required, no force-push.

**VM tests**

- Hosted Windows runners most likely lack nested virtualization, so tests that need WHPX stay `#[ignore]`.
- Optional: a self-hosted Windows runner labeled `whpx` runs a nightly smoke test (boot, ADB up, first video frame). Self-hosted runners execute workflow code, so they run only on `schedule` or `workflow_dispatch`, never for fork pull requests.

**Release workflow** (`.github/workflows/release.yml`)

- Trigger: a pushed tag matching `v*`, using SemVer (`v0.1.0-alpha.1`). Tags with a hyphen become pre-releases.
- Steps: build the release binary, then the MSI (WiX) and a portable zip, then `SHA256SUMS`, then optional Authenticode signing, then a build provenance attestation (`actions/attest-build-provenance`), then a GitHub Release whose notes come from `CHANGELOG.md`.
- Signing secrets live in a protected `release` environment with required reviewers. Unsigned builds trigger SmartScreen warnings, so evaluate SignPath Foundation or Azure's managed code-signing service and check current eligibility for open-source projects.
- winget: after a stable release, a job opens a pull request to `microsoft/winget-pkgs` (`wingetcreate` or a winget-releaser action).
- Updater: opt-in, reads the GitHub Releases API, and verifies the SHA-256 (and signature, when signed) before installing. No background pings.
- Large files: GitHub limits each release asset to 2 GiB. Base Android images stay out of app releases; publish them as assets of a separate `rsdroid-images` repository and pin their checksums in `tools/fetch-deps.ps1`.
- QEMU is GPLv2: each release that bundles it links the exact QEMU source archive and includes the license texts.

**Repository hygiene:** `CODEOWNERS`, issue and pull request templates, `SECURITY.md`, `.github/release.yml` for release-note categories, and an ADR choosing between `cargo-release` and release-please for versioning.

### Cross-cutting: performance budget and benchmarks

The final budget is fixed by [ADR 0003](docs/adr/0003-performance-budget.md) for a reference machine with 4 cores, 16 GB RAM and an integrated GPU. The 45 s boot proposal is a stretch goal, not the gate.

| Metric | Target (v0.1) | How measured |
| --- | --- | --- |
| Guest boot to `sys.boot_completed=1` | 60 s or less | Time until `sys.boot_completed` is 1 |
| Launch to ADB ready | 65 s or less | `adb wait-for-device` plus boot property, with `VIRT_WIFI=0` |
| Clone an instance | 5 s or less | `rsdroid bench clone` |
| Host overhead per running instance, excluding guest RAM | 200 MB or less | Working set of rsdroid processes minus guest RAM |
| Idle host CPU per running instance | 3% of one core or less | Performance counters over 60 s idle |
| Video, software decode | 30 fps at 720p | Frame counter in the render crate |
| First frame after starting the stream | 3 s or less | Time from probe start to decoded frame |
| Tap-to-pixel latency, software path | 100 ms or less (p90) | Timestamped test app or high-speed capture |
| Manager startup | 1 s or less | Cold start to first UI frame |

Harness and gates:

- `rsdroid bench` writes a JSON report. The baseline is committed as `bench/baseline.json`.
- Pure-logic benchmarks (decoder throughput, mapping engine) run with `criterion` on hosted runners and are reported, not gated, because hosted runners are noisy.
- VM benchmarks run nightly on the self-hosted `whpx` runner. A regression of more than 15% against the baseline opens an issue.
- Changing the budget requires an ADR. Never loosen a target just to pass.

### Cross-cutting: kickoff prompts

Paste the common line plus the milestone line into the agent at the start of each milestone. Common line: read `AGENTS.md` and `PLAN.md`, work only on the named milestone, split it into small tasks, verify every acceptance criterion, stop and ask on anything under Ask the human first, and finish with the report format from `AGENTS.md`.

- **M-1:** Build throwaway code on a `spike/` branch, prove the four go criteria, write ADR 0001. Do not create the workspace.
- **M0:** Create the workspace, CI, `tools/fetch-deps.ps1` and `rsdroid doctor`. Ask me for the name, license and base image before choosing.
- **M1:** Boot one headless VM from a typed config with QMP control and a Job Object. Check every QEMU flag against `-help`.
- **M2:** Get live video and input through scrcpy-server into a winit and wgpu window. Write the ADRs on ADB handling and the decoder first and ask me.
- **M3:** Add instances, qcow2 overlays, lifecycle and the egui manager. Prove four concurrent instances.
- **M4:** Build the mapping engine, the overlay editor and gamepad support, with unit tests for binding resolution.
- **M5:** Add APK install, shared folder, screenshots, recording, device emulation and the clean-image documentation.
- **M6:** Run the GPU spike only: benchmarks and an ADR with a recommendation. Do not implement until I decide.
- **M7:** Build the installer, release workflow and docs. Do not push tags or publish; prepare everything for me to do it.

### Cross-cutting: CLI and automation

Every capability of the GUI must be reachable without it. The GUI, the CLI and user scripts are all clients of one manager API, so each feature is built once and can be tested headless. This is also what lets the `whpx` runner drive real smoke tests in CI.

**Design**

- The manager core exposes a JSON-RPC 2.0 API in a new crate, `rsdroid-ipc`. Transport: a Windows named pipe restricted to the current user. Optional localhost TCP with a random token, off by default. Never listen on a non-loopback interface.
- A second new crate, `rsdroid-cli` (clap), builds the `rsdroid` command. It is a thin client: parse, call, print. No business logic lives in it.
- Two run modes, chosen by an ADR at M3: one-shot (the CLI starts the manager in-process) or a background daemon (`rsdroid daemon`) that the GUI and CLI attach to. Prefer the daemon, because instances should outlive a single command.
- The API is versioned with SemVer (`api.version`). The OpenRPC schema is generated from the Rust types (`schemars`) and committed as `docs/api/openrpc.json`. CI fails if it changes without a version bump.

**CLI conventions**

- Shape: `rsdroid <noun> <verb> [instance] [flags]`.
- Global flags: `--json` for machine-readable stdout, `--quiet`, `--timeout <seconds>` and `--data-dir`.
- Results go to stdout, logs and errors to stderr. Exit codes are stable: 0 success, 1 general failure, 2 usage error, 3 instance not found, 4 timeout, 5 environment problem that `doctor` explains.
- Commands are idempotent where it makes sense: `start` on a running instance succeeds.
- Shell completions for PowerShell, bash and zsh are generated by clap.

**Commands by milestone**

| Milestone | Commands added |
| --- | --- |
| M0 | `doctor`, and `doctor --bundle`, a zip of logs and config that the user attaches to a bug report by hand and that is never uploaded |
| M1 | `run <instance>`, `stop <instance>` |
| M2 | `screenshot <instance> <file>`, `input tap, swipe, key, text` |
| M3 | `instance create, clone, list, get, start, stop, pause, resume, snapshot, restore, delete`, `daemon`, `wait <instance> --boot`, `events` (JSON lines) |
| M4 | `keymap list, import, export, apply` and `macro run` |
| M5 | `install <instance> <apk>`, `shell <instance> -- <cmd>`, `adb <instance> -- <args>`, `record <instance> <file>`, `push`, `pull` |
| M7 | Help text, completions and docs; the CLI ships in the installer and is added to PATH only if the user opts in |

Events are JSON-RPC notifications from `events.subscribe`: `instance.state_changed`, `instance.boot_completed`, `window.closed` and `app.foreground_changed`.

**Automation layers**

1. Shell scripts and CI that call the CLI.
2. JSON action lists: macros from M4 are stored as JSON (`tap`, `swipe`, `key`, `wait`, `repeat`) and use the same action vocabulary as keymapping.
3. Embedded scripting only after v0.1, with a sandboxed pure-Rust language such as Rhai calling the same API. It needs an ADR first, and scripts get no filesystem or network access by default.
4. Official examples for PowerShell and Python (JSON-RPC over the named pipe) in `examples/`.

**Security**

- The pipe is accessible to the current user only. TCP, if enabled, requires the token.
- All API input is untrusted: validate instance names (no path separators), paths and sizes.
- Macros and scripts run only when the user invokes them. No auto-run and no remote-control endpoint.

**Testing**

- Contract tests: a golden JSON request and response for every API method, run against a fake backend.
- CLI integration tests with `assert_cmd` against the fake backend, on hosted CI.
- A real-VM smoke test on the `whpx` runner: create, start, wait for boot, install a test APK, take a screenshot, check it is not blank, stop.

M3 kickoff addition: write the ADR on daemon versus one-shot and the OpenRPC schema first, get my approval, then implement.

## 6. Risks and mitigations

- **No GPU acceleration means poor game performance.** Run the M6 spike early enough to inform expectations; ship software rendering with honest documentation.
- **Anti-cheat blocks emulators.** Out of scope; document it.
- **WHPX quirks and lower performance than KVM.** Test on several CPUs and keep per-VM tuning flags in the config.
- **scrcpy protocol drift.** Pin the version and add protocol tests with recorded streams.
- **Licensing traps** (ARM translation, Google apps, openh264). Write an ADR before bundling anything questionable.
- **Hyper-V and VirtualBox conflicts on the host.** The `doctor` command and docs must explain them.

## 7. Definition of done for v0.1

- One command or one installer gets a user to a running Android window.
- At least four concurrent instances with independent windows and storage.
- Keyboard, mouse and gamepad mapping with a visual editor.
- No ads, no telemetry, no outbound traffic except explicit user actions.
- Documented build, license compliance file and reproducible release pipeline.
