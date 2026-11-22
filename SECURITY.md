# Security policy

## Supported versions

rsdroid has no releases yet. Security fixes go to `main` and will be backported to the
latest release once v0.1 ships. Nothing older is supported.

## Reporting a vulnerability

Do **not** open a public issue for a vulnerability. Use GitHub's private vulnerability
reporting:

<https://github.com/n0uzer/rsdroid/security/advisories/new>

Include the version or commit, Windows build, reproduction steps, impact and any
suggested fix. You will get an acknowledgement as soon as possible. Please give the
maintainer time to fix the problem before publishing details.

## Scope

rsdroid runs a local Android VM and downloads pinned tools. Reports are especially
valuable for:

- QEMU launch arguments and Job Object handling that could escape the guest or leave
  orphan processes;
- path handling that writes outside the data directory or follows untrusted links;
- the named-pipe API and any listener reachable from another machine (there must be
  none);
- checksum verification in `tools/fetch-deps.ps1` or a pinned URL that can be
  substituted;
- protocol parsers for scrcpy video and control messages, and the doctor bundle
  archive;
- any telemetry, unexpected network request or credential leak.

Out of scope: vulnerabilities that require an already-compromised host, Android
platform bugs unrelated to rsdroid, and anti-cheat bypasses (explicitly a non-goal).

## Doctor bundles

`rsdroid doctor --bundle` creates a ZIP **locally** and never uploads it. It can
contain paths, usernames and log contents. Review it before attaching it to an issue
and remove anything sensitive.
