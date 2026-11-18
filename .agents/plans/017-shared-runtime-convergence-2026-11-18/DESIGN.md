# Design

Parent: [README.md](README.md)

## 1. Shared process provenance

### 1.1 Physical exit

Keep:

```rust
pub enum ProcessExit {
    Code(i32),
    Signal(i32),
    Unknown,
}
```

Only a reaped/host-observed OS status may produce `Signal(n)`. A requested
termination never invents a signal number.

### 1.2 Cancellation origin

Add one portable origin type. Initial design:

```rust
pub enum CancellationOrigin {
    Caller,
    Runtime,
    Authority,
}
```

Before freezing the enum, audit parent/subagent cancellation paths. Add a
separate `Parent` variant only if those paths are observably distinct and
cannot truthfully map to `Runtime`.

Meaning:

- `Caller`: explicit owner/user/model-facing control request;
- `Runtime`: host shutdown, restart, runtime replacement or owner teardown;
- `Authority`: grant/source/policy revocation or replacement.

This type is factual metadata, not authority.

### 1.3 Claimable process stop cause

Add:

```rust
pub enum ProcessStopCause {
    Timeout,
    Cancelled(CancellationOrigin),
    Failed,
}
```

This is a claim about why the runtime proved it stopped the waited command. It
must never be populated from lifecycle status alone.

### 1.4 Stop-attempt evidence

Add separate types:

```rust
pub enum StopAttemptReason {
    Timeout,
    Cancelled(CancellationOrigin),
    Failed,
    Cleanup,
    Dropped,
}

pub enum StopOutcome {
    Delivered,
    AlreadyGone,
    Failed,
}

pub struct StopAttempt {
    pub reason: StopAttemptReason,
    pub outcome: StopOutcome,
}
```

Rules:

- only `Delivered` can support a terminal `ProcessStopCause`;
- `Cleanup` and `Dropped` never become a terminal cause;
- `AlreadyGone` means desired end state but not a stop performed by us;
- `Failed` means the stop is unproven and the guard remains armed where a
  fail-safe retry is meaningful;
- a later failure cannot relabel an earlier timeout/cancel stop.

### 1.5 ProcessTerminal

Evolve the canonical terminal:

```rust
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    pub stop: Option<ProcessStopCause>,
    pub output: ProcessOutputMeta,
}
```

The terminal is the one canonical settled process-fact record retained by
`JobManager`.

Do not add product fields such as `stopped_by`, `signal_name` or prose.

## 2. Evidence-preserving process-tree guard

### 2.1 Platform primitive

AW currently has duplicate process-tree cleanup behavior across
`aw-tools-adapter`, `aw-platform*` and dotlink. Consolidate the factual
platform operation.

The platform primitive should return a typed outcome rather than unit success:

```text
Delivered
AlreadyGone
Failed
```

Unix process-group semantics:

- successful group signal => Delivered;
- ESRCH => AlreadyGone;
- other signal error => Failed.

Windows Job Object semantics must preserve v0.17.4's distinction:

- query active processes when possible;
- empty job => AlreadyGone;
- successful termination with proven live membership => Delivered;
- unprovable query/termination => Failed.

Do not claim that an unsandboxed Unix process group contains descendants that
escaped with `setsid`.

### 2.2 Adapter guard

The std/async process adapter owns a small RAII guard around the platform
primitive:

- active/disarmed state;
- stop attempt reason supplied by caller;
- typed `StopAttempt` result;
- optional lifecycle-event emission;
- Drop performs `Dropped` fail-safe when still armed;
- failed stop remains armed for Drop retry;
- cleanup of an already-ended command records `Cleanup`, never terminal cause.

Dotlink should consume this primitive rather than permanently keep a separate
semantic guard. Dotlink may still have a thin wrapper for its launcher/Bubblewrap
shape if necessary.

## 3. Execution lifecycle event sink

Do not reuse `JobEventKind`: Result/Settled/Attention are model wake events.

Add a separate host-neutral execution event:

```rust
pub enum ExecutionLifecycleEvent {
    ProcessStopAttempt {
        attempt: StopAttempt,
    },
}
```

The event sink should be optional and zero-cost/no-op when absent.

Possible std-side trait shape:

```rust
pub trait ExecutionEventSink: Send + Sync {
    fn record(&self, event: ExecutionLifecycleEvent);
}
```

The pure core owns event values, not logging.

Dotlink sink mapping:

```text
ProcessStopAttempt
  -> LOG job stop id=<ref> reason=<...> outcome=<...>
```

The exact `LOG` string, stderr/journald destination and quiet-mode policy stay
dotlink-owned.

Because the sink can be invoked from Drop, dotlink's logger must use fallible
non-panicking writes as v0.17.4 does.

## 4. Job lifecycle integration

Keep Graph/Job lifecycle separate from process stop provenance.

Existing Job lifecycle:

- Running;
- Cancelling;
- Revoking;
- Terminal;
- StopReason::Cancelled / Revoked.

Do not mechanically replace `StopReason` with process stop cause. A Job can be
cancelled/revoked even when no OS process stop is provable.

Audit cancellation call sites and carry `CancellationOrigin` where it affects
process termination. Avoid widening immutable journal semantics unless origin is
actually needed for replay/correctness.

`JobManager::ProcessSettlement` continues to carry `ProcessTerminal`, so
adding `terminal.stop` automatically makes the canonical manager the owner of
settled stop facts.

Compact shared status/receipt rendering may include a terse stop token only if
it remains bounded and factual. Product-specific `stopped_by=shutdown` stays
outside core rendering.

## 5. Task terminal/failure detail

### Problem

Current Task state retains only disposition. Dotlink v0.17.4 reconstructs
termination facts manually in MCP `error.data`, which can drift from Job and
process state.

### Shared design

Add optional typed terminal detail to the host Task projection.

Candidate shape:

```rust
pub struct TaskTerminalDetail {
    pub process: Option<ProcessTerminal>,
    pub prior_disposition: Option<TaskDisposition>,
}
```

The exact final shape should reuse existing Task/Job state without duplicating
strings.

Requirements:

- a spool/internal/projection failure after process settlement retains the
  already observed process terminal;
- a failure after an actual deadline can retain the prior timed-out
  disposition;
- successful Tasks pay no extra allocation;
- consumers that do not expose process Tasks can leave detail absent;
- bounded error text remains separate from typed factual detail.

Extend `TaskInfo` / relevant host Task projection only where needed. Do not
turn Task detail into a generic unbounded JSON bag.

`aw-mcp` maps this typed detail to MCP error `data`.

## 6. Flat Job/Node references and bounded diagnostics

### 6.1 Typed flat reference

Dotlink's flat Jobs surface uses `j:<handle>[/n:<coordinate>]`. Add a shared
bounded type backed by existing `execution_ref::Handle` and
`NodeCoordinate`.

Candidate:

```rust
pub struct JobRef {
    pub job: Handle,
    pub node: Option<NodeCoordinate>,
}
```

It owns parse/render for the flat surface.

Transport strings are parsed once. Internal Job operations should consume typed
bounded identity where practical.

### 6.2 Bounded diagnostics

Retain a shared bounded diagnostic helper as defense in depth even after typed
parsing. User-controlled raw strings must never reach an unbounded `Display`.

Dotlink's `74820ff` then becomes mostly a consumer cleanup rather than a
separate semantic fix.

## 7. Shared Job receipt grammar

Current shared receipt support:

- JobStatus;
- NodeStatus;
- Ack(Finish/Through).

Extend it so dotlink does not own a second control/handoff grammar.

Add typed variants or equivalent constructors for:

- durable Job handoff/continuation;
- cancellation accepted/applied;
- ACK/Finish outcome;
- effect-applied-but-full-status-did-not-fit fallback.

Requirements:

- every receipt is built from trusted typed facts;
- no execution occurs during rendering;
- no raw tool arguments;
- whole factual receipt fits or returns `ResultLimit`;
- continuation always names the same durable Job and says no-rerun;
- replay projection may become more compact under a narrower envelope but never
  changes the underlying effect/result.

Exact model wording can remain configurable where product wording genuinely
differs, but the durable semantics and bounded structure stay shared.

## 8. Page budget vs serialized response budget

Introduce zero-cost distinct budget types so code cannot accidentally reuse one
as the other.

Candidate:

```rust
pub struct RawPageBytes(usize);
pub struct SerializedResultBytes(usize);
```

Responsibilities:

`aw-tools`:

- resolve omitted/requested raw page against `JobProfile`;
- preserve raw offsets and page semantics.

`aw-mcp`:

- own serialized `CallToolResult` envelope ceiling;
- reserve protocol/receipt overhead;
- fit only within authenticated/host result ceiling.

Invariant:

```text
jobs_read(limit=1)
=> raw page <= 1 byte
=> serialized response may use the full transport/host envelope
```

This is the shared form of dotlink v0.17.4 `06d5548`.

## 9. MCP receipt metadata and result fitting

### 9.1 Preserve result metadata

AW MCP result boundaries must retain bounded MCP result `_meta`, not only
content/structured/error.

### 9.2 Shared receipt decorator

Move the generic logic currently in dotlink into `aw-mcp`:

- inspect Toolset ReceiptProfile;
- accept host-supplied/stored timestamp;
- store namespaced receipt metadata;
- add visible timestamp content only when needed;
- do not duplicate an existing compact typed receipt timestamp;
- preserve raw text, structured JSON and typed media payload identity.

The core still owns no wall clock. Host decides fresh observation vs immutable
replay timestamp.

### 9.3 Serialized result fitter

Move generic complete-`CallToolResult` fitting into `aw-mcp`:

- preserve `_meta`;
- preserve `is_error`;
- do not corrupt typed media/structured content;
- reserve receipt overhead atomically;
- return an explicit fit error when even the minimal factual envelope cannot
  fit.

Dotlink should stop maintaining its own generic MCP result fitter.

## 10. MCP progress/heartbeat

The semantic filesystem progress type remains transport-independent.

Add an MCP adapter utility in `aw-mcp` for long waits that have no meaningful
total:

- active only when a progress token exists;
- monotonic tick counter;
- bounded per-tick send deadline;
- progress delivery never delays the underlying operation;
- attempted/delivered counts available to an optional host observer;
- no detached queue/task;
- final result, not progress, owns completion.

Dotlink supplies peer/request context and optional logging observer.

## 11. Signal naming

Add a small platform projection utility for known signal numbers where the
platform has stable names.

Example API:

```rust
pub fn signal_name(number: i32) -> Option<&'static str>;
```

AW semantic state stores only the number. Product prose remains product-owned.

## 12. Dotlink compatibility mapping

Final dotlink v0.17.4 wire mapping:

```text
ProcessExit::Signal(n)          -> signal: n
ProcessStopCause::Timeout      -> stopped_by: "timeout"
Cancelled(Caller)              -> stopped_by: "cancel"
Cancelled(Runtime|Authority)   -> stopped_by: "shutdown"
ProcessStopCause::Failed       -> stopped_by: "failed"
```

No `stopped_by` when stop outcome was AlreadyGone/Failed or only
Cleanup/Dropped evidence exists.

Dotlink's outside-signal prose and signal-name wording remain its responsibility.

## 13. Explicit non-goal: outer transport replay extraction

Do not move dotlink's OpenAI `RequestReplayLedger` in this effort.

It owns:

- hosted request IDs;
- serialized response body caching;
- HTTP delivery leases;
- hosted 120-second horizons;
- tunnel namespace changes;
- OpenAI protocol-header semantics.

AW already owns the reusable inner layers:

- Job replay identity;
- output owner/delivery proof;
- Remote Start authorization;
- no-rerun/uncertainty semantics.

Revisit transport replay extraction only if a second transport needs the same
outer Ready/Delivered/Uncertain lease/tombstone state machine.

## 14. Deferred sandbox design

The complete design is intentionally parked in the sibling sandbox plan. No
active implementation phase here may modify it.
