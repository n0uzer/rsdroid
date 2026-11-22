# Third-party components

rsdroid itself is **GPL-3.0-or-later**. This file tracks the direct dependencies and
the external tools the project downloads or bundles. Transitive Rust dependencies are
covered by the allowlist in [`deny.toml`](deny.toml) and are checked by
`cargo deny check` in CI.

Nothing here is legal advice. When in doubt, read the upstream license text.

## Rust dependencies

| Crate | Version | License | Used by | Purpose |
| --- | --- | --- | --- | --- |
| `clap` | 4.6.9 | MIT OR Apache-2.0 | `rsdroid-cli` | command-line parsing |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | `rsdroid-core` | report serialization |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | `rsdroid-core`, `rsdroid-cli` | JSON report output |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 | `rsdroid-core`, `rsdroid-cli` | typed errors |
| `tracing` | 0.1.44 | MIT | `rsdroid-cli` | diagnostics |
| `tracing-subscriber` | 0.3.23 | MIT | `rsdroid-cli` | log filtering and output |
| `time` | 0.3.55 | MIT OR Apache-2.0 | `rsdroid-core` | report timestamps |
| `zip` | 9.8.1 | MIT | `rsdroid-core` | doctor bundle archive |
| `winreg` | 0.56.0 | MIT | `rsdroid-core` (Windows) | read-only registry probes |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | `rsdroid-core` (Windows) | Win32 process and memory probes |
| `libloading` | 0.9.1 | ISC | `rsdroid-core` (Windows) | load `WinHvPlatform.dll` |
| `wait-timeout` | 0.2.1 | MIT OR Apache-2.0 | `rsdroid-core` (Windows) | bounded version probes |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | `rsdroid-core`, tests | temporary files |

Build and test only (not shipped):

| Crate | Version | License | Purpose |
| --- | --- | --- | --- |
| `assert_cmd` | 2.2.2 | MIT OR Apache-2.0 | CLI integration tests |
| `predicates` | 3.1.4 | MIT OR Apache-2.0 | CLI assertions |

`cargo-deny` (MIT OR Apache-2.0) runs in CI and is not part of the product.

## External tools and images

These are downloaded by [`tools/fetch-deps.ps1`](tools/fetch-deps.ps1), never committed
to git. QEMU is verified but not installed or executed by the script.

| Component | Version | License | Distribution notes |
| --- | --- | --- | --- |
| QEMU for Windows | 11.1.0 (build 20260811) | GPL-2.0-only (with BSD/LGPL components) | Separate process. Any release that bundles QEMU must include its license texts and a written offer for the corresponding source. Bundling is an M1 decision. |
| scrcpy-server | 5.0.1 | Apache-2.0 | Pushed to the guest at runtime. Keep the upstream license notice in redistributions. |
| Android-x86 | 9.0-r2 | GPL-2.0 and Apache-2.0 components | Base image is downloaded by the user, not redistributed. The project may publish its own reproducible image later (M5). |

The scrcpy-server checksum is pinned because the client and server protocol are not
stable across releases. Base images and installers are pinned by SHA-256/SHA-512 in
[`tools/deps.json`](tools/deps.json).

## GitHub Actions

CI uses `actions/checkout`, `Swatinem/rust-cache` and
`EmbarkStudios/cargo-deny-action`, pinned to full commit SHAs in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml). They run on GitHub-hosted
runners only and are not distributed with rsdroid.
