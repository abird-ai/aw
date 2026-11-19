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

### 1.2 Final host-facing stop vocabulary

The committed P1 checkpoint `021b8ce` proved the important invariants with a
transitional taxonomy. The final P1 refinement keeps only facts that change
shared process semantics:

```rust
pub enum StopReason {
    Timeout,
    Cancelled,
    RuntimeError,
}

pub enum StopEvidence {
    Stopped,
    NoTarget,
    Unknown,
}

pub struct StopRecord {
    pub reason: Option<StopReason>,
    pub evidence: StopEvidence,
}
```

Meaning:

- `Some(reason)` answers **why the process was meant to stop**;
- `None` means housekeeping with no semantic terminal cause;
- `evidence` answers **what the platform actually proved happened**;
- `NoTarget` is positive knowledge that there is no live containment target;
- `Unknown` means the result cannot be established and the target may still be
  alive.

`NoTarget` and `Unknown` remain distinct because they change Guard behavior:
`NoTarget` disarms, while `Unknown` may keep the Guard armed for a fail-safe
retry.

### 1.3 Rich provenance belongs in hooks, not canonical state

Caller/parent/runtime/authority provenance is useful context, but it does not
change the process stop primitive. Likewise explicit cleanup and Drop fallback
describe **how the attempt happened**, not **why the process should stop**.

Do not encode those dimensions in `StopReason`. Higher layers may capture them
through lifecycle hooks, logging context or product state when needed. This
keeps the shared library small without preventing dotlink or AW orchestration
from retaining richer diagnostics.

If a future provenance fact changes shared correctness or Guard behavior,
promote that fact deliberately then; do not predeclare speculative enum
variants.

A Guard retry must preserve the original semantic reason. For example, a
Cancelled attempt that returns `Unknown` and later succeeds from Drop still
records `Cancelled + Stopped`, not a synthetic `Dropped` reason.

### 1.4 Terminal attribution rule

`StopRecord::terminal_reason()` is intentionally tautological:

```text
Some(Timeout)      + Stopped  -> Timeout
Some(Cancelled)    + Stopped  -> Cancelled
Some(RuntimeError) + Stopped  -> RuntimeError

None         + any evidence -> no terminal stop
Some(reason) + NoTarget     -> no terminal stop
Some(reason) + Unknown      -> no terminal stop
```

Every `StopReason` is therefore a legitimate terminal attribution. There is no
second claimability allowlist.

Convenience constructors keep host code simple:

```rust
StopRecord::new(reason, evidence)
StopRecord::stopped(reason)
StopRecord::no_target(reason)
StopRecord::unknown(reason)
StopRecord::housekeeping(evidence)
```

The obsolete `CancellationOrigin / StopOrigin / ProcessStopCause /
StopAttemptReason / StopOutcome / StopAttempt` public names are removed.
Cleanup and Drop are no longer stop reasons.

Executed final P1 gate: focused **5/5**; full `aw-tools --all-features --lib`
**347/347**; strict all-target/all-feature Clippy; no-default-features check;
downstream `aw-tools-adapter --all-features` check; rustfmt and
`git diff --check` green.

### 1.5 ProcessTerminal

Evolve the canonical terminal toward:

```rust
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    pub stop: Option<StopReason>,
    pub output: ProcessOutputMeta,
}
```

`stop` is populated only from `StopRecord::terminal_reason()`. The terminal is
the one canonical settled process-fact record retained by `JobManager`.

Do not add product fields such as `stopped_by`, `signal_name` or prose.

## 2. Evidence-preserving process-tree guard

### 2.1 Platform primitive

AW currently has duplicate process-tree cleanup behavior across
`aw-tools-adapter`, `aw-platform*` and dotlink. Consolidate the factual
platform operation.

Keep the platform crates below `aw-tools`: they must not depend upward on
`aw_tools::StopEvidence`. The low-level stop primitive therefore returns the
zero-allocation factual shape `io::Result<bool>`:

```text
Ok(true)  -> signal/termination reached a proven live target
Ok(false) -> target was positively absent
Err(_)    -> result is unknown
```

`aw-tools-adapter` maps that losslessly and once to
`StopEvidence::{Stopped,NoTarget,Unknown}`.

Unix process-group semantics:

- successful group signal => `Ok(true)` => Stopped;
- ESRCH => `Ok(false)` => NoTarget;
- other signal error => `Err` => Unknown.

Windows Job Object semantics preserve v0.17.4's distinction:

- query active processes when possible;
- empty job => `Ok(false)` => NoTarget;
- successful termination with proven live membership => `Ok(true)` => Stopped;
- failed membership query or termination => `Err` => Unknown.

Do not claim that an unsandboxed Unix process group contains descendants that
escaped with `setsid`.

### 2.2 Adapter guard

The target platform owns the containment RAII guard; the std/async process
adapter owns only semantic stop state around it:

- platform guard owns active/disarmed state and the final fail-safe termination;
- adapter maps platform evidence into `StopEvidence`;
- shared zero-cost `StopTracker` retains the original semantic `StopReason` only
  across an Unknown retry and resets after Stopped/NoTarget;
- housekeeping uses no reason;
- typed `StopRecord` is produced at each adapter stop boundary;
- Drop retry preserves the original semantic reason rather than inventing a
  Drop reason;
- NoTarget and Stopped disarm; Unknown remains armed for retry;
- cleanup of an already-ended command uses `StopRecord::housekeeping(...)`.

Dotlink should consume this primitive rather than permanently keep a separate
semantic guard. Dotlink may still have a thin wrapper for its launcher/Bubblewrap
shape if necessary.

## 3. Execution lifecycle event sink

Do not reuse `JobEventKind`: Result/Settled/Attention are model wake events.

Add a separate host-neutral execution event:

```rust
pub enum ExecutionLifecycleEvent {
    ProcessStop {
        record: StopRecord,
    },
}
```

The event sink is optional and no-op when absent. `PipeProcess` stores only an
`Option<Arc<dyn ExecutionEventSink>>`; existing constructors install no sink,
while explicit event-aware constructors opt in.

Implemented std-side trait:

```rust
pub trait ExecutionEventSink: Send + Sync {
    fn record(&self, event: ExecutionLifecycleEvent);
}
```

The pure core owns event values, not logging. The canonical event stays small;
a host/adapter observer may capture caller/parent/runtime/authority provenance,
explicit-vs-Drop path, Job identity or product policy in its surrounding
context. Do not add a generic metadata bag or speculative provenance enum to
`StopRecord` merely for diagnostics.

A normal/external process exit may still produce a reason-less housekeeping
`ProcessStop` when the RAII guard checks/cleans descendants. Consumers must not
turn `record.reason=None` into a semantic stop claim.

`ProcessStop` is attempt-level evidence: one logical stop may emit graceful,
forced and housekeeping observations. The adapter therefore also exposes one
final factual `ProcessReceipt::stop_record()` after escalation/output cleanup;
terminal semantics are derived from the same record rather than duplicated.
Products must not print one operator line per event.

Dotlink final lifecycle mapping:

```text
reasoned ProcessReceipt::stop_record()
  -> exactly one LOG job stop id=<ref> reason=<product context> outcome=<evidence>

ProcessTerminal.exit = Signal(n)
+ final stop record has no semantic reason
  -> exactly one LOG job exit id=<ref> signal=<name-or-number> from=outside
```

The second rule is the v0.17.5 product projection. `from=outside` means only
that dotlink did not enter a semantic stop sequence; it is not canonical AW
provenance and never identifies the sender. A reasoned final stop record
suppresses the outside projection even when evidence is NoTarget/Unknown,
because a direct-child fallback may have delivered the physical signal.

The exact `LOG` strings, stderr/journald destination and quiet-mode policy stay
dotlink-owned. The event stream remains available for diagnostics/metrics.

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

Do not mechanically replace the existing Job lifecycle `job::StopReason` with
`execution_stop::StopReason`. A Job can be cancelled/revoked even when no OS
process stop is provable.

Audit cancellation call sites and map process termination to the minimal
`execution_stop::StopReason`. Retain caller/parent/runtime/authority provenance
only in the higher layer that actually needs it; avoid widening immutable
journal semantics or canonical process state for diagnostics alone.

`JobManager::ProcessSettlement` continues to carry `ProcessTerminal`; the
canonical terminal now contains orthogonal `exit` and `stop` facts. A proven
runtime stop can therefore retain both the actual reaped signal/code and the
semantic stop reason. `ProcessTerminal::status()` treats complete proven stops
as Fail even if physical exit is otherwise unknown; incomplete output remains
Unknown.

Compact shared status/receipt rendering includes only the bounded factual
`stop=timeout|cancelled|runtime_error` token. Product-specific
`stopped_by=shutdown` stays outside core rendering.

## 5. Task terminal/failure detail

### Problem

Current Task state retains only disposition. Dotlink v0.17.4 reconstructs
termination facts manually in MCP `error.data`, which can drift from Job and
process state.

### Shared design

Keep Task failure detail smaller than the full process terminal:

```rust
pub struct TaskTerminalDetail {
    pub exit: ProcessExit,
    pub stop: Option<StopReason>,
}

pub struct TaskInfo {
    // existing identity/generation/state fields
    pub terminal: Option<TaskTerminalDetail>,
}
```

The full `ProcessTerminal`, including output metadata, remains canonically
retained by JobManager. Task projection carries only the physical exit and
semantic stop facts that error projection needs. This avoided inflating the hot
`TaskReply` enum or adding heap allocation merely to expose failure detail.

A later spool/result/projection failure may make Task disposition Uncertain
while `TaskTerminalDetail` still preserves an earlier factual signal and
`stop=Timeout`. That removes the need for a separate `prior_disposition` field:
the actual deadline fact already lives in the process stop reason.

Consumers that do not expose process Tasks leave detail absent. Bounded error
text remains separate from typed facts; there is no generic metadata/JSON bag.

Product/transport layers can project this typed detail into MCP error `data`;
`aw-mcp-protocol` owns only generic MCP shaping, not product `stopped_by`
vocabulary or Task-specific policy.

## 6. Flat Job/Node references and bounded diagnostics

### 6.1 Typed flat reference

P5 adds the shared finite-Job surface type:

```rust
pub struct JobRef {
    job: Handle,
    node: Option<NodeCoordinate>,
}
```

Its only accepted/rendered grammar is:

```text
j:<handle>
j:<handle>/n:<positive-canonical-coordinate>
```

It reuses the existing bounded `Handle` and `NodeCoordinate` primitives and
has an independent maximum large enough for exactly the maximal legal flat
form. Empty handles, extra segments, leading-zero nodes, zero nodes,
hierarchical prefixes and oversized refs fail before Job lookup.

The architectural boundary is deliberate:

- hierarchical `ExecutionRef` remains the ownership, Agent-routing and event
  identity;
- finite model/product Job surfaces render `JobRef`;
- the adapter accepts prior hierarchical finite refs as compatibility input,
  resolves them once, then emits only the flat typed form;
- Agent-owned refs keep their richer hierarchical identity.

This avoids flattening AW's internal ownership model merely to match the compact
Jobs transport surface.

### 6.2 Bounded diagnostics

P5 adds one shared 16 KiB UTF-8-safe `bounded_diagnostic` helper. It is
defense in depth, not a substitute for typed parsing. Existing Task diagnostic
bounding now reuses the same helper instead of carrying a second truncation
algorithm.

Huge or malformed finite Job refs are rejected by `JobRef::parse` before they
can enter receipts. Product/transport layers may still apply the shared
diagnostic helper to unrelated caller-controlled error prose.

Dotlink's `74820ff` therefore becomes consumer cleanup plus a transport-boundary
backstop rather than a separate semantic implementation.

## 7. Shared Job receipt grammar

P5 completes the shared finite-Job receipt grammar:

- `JobStatus`;
- `NodeStatus`;
- `Handoff`;
- `Cancel`;
- `Ack(Finish/Through)`.

Every Job receipt now receives a typed `&JobRef`, never an arbitrary string.
Malformed refs therefore cannot be injected at render time.

`render_handoff(...)` first attempts the richer factual `JobStatus` with
`next=jobs_get|jobs_read no-rerun`. If and only if that fails because of the
current result budget, it falls back to:

```text
j:<handle> rev=<revision> next=jobs_get|jobs_read no-rerun
```

The fallback names the exact same durable Job and never changes or repeats the
underlying effect. If even that factual minimum cannot fit, rendering returns
`ResultLimit`.

Cancellation has an idempotent typed receipt:

```text
j:<handle> rev=<revision> cancel=accepted
j:<handle> rev=<revision> cancel=unchanged
```

ACK continues to distinguish `Finish` from `Through(offset)`, but finite
adapter surfaces now render it through the shared receipt rather than local
strings.

Rendering remains pure: no execution, raw arguments, owner delivery, clocks or
product-specific prose occur in `receipt.rs`. Replay may choose the smaller
handoff projection under a narrower envelope, but Job identity/effect/result
remain unchanged.

## 8. Page budget vs serialized response budget

P4 implements two zero-cost, non-zero, `repr(transparent)` budget types so
code cannot accidentally reuse one boundary as the other:

```rust
pub struct RawPageBytes(NonZeroUsize);
pub struct SerializedResultBytes(NonZeroUsize);
```

Responsibilities are now explicit.

`aw-tools`:

- `JobProfile::resolve_read_page(...)` resolves omitted/requested retained
  payload size to `RawPageBytes`;
- a one-byte raw page is valid even though a model/tool response requires a
  larger factual envelope;
- raw retained offsets, EOF and ownership/delivery semantics never depend on
  serialized response size.

`aw-tools-adapter`:

- `jobs_read` resolves raw page size only through `JobProfile`;
- separately captures the authenticated invocation result ceiling as
  `SerializedResultBytes`;
- a narrower serialized ceiling may fit/project fewer bytes from an already
  read raw page, but does not mutate that raw page, its retained length or its
  continuation semantics;
- only the raw prefix actually included in the fully validated response becomes
  delivery credit.

`aw-mcp-protocol`:

- owns the serialized `CallToolResult` ceiling and validates complete MCP
  payloads against `SerializedResultBytes`;
- P6 owns the richer generic complete-result fitter, metadata reservation and
  receipt decoration. The full `aw-mcp` client adapter re-exports this surface
  without duplicating it. P4 does not pull that work forward.

Executed invariant:

```text
JobProfile raw max = 1
jobs_read(limit=1)
=> exactly one retained byte consumed/credited
=> continuation offset advances by exactly 1
=> the serialized response uses the ordinary authenticated envelope
```

A separate regression proves a tighter serialized envelope may return a smaller
projection while the source `OutputPage` remains byte-for-byte unchanged.

This is the shared form of dotlink v0.17.4 `06d5548`.

## 9. MCP receipt metadata and result fitting

### 9.1 Preserve result metadata

P6 extends the public raw `McpToolResult` boundary with bounded result-level
`MetaObject`. Whole result `_meta` is capped at 16 KiB; per-content-block
metadata remains independently bounded by the existing 16 KiB metadata-overhead
rule. The text-only Agent projection still refuses typed media rather than
flattening it, while explicit raw MCP consumers retain result metadata.

`SerializedResultBytes` now means the actual serialized `CallToolResult`
size, including framing, content metadata, structured content and result
`_meta`; it is no longer approximated from payload bytes.

### 9.2 Shared receipt decorator

P6 adds these public `aw-mcp-protocol` receipt helpers. The full `aw-mcp`
client adapter re-exports them for existing AW client users:

```text
stored_receipt_timestamp(...)
has_visible_receipt_timestamp(...)
set_stored_receipt_timestamp(...)
decorate_and_fit_complete_result(...)
```

The namespaced metadata key is `io.abird/receipt`.

The decorator:

- accepts the shared copyable `receipt_policy::ReceiptProfile` directly;
- does not depend on the full Toolset/catalog or any filesystem feature;
- fits once, appends the visible marker in place, and only further shrinks that
  already-fitted body when marker reserve is needed; it does not clone a whole
  media/structured result merely to test sidecar fit;
- accepts a host-supplied timestamp but first checks stored result metadata;
- treats an existing stored `io.abird/receipt.ts` as immutable replay state,
  so it wins over a fresh host observation timestamp;
- merges the timestamp into existing bounded result metadata without deleting
  unrelated keys;
- adds a visible `ts=<unix>` sidecar only if the typed receipt/result does not
  already carry that exact line;
- reserves the exact serialized sidecar cost before fitting, making receipt
  metadata atomic with the response;
- requires the host timestamp only for a fresh selected operation.

Neither `aw-tools` nor `aw-mcp-protocol` reads the wall clock. The host chooses the
fresh observation timestamp; replayed results retain their stored timestamp.

### 9.3 Serialized result fitter

P6 implements public `fit_complete_result(...)` in `aw-mcp-protocol`.

The fitter preserves, byte/shape-for-byte where applicable:

- result `_meta`;
- `result_type`;
- `is_error`;
- content-block order;
- per-block metadata/annotations;
- typed image/audio/resource/resource-link blocks;
- structured JSON.

Only text payload bytes are shrinkable, on a UTF-8 character boundary, with a
bounded generic truncation marker. If immutable typed/structured facts plus the
minimal marker cannot fit, the fitter returns an explicit `Limit` error.

This is intentionally stricter than dotlink's current generic fallback, which
can replace an oversized typed result with text. Dotlink should consume the
shared fitter and delete that semantics-changing local fallback after parity.

## 10. MCP progress/heartbeat

The semantic filesystem progress type remains transport-independent. P7 puts
MCP model projection in `aw-mcp-protocol`, which has no client/server transport
feature at all. Dotlink and other server hosts supply their request peer as a
send closure; the full `aw-mcp` client adapter depends on and re-exports the
same protocol surface.

Implemented heartbeat surface:

```rust
ProgressHeartbeatConfig { interval, send_budget }
ProgressHeartbeat::new(...)
ProgressHeartbeat::from_token(...)
ProgressHeartbeatSummary { attempted, delivered }
ProgressHeartbeatObserver::completed(...)
await_with_progress_heartbeat(...)
```

Defaults preserve the reviewed product behavior:

```text
heartbeat interval = 10 s
per-send budget    = 100 ms
```

Both are host-tunable positive durations.

Heartbeat semantics:

- no progress token => no `ProgressHeartbeat`; awaiting is exactly the
  underlying future and the sender is never polled;
- tick values are monotonic `1, 2, 3, ...`;
- heartbeat notifications always have `total=None` and `message=None`; AW
  never invents a completion percentage;
- numeric ticks stop before they can lose integer monotonicity in MCP's f64
  representation;
- every delivery gets an independent deadline
  `min(now + send_budget, operation_send_deadline)`;
- if the operation's send deadline is already expired, the transport sender is
  not polled;
- while a notification send is pending, the underlying operation remains in
  the same biased `select!` and can complete immediately. Progress
  backpressure therefore cannot add even the per-send budget to operation
  completion latency;
- no queue or detached Tokio task is created;
- after the wrapper returns, no later heartbeat is possible;
- the final operation result alone owns completion; heartbeats never publish a
  synthetic final/100% notification.

The optional observer receives only `{attempted, delivered}`. A small RAII
guard reports the summary on normal completion *and* if the heartbeat wrapper
is itself cancelled/dropped. Product tool names/log destinations stay outside
the shared API.

P7 also integrates real semantic progress through:

```rust
semantic_progress_notification(...)
notify_semantic_progress_bounded(...)
```

These map the existing `aw_tools::filesystem::progress::End::{Known,Unknown}`
onto MCP without coupling the semantic type to rmcp. Known totals are validated,
unknown totals stay absent, the existing 64-byte/control-free phase/message
contract is retained, and counters must remain exactly representable by MCP's
numeric field. Real semantic notifications use the same independently bounded
delivery rule as heartbeats.

Dotlink should obtain the request progress token from its rmcp
`RequestContext`, build `ProgressHeartbeat::from_token(...)`, and pass a
closure around `peer.notify_progress(...)`. Its logging layer implements the
optional observer.

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
ProcessExit::Signal(n)                 -> signal: n
StopReason::Timeout + Stopped         -> stopped_by: "timeout"
StopReason::Cancelled + Stopped       -> stopped_by: "cancel" or "shutdown"
StopReason::RuntimeError + Stopped    -> stopped_by: "failed"
```

The shared `Cancelled` value deliberately does not choose dotlink's `cancel` vs
`shutdown` wording. Dotlink retains that product context at the stop boundary
through its higher-layer state/observer. No `stopped_by` is produced for
`NoTarget`, `Unknown`, or housekeeping with `reason=None`.

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
