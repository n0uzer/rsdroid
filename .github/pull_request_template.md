## Summary

<!-- What changed and why. Link the issue or milestone. -->

## Milestone / issue

<!-- For example: M0 foundations, closes #12. Work outside the current milestone needs a reason. -->

## Verification

<!-- Paste the exact commands and their results, not just "tests pass". -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo build --workspace --locked`
- [ ] `pwsh tools/fetch-deps.ps1 -VerifyManifest`
- [ ] `cargo deny check` (if dependencies changed)

## Checklist

- [ ] The change follows the milestone and its acceptance criteria in `PLAN.md`.
- [ ] New logic has tests; no test was skipped or weakened.
- [ ] Behavior and setup changes are documented; `CHANGELOG.md` is updated.
- [ ] A significant decision has an ADR in `docs/adr/`.
- [ ] `THIRD_PARTY.md` is updated for new or changed dependencies.
- [ ] No binaries, images, credentials or generated artifacts are committed.
- [ ] No telemetry, analytics or unrequested network calls were added.

## Not done / risks

<!-- Anything partial, untested or deliberately deferred. Do not hide failures. -->

## Decisions needed

<!-- Questions for the maintainer, each with a recommendation. -->
