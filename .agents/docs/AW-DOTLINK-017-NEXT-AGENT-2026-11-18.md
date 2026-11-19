# AW + dotlink 0.17 shared-runtime next-agent entrypoint

Use dotlink---l5 only.

Work in:

    AW:      /home/pvl/spaces/abird/src/aw
    dotlink: /home/pvl/spaces/abird/src/dotlink
    release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1/X2 or create another worktree. Preserve unrelated work.
AW .agents/plans/ux/** is reserved by another owner: do not read, list, stage,
reset, stash or inspect its staging. The 017 sandbox/config-v13 plan is frozen;
do not implement it unless the user explicitly promotes it.

Keep implementation turns near 30 minutes, checkpoint at the next coherent
point, update durable docs, then pause.

## Read first

1. AW AGENTS.md
2. AW .agents/docs/CURRENT_HANDOFF.md
3. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
4. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
5. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
6. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
7. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/PUBLISH-REPIN-PREFLIGHT.md
8. dotlink AGENTS.md
9. dotlink .agents/docs/CURRENT_HANDOFF.md
10. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
11. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
12. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/D1-D3-SOURCE-CUTOVER-MAP.md
13. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md
14. live source/tests for every owner touched.

## Current refs

Authoritative fresh-review implementation pin:

    AW_IMPL_SHA 7c7de6c9aeedf714adbe67b17ac863f27697e90e

It is signed and supersedes historical source checkpoint `eb325b31...`.
At this docs-edit instant `origin/main` is still `792f0b38...`; the current user
turn explicitly authorizes publishing AW and atomically repinning dotlink.

Dotlink committed baseline:

    HEAD        8e14ee2b8575c35bc3d5a7204ac15c121410c751
    origin/main 5381729cd469eda54691e540deb3fee13e9f3e3e

Dotlink also has small uncommitted pre-repin parity fixes.

Current read-only release reference:

    local release/0.17  3995448d7bf33794ef84c7bb51150d77cc260ada
    origin/release/0.17 3995448d7bf33794ef84c7bb51150d77cc260ada
    v0.17.5              3995448d7bf33794ef84c7bb51150d77cc260ada
    v0.17.4              fd1cdb6429a341a4e3d27805cae0bec288fc26df

The release worktree advanced concurrently; do not reset or modify it from this
workstream.

Nothing should be staged or pushed.

## Shared process contracts

Physical exit:

    ProcessExit::{Code(i32), Signal(i32), Unknown}

Semantic stop:

    StopReason::{Timeout, Cancelled, RuntimeError}
    StopEvidence::{Stopped, NoTarget, Unknown}

    StopRecord {
        reason: Option<StopReason>,
        evidence: StopEvidence,
    }

reason=None is housekeeping. Only Stopped + Some(reason) may become a terminal
semantic stop.

StopReason::as_str() is the canonical shared spelling:

    timeout
    cancelled
    runtime_error

StopTracker is the zero-cost logical-stop escalation state:

- Unknown retains the original semantic reason for a retry;
- Stopped/NoTarget conclude the sequence and clear it;
- later unrelated cleanup is reason-less.

Platform facts:

    ManagedProcessGuard
    exit_signal(&ExitStatus) -> Option<i32>
    signal_name(i32) -> Option<&'static str>

Guard evidence:

    Ok(true)  -> Stopped
    Ok(false) -> NoTarget
    Err       -> Unknown

NoTarget is positive absence. Unknown is unresolved. Never collapse them.

Terminal truth remains minimal:

    ProcessTerminal {
        exit: ProcessExit,
        stop: Option<StopReason>,
        output: ProcessOutputMeta,
    }

Task failure detail:

    TaskTerminalDetail {
        exit: ProcessExit,
        stop: Option<StopReason>,
    }

The adapter also exposes:

    ProcessReceipt::stop_record() -> Option<StopRecord>

This is the final factual record for one logical stop sequence.

ExecutionLifecycleEvent::ProcessStop { record } is different: it is attempt-level
evidence and may fire for graceful, forced and housekeeping attempts. Products
must not emit one operator line per event.

Dotlink D2 rules through local release 0.17.5:

    reasoned final ProcessReceipt::stop_record()
      + product cancel-vs-shutdown context
      -> exactly one
         LOG job stop id=<id> reason=<reason> outcome=<delivered|gone|failed>

    ProcessTerminal.exit = Signal(n)
      + final stop record has no semantic reason
      -> exactly one
         LOG job exit id=<id> signal=<name-or-number> from=outside

The two lifecycle lines are mutually exclusive. `from=outside` says only that
dotlink did not enter a semantic stop sequence; a reasoned stop record suppresses
it even for NoTarget/Unknown evidence because direct-child fallback may have
delivered the physical signal.

## Shared Job contracts

Finite model/product refs:

    JobRef = j:<handle>[/n:<positive-coordinate>]

JobRef fields are private behind validated constructors/accessors.
Hierarchical ExecutionRef remains internal ownership/Agent-routing identity.

Budgets:

    RawPageBytes
    SerializedResultBytes

Raw page size comes from JobProfile::resolve_read_page; zero is invalid.
A one-byte raw page is valid inside a larger serialized response.

Diagnostics:

    aw_tools::diagnostic::bounded_diagnostic

Receipts:

    Receipt::JobStatus
    Receipt::NodeStatus
    Receipt::Handoff
    Receipt::Cancel
    Receipt::Ack { Finish | Through(...) }

Typed Job/Node receipts now preserve process stop=timeout|cancelled|runtime_error.

Job lifecycle remains distinct and receipts preserve:

    running
    cancelling
    revoking
    settled
    cancelled
    revoked

Do not conflate Job Cancelled/Revoked with process StopReason.

## Lightweight MCP protocol boundary

Generic MCP model projection now lives in aw-mcp-protocol.

It depends only on:

- aw-tools with default features disabled and **no aw-tools feature enabled**;
- rmcp model types with default features disabled;
- serde_json;
- Tokio timing/macros/runtime.

It deliberately does not pull:

- aw-agent-core;
- aw-agent-kernel;
- reqwest;
- jsonschema;
- clap;
- MCP client/server transport features.

The full aw-mcp client adapter depends on and re-exports this surface; there is
one projection implementation.

Shared protocol helpers:

    stored_receipt_timestamp(...)
    has_visible_receipt_timestamp(...)
    set_stored_receipt_timestamp(...)
    validate_complete_result(...)
    fit_complete_result(...)
    decorate_and_fit_complete_result(...)

    ProgressHeartbeatConfig
    ProgressHeartbeat
    ProgressHeartbeatSummary
    ProgressHeartbeatObserver
    await_with_progress_heartbeat(...)
    semantic_progress_notification(...)
    notify_semantic_progress_bounded(...)

Result rules:

- whole result _meta: max 16 KiB serialized;
- per-block metadata overhead independently bounded;
- stored receipt timestamp wins on replay;
- timestamp mutation merges into the existing io.abird/receipt object;
- malformed reserved receipt metadata fails transactionally;
- only text may shrink;
- truncation always carries the generic marker;
- typed media/resources, structured JSON, block order/metadata, result type,
  error class and result metadata are immutable;
- if immutable facts + minimal marker cannot fit, return explicit Limit;
- fit against the full envelope first: if the exact visible ts= survives,
  no sidecar reserve is charged;
- at the 32-block limit, an existing text block can carry the visible marker;
  an all-typed full block set fails explicitly.

Progress rules:

- no token => sender never polled;
- heartbeat ticks are monotonic 1,2,3...;
- heartbeat total/message are absent;
- default interval/send budget = 10 s / 100 ms;
- operation continues polling while send is backpressured;
- no detached task/queue;
- expired delivery deadline does not poll sender;
- observer gets only {attempted, delivered};
- semantic progress uses AW's real known/unknown total, never guessed percent.

## Validation after fresh review

AW:

- aw-tools: 357/357
- aw-tools-adapter: 255/255
- aw-mcp-protocol: 12/12
- aw-mcp: 20/20
- aw-platform-unix: 13/13
- live PTY: 4/4
- cross-UID fixture: 1 intentional ignore
- aw-terminal: 20/20
- no-default/minimal checks: green
- host-side aw-platform-win check: green
- aw-host / aw-cli / workspace all-feature check: green
- strict Clippy for touched crates: green
- rustfmt / diff check: green

Native Windows Job Object/exit-signal execution remains external because L5 has
no Windows Rust std target.

Dotlink after fresh-review parity fixes:

- main binary: 698 passed / 0 failed / 1 intentional ignored
- integrations: 40/40
- all-target/all-feature check: green
- strict Clippy: green
- rustfmt / diff check: green
- Markdown lint: green
- NixOS module check: green

## Dotlink pre-repin parity fixes

Current uncommitted dotlink fixes intentionally keep the old pinned binary
behavior aligned until the shared cutover:

- jobs_read(limit=0) is rejected;
- one-byte pages still work with independent response envelopes and ACK proof;
- local receipt stamping preserves fields inside reserved receipt metadata;
- malformed reserved receipt metadata fails transactionally;
- visible stored timestamps are rechecked after fitting;
- a surviving visible ts= is not charged a fake sidecar reserve;
- text fitting never silently truncates without a marker;
- typed/structured facts remain immutable.

These are temporary generic product helpers. Delete them after the same
regressions pass against the shared protocol crate.

## Publish/repin boundary

Do not publish or repin without explicit user authorization.

The final fresh-review implementation pin is
`7c7de6c9aeedf714adbe67b17ac863f27697e90e`; do not pin `eb325b31`.

The eventual direct dotlink AW dependency set is exactly:

    aw-tools
    aw-tools-fs-unix
    aw-tools-fs-win
    aw-platform
    aw-mcp-protocol

All five plus every transitive AW Git package in Cargo.lock must resolve to the
same exact published AW_IMPL_SHA.

Do not add full aw-mcp.
Do not use sibling/path overrides.

## Post-repin cutover order

D1-D3:

1. replace dotlink private process Guard/pre-spawn logic with aw-platform;
2. use shared exit_signal, StopRecord/StopTracker and retained ProcessTerminal;
3. delete stop_proven / synthetic old ProcessExit reconstruction;
4. preserve terminal facts through later spool/journal failure;
5. use ProcessReceipt::stop_record() for exactly one product stop line and
   combine it with physical ProcessExit for the mutually exclusive v0.17.5
   outside-exit line;
6. use lifecycle events only for attempt diagnostics;
7. project TaskTerminalDetail with one product compatibility mapper.

D4-D5:

1. use typed JobRef;
2. use shared receipt grammar and bounded diagnostics;
3. use JobProfile::resolve_read_page / RawPageBytes;
4. use SerializedResultBytes independently;
5. retain hostile-ref, zero/one-byte page, ACK and replay regressions.

D6-D7:

1. add aw-mcp-protocol;
2. replace local receipt/fitter helpers;
3. replace local heartbeat scheduler;
4. implement product logging as ProgressHeartbeatObserver;
5. retain typed-media/structured, tight timestamp, marker and backpressure tests.

Then rerun D9/v0.17.4 equivalence.

## Frozen work

Do not implement sandbox/mount/config-v13 convergence from this plan.
It remains a separate explicitly deferred scope.
