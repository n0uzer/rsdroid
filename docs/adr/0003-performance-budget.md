# ADR 0003: Performance budget

Status: **Accepted** on 2026-11-22 by the maintainer. The boot and ADB-ready targets
below replace the proposal in `PLAN.md` and are the v0.1 gate.

## Context

`PLAN.md` defines proposed targets for a reference machine with 4 cores, 16 GB RAM and
an integrated GPU, and asks M0 to replace them with numbers based on the M-1
measurements. ADR 0001 measured Android-x86 9.0-r2 with QEMU 11.1.0/WHPX on a faster
machine: Ryzen 5 5600 (6 cores/12 threads), 16 GB RAM, GTX 1660 SUPER, 2 vCPU and
3 GB guest RAM.

Measured results (ADR 0001):

| Metric | Measured |
| --- | --- |
| Guest boot to `sys.boot_completed=1` | 48.6-54.7 s |
| Launch to ADB ready | 59.0-65.1 s, including a 10.2 s manual network step |
| Overlay creation | 0.09 s |
| First frame after probe start | 0.83-2.09 s |
| Tap-to-pixel latency, host side | median 54.5-63.0 ms, p90 63-78 ms |
| Idle CPU per instance | 1.6-2.1% of one core |
| QEMU working set with `-m 3072` | ~3140 MB (almost all guest RAM) |

The 45 s boot proposal is not met on hardware faster than the reference machine.
ADR 0001 also shows that the 10.2 s network step can be removed with `VIRT_WIFI=0`,
which brings ADB-ready close to guest boot. Host overhead excluding guest RAM, video
fps and manager startup were not measured in M-1 because the manager did not exist.

## Decision

The v0.1 budget below applies to the reference machine: 4 cores, 16 GB RAM, integrated
GPU, Windows 10 22H2 or Windows 11, WHPX enabled. Measurements on other machines are
recorded with their hardware and are not compared directly.

| Metric | Budget | How measured | Evidence |
| --- | --- | --- | --- |
| Guest boot to `sys.boot_completed=1` | **60 s or less** | timestamp from launch to the property becoming 1 | 48.6-54.7 s on faster hardware |
| Launch to ADB ready | **65 s or less** | `adb wait-for-device` plus boot property, with `VIRT_WIFI=0` | 59.0-65.1 s with a 10.2 s manual step that the image fix removes |
| Clone an instance | **5 s or less** | `rsdroid bench clone` | overlay creation 0.09 s |
| Host overhead per instance, excluding guest RAM | **200 MB or less** | working set of rsdroid processes, guest RAM subtracted | not yet measurable; first data in M2/M3 |
| Idle host CPU per instance | **3% of one core or less** | performance counters over 60 s idle | 1.6-2.1% of one core |
| Video, software decode | **30 fps at 720p** | frame counter in `rsdroid-render` | not measured in M-1 |
| First frame after starting the stream | **3 s or less** | timestamp from probe start to decoded frame | 0.83-2.09 s |
| Tap-to-pixel latency, software path | **100 ms or less** (p90) | timestamped test app or high-speed capture | median 54.5-63.0 ms, p90 63-78 ms |
| Manager startup | **1 s or less** | cold start to first UI frame | not measured in M-1 |

The 45 s boot proposal remains a **stretch goal**, not the gate. It is realistic only
after the image fix and boot-time tuning; the budget gate is 60 s until an ADR changes
it.

## Why this is not "loosening a target to pass"

No benchmark gate or baseline exists yet, so no check is being made green. `PLAN.md`
explicitly says the table is a proposal to be replaced using M-1 measurements. The
change is isolated to boot/ADB-ready, is based on measurements on faster hardware, and
was submitted as a Proposed ADR and approved by the maintainer rather than applied
silently. All other targets are kept, and the unmeasured ones remain requirements to
measure in M2/M3, not assumptions.

## Measurement rules

- `rsdroid bench` (M2) writes a JSON report; the first accepted baseline is committed
  as `bench/baseline.json`.
- Pure-logic benchmarks run with `criterion` on hosted runners and are reported, not
  gated, because hosted runners are noisy.
- VM benchmarks run nightly on the self-hosted `whpx` runner. A regression of more than
  15% against the baseline opens an issue.
- Host overhead is computed as the rsdroid and QEMU working set minus `-m <guest RAM>`
  so guest RAM is not confused with host overhead (finding 5 in ADR 0001).
- Tap latency must come from a measurement that requires the screen to change near the
  tap; the frame-change check from the spike is not valid (finding 3 in ADR 0001).
- Changing a budget entry, or the baseline, requires an ADR and maintainer approval.
  Never loosen a target to make CI pass.

## Consequences

- M1 must bake `VIRT_WIFI=0` into the base image; otherwise the ADB-ready budget of
  65 s cannot be met without a manual step.
- M2 must add the frame counter and the latency measurement harness before the video
  and latency targets can be verified.
- M3 must measure host overhead and manager startup; the 200 MB and 1 s values are
  accepted as requirements, not as measured facts.
- The reference machine is deliberately weaker than the M-1 machine. Meeting the same
  boot budget there is a real constraint and may require boot tuning.
