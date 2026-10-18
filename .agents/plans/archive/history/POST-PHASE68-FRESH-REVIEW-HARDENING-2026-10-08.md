# Post-Phase68 fresh review hardening — 2026-10-08

**Status:** complete; implementation committed and validated; Phase 68 remains the immutable numbered checkpoint
**Implementation checkpoint:** `5938d4e33fd837a6d3c741779eeb891488eae770` (`Harden post-Phase68 durability and release safety`)
**Source version:** 0.15.0
**SQLite schema:** v33 unchanged
**Numbered phase:** unchanged at `phase-68`
**Authority expansion:** none

## Purpose

Fresh review of the live Phase-68 tree for bugs, regressions, correctness, intuitive user behavior, scalability, composability and maintenance cost.

This is an unnumbered successor hardening pass. It does not create Phase 69 and does not revive intentionally skipped Phase 64/65 indexing/LSP work.

## Findings and fixes

### 1. Schedule claims could become permanently orphaned

Before this review, `pipeline_schedule_occurrences.state='claimed'` had no durable recovery path. A crash after claim persistence but before occurrence-to-run linkage could permanently block a `serial` schedule or cause a `skip` schedule to keep skipping behind the dead claim.

Hardening:

- persistent `run-due` uses the existing SessionStore durable process incarnation ID, never a raw OS PID;
- a newer owner process may recover an old claim only when the claimant is fenced/stopped/expired (or the owner lock proves an unknown pre-hardening claimant cannot still be authoritative);
- trusted-direct mode does not guess that an unknown claimant is dead;
- claimed provenance remains immutable: recovery does not rewrite `claimed_by`;
- paused schedules do not surface orphaned unadmitted claims; resume makes the same claim recoverable again.

### 2. Crash after pipeline admission but before occurrence linkage

A crash could occur after the deterministic pipeline run committed but before the occurrence row moved from `claimed` to `admitted`.

Hardening:

- reconciliation first searches the exact existing pipeline request ID;
- pipeline/revision/binding/input must exactly match the immutable schedule snapshot;
- the existing run is attached even if the binding was disabled after that run was already admitted;
- no second pipeline run/provider/tool dispatch is created.

### 3. Schedule request IDs were publicly preclaimable

Manual callers could use `sched:<schedule>:<occurrence>`, potentially colliding with future schedule occurrence identity.

Hardening:

- `sched:` is reserved by the store;
- public/manual `pipeline_admit_manual` rejects it;
- only crate-internal `pipeline_admit_scheduled` accepts the reserved namespace;
- occurrence admission verifies that the stored request ID equals the deterministic schedule/occurrence identity.

### 4. Pause/resume and process downtime could trigger surprise catch-up bursts

A paused schedule whose due time passed could run immediately on resume. An active one-minute schedule after long process downtime could repeatedly backfill old intervals and create an unexpected cost/work burst.

Hardening:

- resume preserves a still-future due time;
- if the due time passed while paused, cadence restarts from resume time;
- active downtime coalesces to at most one catch-up occurrence;
- after that occurrence/skip, next due is one interval after the current claim time rather than replaying every missed interval;
- occurrence numbering/evidence remains monotonic and explicit.

There is still no cron/missed-run-policy subsystem. The simple default is intentionally **no burst backfill**.

### 5. Whole-config stale snapshot writes could erase unrelated concurrent changes

Native Agent/Workspace config persistence and local web settings previously saved complete cached `FileConfig` snapshots. A skill added by another aw process could later disappear when a long-lived UI saved an unrelated theme/padding/profile change.

Hardening:

- one stable advisory metadata lock lives next to the config;
- one central `config::mutate` API locks, reloads the latest disk config, applies one typed mutation and atomically publishes it;
- `config::save_changes(before, after)` performs a three-way semantic merge;
- unrelated map/field changes merge;
- same-field stale edits fail with an explicit conflict instead of silently overwriting the newer value;
- native coalesced persistence retains its bounded/nonblocking latest-snapshot model but merges its local delta against current disk state;
- web UI/profile/default mutations use the same merge authority;
- ChatGPT host ID is a locked get-or-create mutation before OAuth begins;
- skill add/remove/install/update use the same config authority.

No config watcher, daemon or second persistence service is introduced.

### 6. Nested relative `--config` broke managed skill paths

Phase-66 bundle installation could persist a relative managed path that was later resolved against the config directory a second time.

Hardening:

- config mutation resolves a relative explicit config path once against the current directory;
- managed content is derived from that absolute config location;
- actual-binary Phase-66 smoke installs through `target/config.toml` and verifies exact activation from the resulting managed file.

### 7. `skill export` no-replace was TOCTOU-racy

`exists()` followed by an overwrite-capable atomic publication could race between aw processes.

Hardening:

- bundle output uses the same stable metadata-lock primitive;
- the no-replace existence decision and publication are serialized;
- concurrent no-replace export has exactly one winner.

### 8. Release/native probes inherited arbitrary caller credentials

The Phase-68 release auditor intentionally executes the candidate native binary for `--version`, but previously inherited the complete caller/CI environment.

Hardening:

- the native audit subprocess receives only the deliberately minimal probe environment;
- Phase-60/61/66/67/68 actual-binary validation helpers likewise do not inherit unrelated caller credentials.

### 9. Installers executed the downloaded candidate before installation

The installers used the candidate binary itself to discover/validate version before installation. This unnecessarily executed untrusted release bytes in the user's environment before the install decision.

Hardening:

- installers download release `VERSION` metadata instead;
- explicit requested versions must match that metadata;
- checksum sidecars must name the exact selected asset;
- Unix and PowerShell paths require one strict sidecar record;
- the candidate is not executed before installation;
- publisher-side Phase-68 artifact audit remains the place that intentionally executes the native artifact, under a scrubbed environment.

Checksums/VERSION from the same release origin remain integrity metadata, not publisher signatures.

## Architecture preserved

This pass intentionally does not add:

- schema v34;
- Phase 69;
- Phase 64/65 indexing/LSP work;
- another scheduler/executor loop;
- cron/webhook/shell/model-created schedule authority;
- config watchers or a config daemon;
- executable skill packages;
- new provider/tool/routing authority;
- release signing/notarization/Sigstore claims.

## Validation targets

1. Phase 68 remains current and schema stays v33.
2. Config merge regressions prove unrelated external mutations survive and same-field stale writes conflict.
3. Skill bundle no-replace serialization and relative nested config activation pass.
4. Phase-67 source checker includes process-incarnation recovery, paused orphan behavior, reserved namespace and no-burst cadence.
5. Actual-binary Phase-67 schedule smoke passes all strengthened cases.
6. Release audit fixture, release architecture and installer smoke remain green.
7. Browser model/workspace/rich-Markdown suites remain green.
8. Full native and minimal warning-denied suites remain green.
9. Whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc and static validation remain green.
10. Full source-bound reference profile passes after this checker is wired.

## Implementation evidence before checkpoint

- focused fresh-review checker: **92/92**;
- Phase-67 successor checker: **88/88**;
- Phase-68 successor checker: **79/79**;
- actual-binary Phase-67 schedule smoke: **10/10**;
- actual-binary Phase-66 skill bundle smoke: **4/4**;
- actual-binary Phase-61 skill registry smoke: **4/4**;
- actual-binary Phase-60 skill smoke: **3/3**;
- full native `aw`: **258/258**;
- minimal/no-default warning-denied native `aw`: **83/83** plus inherited library/provider tests;
- browser model/workspace/Agent/rich-Markdown suites: **107/107** and production Vite build green;
- release architecture: **76 checks**; synthetic artifact audit: **5/5**; Unix installer fixture smoke: green;
- whole-workspace/all-target compile: green;
- whole-workspace/all-target Clippy `-D warnings`: green;
- doctests/rustdoc warnings-as-errors: green;
- static validation: **1042 files / 1019 authored Rust test attributes / 735 links / 0 errors / 0 warnings**;
- complete source-bound reference profile: **78/78**; initial receipt `validation/host/20261008T061230Z-f232f660ca4e/report.json`.

## Remaining intentionally unclaimed / deferred

- actual Phase-68 optimized/current five-target release execution remains bounded by its documented environment limitations;
- Windows cross-target compilation of this review's metadata-lock helper is not claimed on this host because no Windows Rust target is installed; whole-workspace host compilation and source validation cover the implementation here;
- no automatic live config reloading is added. Concurrent unrelated writes merge; same-field races conflict and are surfaced at the existing persistence barrier;
- signing/notarization/publisher trust remain separate work;
- Phase 64/65 remain skipped.

## Next gate

User review / explicit next task.

Do not infer Phase 69 automatically.
