# AW -> dotlink 0.17 publish/repin preflight

Status: **prepared only; authority-gated**

This is the mechanical cutover checklist for publishing the accumulated AW
P1-P8 shared-runtime implementation and then moving dotlink D1-D7 onto it.

Do not stage, commit, push, publish, repin, tag, release or deploy from this
document unless the user explicitly authorizes that action.

## 1. Current source boundary

AW implementation pin selected for this publish:

    AW_IMPL_SHA 1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
    origin/main 792f0b38d104659264b70c31736655df1a46b60f  # before current push

Signed local commits above origin/main include:

    796225e9b50e824606586859d7bcc88722b4919e  Separate process signal identity from lifecycle
    63f4056027135227f49170ce6602c8f43a9f5ad0  Plan shared-first 0.17 runtime convergence
    021b8cec7b9122a6f6acaf5ef9137fb31f0a6659  Add shared process stop evidence types
    eb325b31d393e4d30c8aa2ff3d9e00bf5c506d2f  Complete shared 0.17 runtime convergence
    61ed065dbaf88ef6365d2ecb890e0df940e250f4  Record shared 0.17 convergence handoff
    7c7de6c9aeedf714adbe67b17ac863f27697e90e  Fix shared 0.17 projection boundaries
    1a5d0f65208e4a05b737c804a55a3aaad3e2fa59  Relax shared MCP protocol dependency ranges

`1a5d0f65...` is the final `AW_IMPL_SHA`: it keeps the fresh-review semantics
from `7c7de6c9...` while allowing compatible downstream Tokio/serde_json
versions. It supersedes `eb325b31` and intermediate `7c7de6c9` for dependency use.

The new lightweight shared surface is `crates/aw-mcp-protocol/**`; `aw-mcp`
depends on and re-exports it for AW client users.

Frozen sandbox/config-v13 paths remain out of scope and must have no working
diff.

## 2. Commit partition when explicitly authorized

The initial source/docs pair exists at `eb325b31` / `61ed065d` and the
fresh-review source checkpoint is signed at `7c7de6c9...`; the downstream-range
follow-up is signed at `1a5d0f65...` and is the final pin. Only docs sync and
remote publication of that follow-up remain before the atomic dotlink repin.

### A. Fresh-review source checkpoint

Completed signed source checkpoints:

    7c7de6c9aeedf714adbe67b17ac863f27697e90e  Fix shared 0.17 projection boundaries
    1a5d0f65208e4a05b737c804a55a3aaad3e2fa59  Relax shared MCP protocol dependency ranges

The committed source/test set is:

    Cargo.toml
    Cargo.lock
    crates/aw-mcp/Cargo.toml
    crates/aw-mcp/src/lib.rs
    crates/aw-mcp-protocol/Cargo.toml
    crates/aw-mcp-protocol/src/lib.rs
    crates/aw-tools-adapter/src/process.rs
    crates/aw-tools/src/execution_stop.rs
    crates/aw-tools/src/filesystem/runner.rs
    crates/aw-tools/src/lib.rs
    crates/aw-tools/src/receipt.rs
    crates/aw-tools/src/receipt_policy.rs
    crates/aw-tools/src/toolset.rs

The final signed `1a5d0f65...` commit, not `eb325b31` or intermediate
`7c7de6c9`, is the dependency pin SHA.

Do not stage .agents/plans/ux/** or any frozen sandbox/config-v13 path.

This commit is the dependency pin SHA for dotlink.

Keeping P2-P8 source in one final implementation checkpoint avoids publishing
intermediate commits whose mutually dependent core/platform/adapter/MCP changes
have not been qualified as independent repository states.

### B. Documentation/handoff checkpoint

Proposed subject:

    Record shared 0.17 convergence handoff

Stage only the owned convergence docs/handoffs after the source checkpoint is
fixed:

    .agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md
    .agents/docs/AW-DOTLINK-017-NEXT-AGENT-2026-11-18.md
    .agents/docs/CURRENT_HANDOFF.md
    .agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
    .agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
    .agents/plans/017-shared-runtime-convergence-2026-11-18/PUBLISH-REPIN-PREFLIGHT.md
    .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
    .agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md

This docs commit may be pushed after the implementation checkpoint, but dotlink
must pin the implementation checkpoint SHA, not this later docs-only SHA.

## 3. AW pre-commit gates

Immediately before the implementation commit, revalidate:

- aw-tools all-features lib;
- aw-tools-adapter all-features lib;
- aw-mcp-protocol unit + strict Clippy + dependency-tree lightweight-boundary check;
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
intentionally ignored), adapter **255/255**, terminal **20/20**, strict affected
Clippy, minimal configs, downstream host/CLI and full workspace all-feature
check green. Windows-native execution remains external because L5 lacks the
Windows Rust std target.

The exact dotlink consumer inventories are recorded in the companion
`D1-D3-SOURCE-CUTOVER-MAP.md` and `D4-D7-SOURCE-CUTOVER-MAP.md` files.

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

Current committed dotlink baseline:

    HEAD        8e14ee2b8575c35bc3d5a7204ac15c121410c751
    origin/main 5381729cd469eda54691e540deb3fee13e9f3e3e

Signed pre-repin Nix/logging/runtime/docs checkpoints already exist. Fresh review
adds one small product-source parity set plus its updated consumer documentation.

If explicit commit authorization includes dotlink, use two logical signed
checkpoints before the semantic cutover.

### Dotlink A. Fresh-review product parity

Suggested subject:

    Harden pre-repin MCP and shell boundaries

Stage exactly:

    src/mcp.rs
    src/mcp/tools/jobs.rs
    src/mcp/tools/shell.rs
    tests/stdio_tools.rs

This contains only independently valid behavior on the old AW pin: zero-byte
raw-page rejection, receipt-object/tight-timestamp fixes, mandatory truncation
markers, and the v0.17.5 hard-deadline wording.

### Dotlink B. Fresh-review consumer docs

Suggested subject:

    Record fresh 0.17.x consumer cutover

Stage exactly:

    AGENTS.md
    .agents/docs/CURRENT_HANDOFF.md
    .agents/docs/runtime-health-check.md
    .agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
    .agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
    .agents/plans/017-main-shared-consumer-backport-2026-11-18/V0174-EQUIVALENCE.md
    .agents/plans/017-main-shared-consumer-backport-2026-11-18/D1-D3-SOURCE-CUTOVER-MAP.md
    .agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md

These commits are rollback/readability aids; they are not permission to push,
publish or repin.

## 6. Atomic dependency set

The three existing direct AW Git dependencies currently all pin
792f0b38d104659264b70c31736655df1a46b60f:

    aw-tools
    aw-tools-fs-unix
    aw-tools-fs-win

D1 and D6/D7 introduce two additional direct shared surfaces:

    aw-platform
    aw-mcp-protocol

The repin must therefore converge the complete direct AW set on one exact
published AW_IMPL_SHA:

    aw-tools          -> AW_IMPL_SHA
    aw-tools-fs-unix  -> AW_IMPL_SHA
    aw-tools-fs-win   -> AW_IMPL_SHA
    aw-platform       -> AW_IMPL_SHA
    aw-mcp-protocol   -> AW_IMPL_SHA

Cargo.lock must resolve every Git package sourced from
https://github.com/abird-ai/aw to that same commit. Do not leave a mixed AW
revision graph.

Why the two new direct dependencies are required:

- D1 replaces dotlink's private process-tree Guard with the target-selected
  aw-platform::ManagedProcessGuard and platform facts.
- D6/D7 replace dotlink-local receipt/fitter/heartbeat helpers with the
  lightweight `aw-mcp-protocol`; dotlink must not depend on the full `aw-mcp`
  client adapter. It enables no aw-tools feature, so it cannot widen dotlink's
  existing `aw-tools/conformance,durable` feature set through Cargo unification.

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
- consume shared `StopTracker` for Unknown->retry reason preservation;
- treat `ExecutionLifecycleEvent::ProcessStop` as attempt-level diagnostics;
- render exact product stopped_by and exactly one operator stop line from
  `ProcessReceipt::stop_record()`, never once per attempt event;
- project shared TaskTerminalDetail { exit, stop };
- remove private stop_proven/synthetic process-verdict reconstruction only after
  parity tests are green.

### D4-D5: finite refs / receipts / budgets

- replace local finite ref parser with aw_tools::execution_ref::JobRef;
- consume shared receipt renderers;
- use bounded_diagnostic;
- use RawPageBytes for source-page requests;
- use SerializedResultBytes independently for the response envelope;
- retain the already-green zero-byte rejection, one-byte page, absolute-offset,
  ACK and hostile-ref regressions.

### D6-D7: MCP result decoration / progress

- add/consume `aw-mcp-protocol` only;
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
- current v0.17.x equivalence matrix through the read-only local 0.17.5 release head;
- real connector stop-line tests;
- v0.17.5 outside-signal line / stop-vs-exit mutual-exclusion tests at every verbosity;
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
