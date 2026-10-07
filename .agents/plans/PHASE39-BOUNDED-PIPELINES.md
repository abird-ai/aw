# Phase 39 — bounded pipeline runtime foundation

**Status:** implementation complete and locally validated on 2026-10-03; ready for the Phase-39 checkpoint commit/tag and user review.
**Base:** `7d1be61 / phase-38`.
**Schema target:** SQLite v22.
**Product boundary:** original v0.7 Phase 7A only — one bounded manual pipeline model. No classifier routing, sticky affinity, child agents, schedules, dashboards or advanced governance are part of this phase.

## Goal

Introduce one durable, typed pipeline primitive that later routing/autonomy can reuse without replacing the proven direct Agent path.

Bare `aw` remains the direct single-agent path. A pipeline is an explicit immutable revision selected by an authorized user. Manual pipeline runs are durable and idempotent, execute a small linear sequence of bounded steps, may pause for explicit approval, and may delegate a bounded `agent` step to the existing native Agent primitive. Every external step records intent before execution. A started external step without a settled receipt is never silently replayed.

## Phase 39 scope

1. Add transport-neutral `aw-pipeline`:
   - immutable typed `PipelineDefinition`;
   - bounded `PipelineLimits`;
   - linear `check | transform | approval | agent | emit` steps;
   - deterministic validation and SHA-256 definition/value digests;
   - no provider, store, tool, filesystem, network or auth authority.
2. Add optional `pipelines` Cargo feature. `full` includes it; explicit tiny builds do not.
3. Add SQLite schema v22:
   - immutable pipeline revisions;
   - explicit scope bindings with CAS revision;
   - durable pipeline runs;
   - durable step attempts/receipts;
   - durable approval requests/decisions;
   - exact manual request-ID deduplication.
4. Pipeline revision mutation is append-only. Editing creates a new revision; existing runs pin one exact revision.
5. Phase-39 binding scopes are deliberately small: `user` and `session`; triggers are `manual` only. Binding modes are `gate | transform | observer | task`, but only `task` is manually runnable in this phase.
6. Manual-run authority:
   - the pipeline owner may run an owned revision;
   - otherwise an active matching task/manual binding is required;
   - a Session binding additionally requires current Session read authority;
   - bindings never copy provider accounts, tool grants, credential custody or Session append.
7. Bounded linear execution:
   - `check`: deterministic host-independent assertion;
   - `transform`: deterministic text transformation with separate receipt;
   - `approval`: durable pause; no later step executes before explicit approval;
   - `agent`: invokes the existing native Agent using the caller's ordinary provider/account/model/tool authority;
   - `emit`: marks the current value as pipeline output.
8. No database transaction spans provider/model work or human approval.
9. Agent-step intent is durably marked `started` before provider execution. Process death before settlement yields `unknown_external_effect`; resume does not rerun it.
10. A failed/rejected pipeline never invents an assistant result or silently falls through.
11. Receipts record exact revision, input/output digests, step kind/state, execution Session/branch provenance and actual model invocation usage when available. Unknown usage remains unknown, never zero.
12. Manual runs use exact request IDs. Same request+input+revision resolves the existing run; changed input/revision conflicts.
13. Pipeline definitions cannot contain provider URLs, credentials, shell commands, grants or dynamic executable code.
14. Ordinary bare-agent execution performs no pipeline lookup/classifier work.

## Step contract

Phase 39 supports only:

- `check.non_empty`
- `check.max_bytes { max }`
- `check.contains { needle }`
- `transform.trim`
- `transform.prefix { text }`
- `transform.suffix { text }`
- `approval { message }`
- `agent { role }`, where `role` is bounded provenance only
- `emit`

Definitions are linear. DAG/parallel execution is deliberately deferred.

## Typed CLI

The full build exposes:

- `aw pipeline define FILE`
- `aw pipeline list`
- `aw pipeline show PIPELINE [--revision ...]`
- `aw pipeline bind ...`
- `aw pipeline bindings`
- `aw pipeline binding-set ...`
- `aw pipeline run ...`
- `aw pipeline resume RUN`
- `aw pipeline status RUN`
- `aw pipeline approve APPROVAL [--deny]`

The management commands manipulate durable definitions/bindings/receipts. `run`/`resume` are dispatched through the root runtime adapter so an agent step uses the same `Host::prompt_with` path as normal aw instead of creating a second agent loop.

## User-requested closeout hardening folded into Phase 39

The user reported one opaque SQLite failure after model setup and several fullscreen workspace interaction problems while Phase 39 was still uncommitted. These are correctness/UX fixes folded into this checkpoint, not a new numbered phase.

### Storage diagnostics and predecessor migration

- Healthy Phase-38 schema-v21 state is actual-binary tested through v22 migration and the first durable prompt.
- Clean first-run/deferred onboarding remains green.
- The exact user's local state DB is not visible through the current connector, so its specific record/constraint could not be inspected.
- Generic non-Busy/Locked SQLite failures now retain a bounded sanitized SQLite error class, extended code and message via `StorageDetail`, while preserving explicit no-automatic-replay wording.
- Busy/Locked remains a distinct bounded conflict.
- This diagnostic change does not expose prompt/provider payloads by design.

### Native fullscreen workspace interaction

- Clicking/opening a File preview activates the main preview tab **without stealing Explorer keyboard focus**.
- Files List mode is a directory browser: Enter descends, nested views begin with `../`, directories render with `/`, and Tree → List preserves the selected/current directory instead of resetting to root.
- Git List/Tree applies only to **Changes**. Worktrees/Branches render no meaningless display toggle; command attempts there fail explicitly.
- List/Tree uses compact Unicode icon/text-symbol controls rather than the words “List / Tree”; no Nerd Font is required.
- Every content tab has a clickable `×` close target in addition to keyboard close.
- Read-only File tabs use lazy Syntect highlighting over already-loaded bounded text, with sanitized plain-text fallback. Highlighting owns no filesystem/model/provider/store authority and is invalidated when the file preview refreshes.

## Non-goals

- model classifiers / Jev / Laya;
- Fixed / Route once / Every prompt / Sticky routing presets;
- dynamic worker/child-session fanout;
- schedules, cron, missed-run or overlap policy;
- automatic event/webhook triggers;
- dashboards/cost UI;
- tool/data/call_pipeline steps;
- generative plan instantiation;
- guardian recursion or advanced group policy;
- pipeline-defined credentials, URLs, shell commands or grants;
- changing provider/account/tool authority;
- weakening Phase-34/35 no-replay, writer-epoch or lock invariants;
- changing Phase-38 draft authority;
- native collaborative-draft participation.

## Focused executed evidence

Already green on the live Phase-39 tree:

- `aw-pipeline`: **6/6** bounded definition/digest/deterministic-step tests.
- Phase-39 store/runtime focused tests: **6/6**.
- schema-v21 → v22 migration regression preserving store identity/generation: green.
- Phase-39 source/authority checker: **144 checks**.
- workspace interaction/source checker: **38 checks**.
- focused native workspace UI suite: **22/22**.
- Syntect preview tests: **2/2**.
- SQLite actionable-diagnostic regression: **1/1**.
- actual compiled pipeline smoke with fake provider: **7/7**.
- actual compiled healthy-v21 → v22 → first durable prompt smoke: **1/1**.
- clean first-run/deferred onboarding actual-binary smoke: green.
- warning-denied `--no-default-features --features pipelines` compile/test path: green.
- warning-denied current `full` compile check: green.

Final closeout evidence on the exact current tree:

- source-bound reference profile: **43/43 steps green**;
- validation plan sizes: **43 reference / 119 Rust / 122 full**;
- warning-denied cuts green for `aw-pipeline`, explicit tiny `--no-default-features`, `pipelines`, `workspace`, and normal `full`;
- normal full product tests include **101 library/web + 95 CLI/workspace** tests plus provider fixtures;
- entire Cargo workspace/all-targets suite: green with `RUSTFLAGS=-D warnings`;
- entire workspace doctest suite: green with warnings denied;
- pinned immutable-store **Rust/Clippy 1.98.1**: full workspace/all-targets Clippy green with `-D warnings`;
- `RUSTDOCFLAGS=-D warnings cargo doc --locked --workspace --no-deps`: green;
- static source validation: **886 active files / 34 workspace manifests / 98 Python sources / 39 TOML / 104 JSON / 18 JavaScript / 760 authored Rust test attributes / 705 local links / 0 errors / 0 warnings**;
- `git diff --check`: green.

Host-specific evidence boundary: inherited Git/PTY fixtures use their documented `AW_GIT=/run/current-system/sw/bin/git` and `AW_BASH=/run/current-system/sw/bin/bash` overrides because this NixOS host lacks `/usr/bin/git` and `/bin/bash`. The connector's read-only `/nix/store` still prevents executing `nix build .#release-all`; no five-target release or GitHub Release execution is claimed. The user's exact local SQLite file is not available through this connector; both clean first-run and real non-empty Phase-38 v21 → current v22 prompt execution were proven, and future non-lock SQLite failures now expose bounded sanitized cause detail.

## Validation gates

Before the Phase-39 checkpoint/tag:

- source-bound reference profile, including pipeline and workspace-polish checkers;
- warning-denied tiny, pipelines, workspace and full cuts;
- whole-workspace/all-targets and doctests;
- pinned Clippy 1.98.1 and rustdoc warnings-as-errors;
- final static source/link validation and `git diff --check`;
- actual binary pipeline and v21-upgrade-first-prompt smokes;
- changed-file secret scan.

The connector's read-only `/nix/store` still prevents claiming `nix build .#release-all` here.

## Review boundary

Phase 39 ends at the bounded manual pipeline primitive plus the correctness/UX hardening above. Do not begin routing/classifier affinity, child workers, scheduling/autonomy, dashboard observability or advanced governance until the user explicitly authorizes the next numbered phase. Do not move historical tags. Do not push unless explicitly requested.
