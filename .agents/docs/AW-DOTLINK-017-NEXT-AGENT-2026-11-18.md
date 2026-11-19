# AW + dotlink 0.17 shared-runtime next-agent entrypoint — P9

Use **dotlink---l5 only**.

Continue in:

    /home/pvl/spaces/abird/src/aw

Dotlink main:

    /home/pvl/spaces/abird/src/dotlink

Release reference:

    /home/pvl/spaces/abird/src/dotlink-017-mounts
    release/0.17 @ 887401398da297c458ffa2a152a07e1b85870c35

Do not use A1/X2. Do not create another worktree. Preserve unrelated dirty or
staged work. Do not stage, commit, push, tag, release, deploy or publish unless
the user explicitly authorizes it.

Keep each implementation turn near 30 minutes. Aim for one coherent technical
checkpoint, update durable docs, then pause.

## Read first

1. `/home/pvl/spaces/abird/src/aw/AGENTS.md`
2. `/home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md`
3. `/home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md`
4. `/home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md`
5. `/home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md`
6. `/home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md`
7. `/home/pvl/spaces/abird/src/dotlink/AGENTS.md`
8. `/home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md`
9. `/home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md`
10. live source/tests for every owner you touch.

The frozen sandbox plan may be read for context only:

    /home/pvl/spaces/abird/src/aw/.agents/plans/017-sandbox-mount-convergence-2026-11-18/README.md

Do **not** implement sandbox/mount/config-v13 work unless the user explicitly
promotes it.

## Exact expected refs

AW:

    HEAD        021b8cec7b9122a6f6acaf5ef9137fb31f0a6659
    origin/main 792f0b38d104659264b70c31736655df1a46b60f

Dotlink main:

    HEAD        6598de605a006a75473a2043de3f60db384b0d02
    origin/main 5381729cd469eda54691e540deb3fee13e9f3e3e

Release authority:

    release/0.17 = 887401398da297c458ffa2a152a07e1b85870c35
    v0.17.4 tag = fd1cdb6429a341a4e3d27805cae0bec288fc26df

Nothing should be staged. Dotlink source should still have no diff; only its
017 consumer-plan README/TODO should be dirty.

Before editing, revalidate:

- HEAD/origin for all three checkouts;
- `git status`;
- staged files;
- `git diff --check`;
- frozen sandbox paths have no worktree or staged diff;
- dotlink source/Cargo files remain untouched.

## Current phase

**P1-P7 are implemented in the AW working tree. P8 host-available technical
validation is complete. Start P9 only.**

Do not redo P1/P2. In particular, ignore historical text describing
`StopOrigin`, `Cleanup`, `Dropped`, `ProcessStopCause`,
`StopAttemptReason`, `StopOutcome` or `StopAttempt`. Those are obsolete.

## Canonical shared contracts to consume

### Process stop

    StopReason::{Timeout, Cancelled, RuntimeError}
    StopEvidence::{Stopped, NoTarget, Unknown}

    StopRecord {
        reason: Option<StopReason>,
        evidence: StopEvidence,
    }

`reason=None` means housekeeping. Only `Stopped + Some(reason)` may become a
terminal semantic stop cause.

Physical exit remains orthogonal:

    ProcessExit::{Code(i32), Signal(i32), Unknown}

Never synthesize a signal number from a stop request.

Platform stop proof is below aw-tools:

    aw-platform-* -> io::Result<bool>
      Ok(true)  => Stopped
      Ok(false) => NoTarget
      Err       => Unknown

`ManagedProcessGuard` disarms on Stopped/NoTarget and stays armed on Unknown.
The final pre-publish D1 platform fact is also implemented:
`aw_platform::exit_signal(&ExitStatus) -> Option<i32>` owns Unix physical
signal extraction (Windows returns `None`), and the AW process adapter consumes
the same helper. Fresh qualification is Unix platform **13/13 + 4/4 PTY**,
adapter **256/256**, terminal **20/20**, strict affected Clippy, minimal configs,
downstream host/CLI and workspace all-feature compile. The native Windows Job
Object/exit-signal assertions are authored but cannot run on L5 because the
Windows Rust std target is not installed.

### Process terminal / Task detail / events

    ProcessTerminal {
        exit: ProcessExit,
        stop: Option<StopReason>,
        output: ProcessOutputMeta,
    }

    TaskTerminalDetail {
        exit: ProcessExit,
        stop: Option<StopReason>,
    }

    ExecutionLifecycleEvent::ProcessStop { record }

Do not conflate Job cancellation/revocation with physical process exit.

### Budgets

    RawPageBytes
    SerializedResultBytes

Raw retained page limits come only from `JobProfile::resolve_read_page`.
Serialized MCP/result envelope limits are separate.

A one-byte raw page is valid inside a larger serialized response.

### Finite Job refs

    JobRef = j:<handle>[/n:<coordinate>]

Finite model/product output uses flat `JobRef`.
Hierarchical `ExecutionRef` remains internal ownership/Agent-routing identity.

### Shared finite receipt grammar

    Receipt::JobStatus
    Receipt::NodeStatus
    Receipt::Handoff
    Receipt::Cancel
    Receipt::Ack {
        AckMode::Finish | AckMode::Through(...)
    }

`render_handoff` may fall back to a smaller receipt under a tight envelope but
must keep the exact same Job and explicit `no-rerun`.

### Diagnostics

Use shared UTF-8-safe:

    aw_tools::diagnostic::bounded_diagnostic

Do not recreate unbounded product ref/error formatting.

### MCP result metadata / fitting

Shared `aw-mcp` surface includes:

    McpToolResult { content, structured_content, is_error, meta }

    stored_receipt_timestamp(...)
    has_visible_receipt_timestamp(...)
    set_stored_receipt_timestamp(...)
    decorate_and_fit_complete_result(...)
    fit_complete_result(...)

Shared receipt metadata key:

    io.abird/receipt

Stored `ts` wins on replay. Fresh selected results receive a host-supplied
timestamp.

The fitter measures actual serialized `CallToolResult` bytes.
Only text may shrink. Typed media/resources, structured JSON, result metadata,
result type, error class, content-block order and block metadata are immutable.
If immutable facts cannot fit, return an explicit limit error rather than
rewriting them as prose.

### MCP progress

Shared `aw-mcp` surface:

    ProgressHeartbeatConfig
    ProgressHeartbeat
    ProgressHeartbeatSummary
    ProgressHeartbeatObserver
    await_with_progress_heartbeat(...)
    semantic_progress_notification(...)
    notify_semantic_progress_bounded(...)

Dotlink owns extracting the rmcp request progress token and peer. Build
`ProgressHeartbeat::from_token(...)` and provide
`peer.notify_progress(...)` through the send closure.

Heartbeat defaults are 10 s interval / 100 ms per-send budget. Heartbeats are
monotonic, unknown-total, no-message ticks. No detached task/queue exists.
While progress delivery is blocked, the operation must continue being polled.
The optional observer carries attempted/delivered counts into dotlink logging.

Real semantic progress must use AW's existing
`filesystem::progress::End::{Known,Unknown}`; never invent percentages.

### Signal naming

Use the shared platform `signal_name(number)` helper for presentation hints.
Canonical process state stores the numeric signal only.

## P8 validation baseline

Current working tree is green on L5:

- `aw-tools --all-features --lib`: **354/354**
- `aw-tools-adapter --all-features --lib`: **256/256**
- `aw-mcp --all-features --lib`: **33/33**
- `aw-platform-unix`: **13/13**
- Unix live PTY integrations: **4/4**
- cross-UID Unix fixture: intentionally ignored
- `aw-terminal --lib`: **20/20**
- strict all-target/all-feature Clippy for touched crates
- no-default `aw-tools`
- batch-only `aw-tools`
- no-default adapter
- no-default Unix platform
- host-side Windows platform check
- downstream `aw-host`, `aw-cli`
- full workspace all-feature compile
- rustfmt and `git diff --check`

P8 API review also made `ProgressHeartbeatConfig` and `JobRef` fields private
behind validated constructors/accessors.

## P9 objective — dotlink thin consumer

Only now begin the dotlink source cutover.

The companion D-items map to P9:

1. Replace dotlink-local stop/cause/process-terminal reconstruction with shared
   AW stop/terminal facts.
2. Project shared `TaskTerminalDetail { exit, stop }` into MCP error data
   without erasing reaped exit/signal behind later internal failures.
3. Replace local raw finite Job-ref parsing with `JobRef`; keep product
   transport error bounding.
4. Replace local handoff/status/cancel/ACK grammar with shared receipts.
5. Consume `RawPageBytes` / `SerializedResultBytes`; preserve raw
   offsets/EOF/delivery/ACK proof.
6. Replace local MCP receipt metadata helpers and generic result fitter with
   shared aw-mcp helpers.
7. Replace local heartbeat helper with shared heartbeat/progress adapter and
   implement dotlink logging as `ProgressHeartbeatObserver`.
8. Keep only genuinely product-owned mappings:
   - exact `stopped_by="cancel"|"shutdown"|"timeout"|"failed"` wording;
   - operator log destination/text;
   - transport response windows;
   - config/CLI/env/Nix behavior;
   - outer OpenAI request replay ledger.
9. Run the v0.17.4 equivalence matrix and delete duplicate local generic
   semantics only after parity is proved.

Important release references:

    c185558  stop/provenance
    74820ff  bounded Job diagnostics
    06d5548  raw page vs serialized envelope
    30099aa  Nix environment ownership
    5b27784  runtime-health/backport docs
    8874013  platform signal-label test cleanup

Do not mechanically copy release-local architecture. Use those commits as
behavioral reference only.

## Authority-gated items — do not perform automatically

P8 still has three intentionally open items:

- signed AW implementation checkpoint;
- publish/push that exact AW checkpoint;
- atomically repin all dotlink AW dependencies to that exact published commit.

They require explicit user authorization. P9 may be implemented on the current
dirty shared working tree if the user directs continuation, but do not create a
fake pin to an unpublished/dirty state.

## Checkpoint discipline

At the end of each P9 turn:

- run focused tests plus the relevant full affected suite;
- run strict Clippy for touched crates;
- run rustfmt and `git diff --check`;
- verify frozen sandbox paths remain untouched;
- verify unrelated work is preserved;
- update AW master plan and dotlink companion plan;
- pause at the next coherent boundary.
