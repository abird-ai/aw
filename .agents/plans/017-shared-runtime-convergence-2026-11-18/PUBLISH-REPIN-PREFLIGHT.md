# AW -> dotlink 0.17 publish/repin preflight

Status: **prepared only; authority-gated**

This is the mechanical cutover checklist for publishing the accumulated AW
P1-P8 shared-runtime implementation and then moving dotlink D1-D7 onto it.

Do not stage, commit, push, publish, repin, tag, release or deploy from this
document unless the user explicitly authorizes that action.

## 1. Current source boundary

AW:

    HEAD        021b8cec7b9122a6f6acaf5ef9137fb31f0a6659
    origin/main 792f0b38d104659264b70c31736655df1a46b60f

Existing local commits above origin/main:

    796225e9b50e824606586859d7bcc88722b4919e  Separate process signal identity from lifecycle
    63f4056027135227f49170ce6602c8f43a9f5ad0  Plan shared-first 0.17 runtime convergence
    021b8cec7b9122a6f6acaf5ef9137fb31f0a6659  Add shared process stop evidence types

All three contain SSH commit signatures. AW is configured with
commit.gpgsign=true, gpg.format=ssh and the owner's Ed25519 signing key.

The remaining P2-P8 source is intentionally unstaged in the AW working tree.
It includes the shared platform Guard/evidence adapter, ProcessTerminal/Task
propagation, raw-vs-serialized budgets, flat JobRef, bounded diagnostics, typed
receipts, aw-mcp receipt/result fitting, and heartbeat/progress.

The untracked source files that must be part of the implementation checkpoint:

    crates/aw-tools/src/budget.rs
    crates/aw-tools/src/diagnostic.rs

Frozen sandbox/config-v13 paths remain out of scope and must have no working
diff.

## 2. Commit partition when explicitly authorized

Prefer two new AW commits after the three existing signed commits.

### A. Source implementation checkpoint

Proposed subject:

    Complete shared 0.17 runtime convergence

Stage only the P2-P8 source/test implementation in:

    crates/aw-tools/**
    crates/aw-tools-adapter/**
    crates/aw-mcp/**
    crates/aw-platform-unix/**
    crates/aw-platform-win/**
    crates/aw-terminal/**

Include budget.rs and diagnostic.rs.

Do not stage .agents/plans/ux/** or any frozen sandbox/config-v13 path.

This commit is the dependency pin SHA for dotlink.

Keeping P2-P8 source in one final implementation checkpoint avoids publishing
intermediate commits whose mutually dependent core/platform/adapter/MCP changes
have not been qualified as independent repository states.

### B. Documentation/handoff checkpoint

Proposed subject:

    Record shared 0.17 convergence handoff

Stage the owned convergence docs/handoffs and historical plan-status
synchronization only after the source checkpoint is fixed.

This docs commit may be pushed after the implementation checkpoint, but dotlink
must pin the implementation checkpoint SHA, not this later docs-only SHA.

## 3. AW pre-commit gates

Immediately before the implementation commit, revalidate:

- aw-tools all-features lib;
- aw-tools-adapter all-features lib;
- aw-mcp all-features lib;
- aw-platform-unix unit + PTY integration;
- aw-terminal lib;
- host-side aw-platform-win check;
- strict Clippy for every touched crate;
- aw-tools no-default/minimal/stateless configurations;
- downstream aw-host / aw-cli checks;
- workspace all-feature check;
- rustfmt;
- git diff --check.

Native Windows Job Object execution remains an external gate on L5 because the
Windows Rust std target is not installed; host-side Windows compilation must
still be green.

### Pre-publish D1 API gap — resolved

The shared platform already owned pre-spawn containment setup,
`ManagedProcessGuard`, three-way stop evidence and signal-name presentation.
Dotlink source mapping found one remaining factual gap: Unix extraction of the
signal that physically ended a waited `ExitStatus`.

That gap is now resolved by:

    aw_platform::exit_signal(&std::process::ExitStatus) -> Option<i32>

Unix uses `ExitStatusExt::signal()`; Windows returns `None` and has a native
assertion authored in its existing managed-process test. The AW process adapter
now consumes this same helper, so dotlink need not copy cfg-sensitive signal
logic. `aw-platform` remains independent of `aw-tools::ProcessExit`; the host or
adapter combines `status.code()` / `exit_signal()` into the shared exit enum.

Fresh qualification: Unix platform **13/13 + 4/4 PTY** (cross-UID fixture still
intentionally ignored), adapter **256/256**, terminal **20/20**, strict affected
Clippy, minimal configs, downstream host/CLI and full workspace all-feature
check green. Windows-native execution remains external because L5 lacks the
Windows Rust std target.

The exact dotlink D1-D3 source inventory is recorded in its companion
`D1-D3-SOURCE-CUTOVER-MAP.md`.

Before staging, verify status/staging while excluding the reserved UX subtree,
and separately verify zero working diff in the frozen sandbox plan/source set.

## 4. Publish gate

Only after explicit publish/push authorization:

1. create the signed implementation commit;
2. record its exact full SHA as AW_IMPL_SHA;
3. create the signed docs/handoff commit;
4. push AW main;
5. fetch/verify origin/main;
6. verify AW_IMPL_SHA is reachable from origin/main;
7. verify its SSH signature is present;
8. do not tag or release unless separately authorized.

A successful push is not sufficient evidence by itself; exact remote refs and
commit reachability must be checked.

## 5. Dotlink pre-repin baseline

Current dotlink:

    HEAD        6598de605a006a75473a2043de3f60db384b0d02
    origin/main 5381729cd469eda54691e540deb3fee13e9f3e3e

The dirty dotlink tree already contains product-owned/pre-repin D2/D4-D8/D9
hardening and compatibility regressions. Preserve it exactly.

If commit authorization includes dotlink, prefer logical signed baseline commits
before the semantic cutover:

1. Nix environment ownership hardening;
2. pre-repin runtime/MCP compatibility prerequisites;
3. equivalence/runtime-health/consumer-plan documentation.

These baseline commits are rollback/readability aids; they are not permission to
publish or repin.

## 6. Atomic dependency set

The three existing direct AW Git dependencies currently all pin
792f0b38d104659264b70c31736655df1a46b60f:

    aw-tools
    aw-tools-fs-unix
    aw-tools-fs-win

D1 and D6/D7 introduce two additional direct shared surfaces:

    aw-platform
    aw-mcp

The repin must therefore converge the complete direct AW set on one exact
published AW_IMPL_SHA:

    aw-tools          -> AW_IMPL_SHA
    aw-tools-fs-unix  -> AW_IMPL_SHA
    aw-tools-fs-win   -> AW_IMPL_SHA
    aw-platform       -> AW_IMPL_SHA
    aw-mcp            -> AW_IMPL_SHA

Cargo.lock must resolve every Git package sourced from
https://github.com/abird-ai/aw to that same commit. Do not leave a mixed AW
revision graph.

Why the two new direct dependencies are required:

- D1 replaces dotlink's private process-tree Guard with the target-selected
  aw-platform::ManagedProcessGuard and platform facts.
- D6/D7 replace dotlink-local receipt/fitter/heartbeat helpers with aw-mcp.

Do not add aw-tools-adapter merely to reach these generic surfaces; dotlink
retains its own product shell/job adapter and should consume the smallest shared
owners directly.

No sibling/path dependency is allowed.

## 7. Repin procedure

After AW_IMPL_SHA is published and explicit repin authorization exists:

1. edit all five direct AW Git dependency entries to the exact SHA;
2. update Cargo.lock from the Git revision;
3. inspect every git+https://github.com/abird-ai/aw lock entry;
4. fail the repin if any AW package resolves to another SHA;
5. run locked all-target/all-feature check before semantic source edits.

Do not fake an unpublished rev and do not use a local path override for
qualification.

## 8. D1-D7 cutover order

### D1-D3: process provenance / lifecycle / terminal projection

- consume aw-platform::ManagedProcessGuard;
- consume aw_tools::execution_stop::{StopRecord, StopEvidence, StopReason};
- preserve ProcessExit::Signal(n) as observed physical exit;
- connect ExecutionLifecycleEvent::ProcessStop to the product observer;
- render exact product stopped_by and operator log only from canonical facts;
- project shared TaskTerminalDetail { exit, stop };
- remove private stop_proven/synthetic process-verdict reconstruction only after
  parity tests are green.

### D4-D5: finite refs / receipts / budgets

- replace local finite ref parser with aw_tools::execution_ref::JobRef;
- consume shared receipt renderers;
- use bounded_diagnostic;
- use RawPageBytes for source-page requests;
- use SerializedResultBytes independently for the response envelope;
- retain the already-green one-byte-page, absolute-offset, ACK and hostile-ref
  regressions.

### D6-D7: MCP result decoration / progress

- add/consume aw-mcp;
- replace local stored/visible receipt timestamp helpers;
- replace local text-only complete-result fitter;
- preserve the already-green immutable metadata/media behavior;
- replace local heartbeat scheduler with shared ProgressHeartbeat::from_token
  and await_with_progress_heartbeat;
- implement product logging as ProgressHeartbeatObserver;
- retain 10 s / 100 ms defaults and the no-backpressure-latency regression.

Delete local generic equivalents only after the shared implementation passes the
same product behavior tests.

## 9. Dotlink cutover qualification

After D1-D7:

- locked all-target/all-feature cargo check;
- full dotlink binary unit suite;
- all integration tests;
- strict all-target/all-feature Clippy;
- rustfmt;
- git diff --check;
- Nix module/check derivations;
- current v0.17.4 equivalence matrix;
- real connector stop-line tests;
- external-kill/no-stopped_by tests;
- typed media/structured result tests;
- one-byte jobs_read + ACK tests;
- progress backpressure/drop tests.

Then rerun D9 final operational/equivalence qualification. The pre-repin D9
checkpoint is not the final post-cutover gate.

## 10. Stop/rollback conditions

Stop before further mutation if any of these occurs:

- AW implementation commit is not published/reachable;
- signature is absent or unexpected;
- any dotlink AW dependency/lock entry resolves to a different SHA;
- frozen sandbox/config-v13 paths acquire a diff;
- reserved UX staging would need to be inspected or changed;
- a shared API needed by D1-D7 is missing from AW_IMPL_SHA; the previously
  identified physical ExitStatus signal-fact gap is resolved and must not
  regress before the implementation SHA is frozen;
- parity requires recreating a dotlink-local copy of shared semantics;
- a cutover test loses physical exit/stop evidence, Job identity, receipt
  metadata, ACK proof, typed result identity, or progress cancellation behavior.

Rollback is Git-level: keep the pre-repin dotlink baseline intact and restore
all five AW dependency entries/lock records together. Never roll back only one
AW crate pin.

## 11. Authority boundary

A generic "continue" / "let's go next" is not authorization to commit, publish,
push or repin.

The next executable authority-gated action is:

    signed AW implementation commit -> signed docs commit -> push AW ->
    verify AW_IMPL_SHA -> atomically repin dotlink -> D1-D7 cutover
