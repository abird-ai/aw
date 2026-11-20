# AW + dotlink 0.17.x full comprehensive handoff — 2026-11-19

Author: coordinating implementation agent
Owner: @pvl
Source authority host: **L5 only**
AW: `/home/pvl/spaces/abird/src/aw`
dotlink: `/home/pvl/spaces/abird/src/dotlink`
release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

This is the superseding end-to-end handoff for the AW/dotlink shared-runtime
0.17.x convergence workstream. It is intended to let a new agent become
productive without reconstructing the skipped conversation history.

It incorporates:

- the original AI-native filesystem/Graph/Jobs design lineage;
- unified AW Jobs + Agent runtime ownership;
- shared Toolset/output/ACK/replay convergence;
- typed MCP media;
- pre-0.17 shared convergence;
- the v0.17.0–v0.17.5 release audit;
- the stop-provenance redesign and simplification;
- AW P1–P8 implementation and fresh-review fixes;
- final AW publication/pin;
- dotlink pre-repin hardening;
- atomic repin and D1–D3 process convergence;
- the exact remaining D4–D7 cutover plan;
- frozen sandbox/config-v13 scope;
- current validation evidence and external gates.

When this file conflicts with live source/tests or the active owning plan, the
live source/tests and active plan win.

---

## 0. Authority order

Use this order whenever facts conflict:

1. direct user instruction in the current conversation;
2. live source and live tests on L5;
3. active owning plan README / DESIGN / AUDIT / TODO;
4. this handoff;
5. older dated comprehensive handoffs and archived plans;
6. release-line implementation only as behavioral reference, never as automatic
   architectural authority.

Portable semantics belong in AW. Dotlink should consume/project those semantics
and keep only product/transport/deployment policy.

---

## 1. Non-negotiable operating instructions

- Use **dotlink---l5 only** for this workstream.
- Do not use A1.
- Do not create another worktree.
- Continue primarily in:
  - AW: `/home/pvl/spaces/abird/src/aw`
  - dotlink: `/home/pvl/spaces/abird/src/dotlink`
- Preserve unrelated dirty/staged work exactly.
- AW `.agents/plans/ux/**` is reserved by another owner:
  - do not read it;
  - do not list it;
  - do not inspect its staging;
  - do not stage/reset/stash/commit it.
- Keep implementation turns around 30 minutes and stop at the next coherent
  checkpoint.
- Do not commit, push, tag, publish, release or deploy unless explicitly
  authorized for that action.
- Signed history is required; never disable commit signing.
- Revalidate Git refs/status/staging before each new implementation phase.
- Live source/tests outrank prose.
- Keep AW as the reusable semantic owner wherever practical.
- Prefer zero-cost / `Copy` / transparent newtype shared values on hot paths.
- Prefer clean/simple APIs over speculative abstraction.
- Optimize obvious hot paths only where the simpler design remains maintainable.
- Keep product strings/logging outside canonical shared state.
- Do not reintroduce old `job_*` APIs, a second JobManager, a second process
  Guard, a second result fitter, or a second progress scheduler.
- The sandbox/mount/config-v13 scope is **frozen** until the user explicitly
  promotes it. Reading the design is allowed; implementation is not.

---

## 2. Exact current Git / release state

### 2.1 AW

Current AW:

```text
HEAD        4234447612421069a746c925c76a25e3ba27774a
origin/main 4234447612421069a746c925c76a25e3ba27774a
```

Current HEAD:

```text
4234447 Record final shared 0.17 implementation pin
```

Authoritative reusable implementation pin consumed by dotlink:

```text
AW_IMPL_SHA =
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
Relax shared MCP protocol dependency ranges
```

Important AW commits in the 0.17 shared-runtime sequence:

```text
796225e  Separate process signal identity from lifecycle
63f4056  Plan shared-first 0.17 runtime convergence
021b8ce  Add shared process stop evidence types
eb325b3  Complete shared 0.17 runtime convergence
7c7de6c  Fix shared 0.17 projection boundaries
1a5d0f6  Relax shared MCP protocol dependency ranges
4234447  Record final shared 0.17 implementation pin
```

The final dotlink pin is **1a5d0f6**, not eb325b3 or 7c7de6c.

AW working tree is clean at handoff creation, excluding the opaque reserved UX
subtree.

### 2.2 dotlink

Current dotlink:

```text
HEAD        4c3b96fd5ba9e6772bc159f10a3ad893c9729887
origin/main 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
```

Latest checkpoints:

```text
46215fe  Harden pre-repin MCP and shell boundaries
0aea1f4  Record fresh 0.17.x consumer cutover
36d6533  Repin AW and complete D1-D3 process convergence
4c3b96f  Record repinned D1-D3 convergence checkpoint
```

D0 repin and D1–D3 are complete, signed and pushed.

Current direct AW Git dependencies:

```text
aw-tools
aw-tools-fs-unix
aw-tools-fs-win
aw-platform
aw-mcp-protocol
```

All direct/transitive AW Git packages in `Cargo.lock` resolve to exactly:

```text
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
```

The final repin caused **no registry package/version churn** and uses no
sibling/path override.

Dotlink working tree is clean at handoff creation.

### 2.3 Release reference

Current published release reference:

```text
release/0.17        3995448d7bf33794ef84c7bb51150d77cc260ada
origin/release/0.17 3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.5             3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.4             fd1cdb6429a341a4e3d27805cae0bec288fc26df
```

Important release references:

```text
c185558  v0.17.4 canonical stop/provenance implementation
74820ff  bound caller-controlled Job IDs in diagnostics
06d5548  raw page limit != serialized response envelope
30099aa  Nix module-owned environment hardening
5b27784  runtime-health/backport docs
8874013  ask platform for signal label in tests
f9537bb  v0.17.5 outside-signal lifecycle record
a1f8505  v0.17.5 hard execution-deadline wording
3995448  release 0.17.5
```

The release lane is behavioral reference. Do not wholesale cherry-pick its
runtime model back into current main.

---

## 3. Read this next-agent material in this order

Before editing:

1. AW `AGENTS.md`
2. AW `.agents/docs/CURRENT_HANDOFF.md`
3. this file
4. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md`
5. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md`
6. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md`
7. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md`
8. dotlink `AGENTS.md`
9. dotlink `.agents/docs/CURRENT_HANDOFF.md`
10. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md`
11. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md`
12. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/D1-D3-SOURCE-CUTOVER-MAP.md`
13. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md`
14. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/V0174-EQUIVALENCE.md`
15. live source/tests for each owner being touched.

Historical deep context:

- `.agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md`
- `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
- `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
- the superseded
  `.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md`

Use those for history, not current refs/status.

---

## Part I — chronology and design lineage

### 4. AI-native filesystem / Graph / Jobs foundations

The workstream did not begin with 0.17. The architectural base was established
during the AI-native filesystem and Job Graph design work in late October /
early November.

The design converged around:

- canonical native filesystem tools;
- Graph-native batch execution;
- durable Jobs;
- finite compact model-facing refs;
- compact token-efficient receipts;
- one execution path for foreground/background;
- immutable Graph nodes;
- explicit authority and policy;
- exact output ownership and delivery proof;
- no duplicate product Job runtimes.

Representative historical dotlink commits:

```text
c6f0e1c  spec AI-native filesystem tool surface
9b7d4b4  refine AI-native filesystem limits and edit
6c1cb16  specify filesystem status and batch grammar
f28ce2c  design batch execution graph
ebb5b84  seal AI-native filesystem design
8a3e7bb  unify durable batch controls with jobs
a156142  make jobs canonical graph runtime
bec528f  finalize canonical job graph design
81a54ce  seal reviewed AI-native job graph design
c34ea5a  seal tool design edge cases
```

Important lasting decision:

> `background` is a disposition of the same durable execution, not a separate
> execution engine.

This is why current main must never resurrect separate foreground/background
runners or old `job_output`/line-tail APIs.

---

### 5. Unified AW Jobs + Agent runtime — November 9–14

Cross-repo design moved reusable Job/Graph/Agent/session/replay/authority
semantics into AW.

Important AW checkpoints:

```text
406bfa5  Resolve unified Graph, replay, authority, and integration design
890e9a3  Resolve shared Job, replay, wake, and Graph ownership design
9e9ab04  Implement unified Jobs and Agent runtime checkpoint
0a6c6a0  Complete unified Jobs and Agent runtime
88cd965  Record finalized unified runtime pins
```

Important dotlink integration checkpoints:

```text
886e861  Align dotlink Start and Job contracts with AW design
3f59bb1  Cut over standalone Jobs to shared AW runtime
c90a660  Finalize dotlink unified Jobs bridge
```

Core ownership decision:

- AW owns reusable JobManager/runtime semantics.
- Dotlink is the concrete finite Process/Graph host.
- Dotlink must not keep a second independent Job runtime.
- Graph nodes are immutable.
- replay is identity/effect preserving;
- authority checks remain layered and explicit;
- output owner/delivery proof is correctness state, not mere presentation.

The finalized unified runtime handoff is:

```text
AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
```

---

### 6. Workspace/remote-view work — separate but protected context

AW also completed major Workspace/remote-view work in parallel:

```text
c9aaf0e  Complete delegated browser authority and surface lifecycle
aaab161  Complete workspace views UX and remote trust
62b8da6  Complete remote Workspace target integration
7d09d5f  Advance workspace views, remote resources and handoff
```

This is not the active 0.17 workstream.

Do not regress it while editing shared tools/runtime.

The reserved `.agents/plans/ux/**` subtree remains opaque to this workstream.

---

### 7. Shared Toolset / output / ACK / replay convergence — November 15–17

AW progressively absorbed portable finite-Job/tool semantics:

```text
0d2b4e3  Build configurable toolset and harden shared Job control
be6d48d  Harden shared Job output and journal uncertainty
26a8c78  Preserve strict output ACK replay across compaction
beef1bc  Harden Job writer quiescence
7e6fe20  Align shared Job list metadata
```

Important invariants established here:

- Toolset is immutable after build.
- metadata is presentation/configuration, not authority.
- finite `jobs_get/jobs_read/jobs_ack` share the canonical JobManager.
- output delivery proof is exact-owner aware.
- ACK Through advances only actually delivered bytes.
- omitted-offset Finish releases owned streams under the correct owner.
- already-committed ACK replay remains valid after another owner compacts the
  retained physical prefix.
- a peek/tail grants no delivery/ACK credit.
- no foreign-owner ACK.
- writer quiescence prevents terminal Job pruning while an OutputWriter remains
  live.
- `jobs_get` wait is settlement/event aware.
- `jobs_read` wait is byte/EOF aware.

Dotlink adoption checkpoints:

```text
d0a3922  Adopt reviewed AW Job output runtime
4f343c3  Complete AW shared-tools convergence
5381729  Complete shared Toolset metadata adoption
```

Historical full detail:

```text
AW .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md
```

---

### 8. B6 shared Toolset metadata convergence

B6 centralized canonical tool metadata/defaults:

- filesystem tool title/description/schema facts;
- shell/PowerShell/Batch descriptor facts;
- finite Job operation metadata;
- one Job profile drives advertised/runtime values;
- jobs_list lower bound lives in AW;
- list_tools/get_tool/inspection use the same canonical projection.

Product-owned dynamic facts remain product-owned, especially:

- shell execution timeout ceilings;
- output ceilings;
- profile/environment/deployment policy.

Toolset metadata is never execution authority.

---

### 9. Typed MCP media boundary

AW added a bounded raw MCP result boundary:

```text
d6009de  Preserve typed MCP media results
30dc55a  Record typed MCP media checkpoint
```

The MCP boundary preserves:

- Text;
- Image;
- Audio;
- embedded Resource;
- ResourceLink;
- structured content.

It bounds content block count / payload / metadata and fails closed on unknown
future content kinds.

Ordinary Agent `ToolOutput` remains text-only.

Real cross-repo stdio qualification proved:

```text
PNG -> image/png
WAV -> audio/wav
PDF -> application/pdf embedded resource
```

Provider/model media delivery, durable media history and consent remain separate
future work.

---

### 10. Pre-0.17 shared convergence — November 18

A semantic audit of the pre-v0.17 release line reduced missing reusable work to:

1. scoped result/receipt metadata + timestamps;
2. shared `jobs_read` page default/max;
3. dotlink synchronous shell omitted-output default.

AW checkpoints:

```text
723ec24  Add scoped receipt metadata and Job page profiles
792f0b3  Allow scoped extended Agent tool results
```

Key receipt design:

- receipt selection is explicit semantic scope;
- AW owns no wall clock;
- host supplies timestamp;
- immutable exact replay reuses stored timestamp;
- fresh `jobs_list/get/read` observation gets fresh timestamp;
- timestamps are not process start time / mtime / settlement time;
- default shared receipt timestamp policy is off.

Job read profile:

```text
default raw page = 256 KiB
shared hard max  = 8 MiB
```

Schema/runtime derive from the same profile.

Trusted Host Agent result budget may be larger than ordinary tools, but
external sources cannot self-widen it.

Dotlink pre-0.17 convergence checkpoint:

```text
4f60bc3 Complete pre-0.17 shared convergence
```

That work introduced current receipt timestamp/checkpoint behavior, current
jobs_read default behavior and the product's roughly 30-minute implementation
workflow guidance.

Important:

> 30 minutes is human/agent workflow guidance, not an execution timeout,
> transport wait, Job TTL or cancellation deadline.

---

### 11. v0.17.x release audit

The release lane was audited semantically rather than cherry-picked.

#### 11.1 v0.17.0 / v0.17.1

Already satisfied/superseded by current main/shared AW:

- single durable background shell path;
- explicit ACK/Finish ownership;
- compact Job payloads;
- token-gated progress;
- `--no-tunnel`;
- strict schemas;
- `utf8_lossy`;
- receipt count units;
- old `job_output`/line-tail surface.

Do not re-port those release-local mechanisms.

#### 11.2 v0.17.2 / v0.17.3

These are the sandbox/config-v13/mount bundle.

Representative release commits:

```text
21de8b3
e7db299
349a7d0
cb7418a
c71f426
6f48703
ba947c6
ae4d623
d855672
0983d39
```

Disposition: **deferred**.

The reusable design is frozen under AW's 017 sandbox plan. Do not implement it
without explicit promotion.

#### 11.3 v0.17.4

Canonical stop/provenance reference:

```text
c185558
```

Hardening references:

```text
74820ff  bounded caller-controlled Job diagnostics
06d5548  raw page != response envelope
30099aa  Nix module-owned environment
5b27784  runtime-health/backport docs
8874013  platform signal labels in tests
```

The audit concluded that reusable stop/process/Task/Job/MCP facts belonged in
AW, not in a permanent dotlink-local release compatibility layer.

#### 11.4 v0.17.5

New release behaviors:

```text
f9537bb  outside-signal lifecycle record
a1f8505  hard execution deadline wording
3995448  release 0.17.5
```

Current main now satisfies D1–D3/v0.17.5 process lifecycle semantics.

---

## Part II — stop/process redesign and final shared architecture

### 12. Physical exit is not lifecycle

The first shared process correction was:

```text
796225e Separate process signal identity from lifecycle
```

Canonical physical exit:

```rust
pub enum ProcessExit {
    Code(i32),
    Signal(i32),
    Unknown,
}
```

Rules:

- only reaped/host-observed OS status produces `Signal(n)`;
- timeout/cancel/shutdown never manufactures a signal number;
- cancellation/deadline/revocation/failure are not physical exit shapes.

Shared platform fact:

```rust
aw_platform::exit_signal(&ExitStatus) -> Option<i32>
```

Signal naming is presentation:

```rust
aw_platform::signal_name(i32) -> Option<&'static str>
```

No dotlink-local signal-name table.

---

### 13. Stop vocabulary evolution

The initial 021b8ce checkpoint intentionally proved invariants but had an
overloaded taxonomy. It went through discussion around:

- cancellation origin;
- stop cause;
- attempt reason;
- outcome;
- TargetGone vs Unknown;
- cleanup vs Drop.

The user explicitly asked for a simpler library-level design.

Final canonical shared vocabulary became:

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

- `Some(reason)`: semantic reason the process was meant to stop;
- `None`: housekeeping, not a semantic terminal cause;
- `Stopped`: platform has evidence the stop acted on a live target;
- `NoTarget`: positive knowledge that no live containment target remains;
- `Unknown`: resulting target state cannot be established.

Never collapse `NoTarget` into `Unknown`.

Why:

```text
NoTarget -> positive absence -> disarm -> no retry
Unknown  -> unresolved       -> may remain armed -> fail-safe retry
```

Cleanup/Drop are execution paths, not stop reasons.

Caller/parent/runtime/authority provenance is useful context but is not part of
canonical process-stop state.

---

### 14. StopTracker

Unknown retries need to preserve the original semantic reason without growing
the canonical stop enums.

Shared zero-cost state:

```rust
StopTracker
```

Behavior:

- initial `Cancelled + Unknown` remembers Cancelled;
- later housekeeping/fail-safe retry that proves Stopped yields
  `Cancelled + Stopped`;
- Stopped/NoTarget concludes/reset the sequence;
- later unrelated cleanup is reason-less.

This avoids synthetic `Dropped`/cleanup reasons and keeps the state small.

---

### 15. Terminal attribution

Only:

```text
StopEvidence::Stopped + Some(StopReason)
```

may become terminal semantic stop.

Examples:

```text
Timeout      + Stopped  -> terminal.stop = Timeout
Cancelled    + Stopped  -> terminal.stop = Cancelled
RuntimeError + Stopped  -> terminal.stop = RuntimeError

Some(reason) + NoTarget -> no terminal stop
Some(reason) + Unknown  -> no terminal stop
None + any evidence     -> no terminal stop
```

Canonical terminal:

```rust
ProcessTerminal {
    exit: ProcessExit,
    stop: Option<StopReason>,
    output: ProcessOutputMeta,
}
```

Product fields such as `stopped_by`, signal labels and prose do not belong
here.

---

### 16. Managed process-tree Guard

Platform ownership:

- Unix:
  - successful process-group signal -> Stopped;
  - ESRCH -> NoTarget;
  - other error -> Unknown.
- Windows Job Object:
  - proven empty -> NoTarget;
  - proven live + successful termination -> Stopped;
  - failed/unprovable query/termination -> Unknown.

Shared platform Guard:

```text
ManagedProcessGuard
```

NoTarget / Stopped disarm.
Unknown stays armed for final fail-safe retry.

Dotlink's previous private process-group/Job Object Guard semantics are gone
after D1.

The unsandboxed Unix `setsid` descendant-escape limitation remains a real
containment limitation; do not rewrite platform evidence merely to hide it.

---

### 17. Lifecycle events vs final logical stop

These are deliberately separate.

Attempt-level diagnostic event:

```rust
ExecutionLifecycleEvent::ProcessStop { record }
```

One logical stop may emit multiple attempt observations:

- graceful;
- forced;
- cleanup;
- fail-safe retry.

Products must not render one operator line per attempt event.

The adapter also exposes the final factual logical record:

```rust
ProcessReceipt::stop_record() -> Option<StopRecord>
```

That is the correct product-facing stop decision.

---

### 18. v0.17.5 product projection

Dotlink retains only product context required for compatibility:

```text
CancelOrigin::{Caller, Connector}
```

This context lives beside cancellation state and is not promoted back into
AW's canonical StopReason.

One product mapper drives sync result / jobs_get / Task failure / lifecycle
projection.

`stopped_by` mapping:

```text
shared Timeout      -> "timeout"
shared Cancelled:
  caller            -> "cancel"
  connector teardown-> "shutdown"
shared RuntimeError -> "failed"
```

Only proven terminal stop attribution may emit `stopped_by`.

Lifecycle operator records are mutually exclusive:

```text
reasoned final StopRecord
  -> exactly one
     LOG job stop id=<id> reason=<product reason>
                  outcome=<delivered|gone|failed>

ProcessExit::Signal(n)
+ no reasoned stop sequence
  -> exactly one
     LOG job exit id=<id> signal=<name-or-number> from=outside
```

`from=outside` means only:

> dotlink did not enter a semantic stop sequence.

It does **not** identify or blame the sender.

A reasoned stop sequence suppresses `from=outside` even when evidence is
NoTarget/Unknown, because direct-child fallback may have delivered the physical
signal.

---

### 19. Task terminal detail

Later infrastructure failure must never erase already observed process facts.

Compact shared failure detail:

```rust
TaskTerminalDetail {
    exit: ProcessExit,
    stop: Option<StopReason>,
}
```

Examples that must survive later spool/journal/internal failure:

```text
exit=Signal(9), stop=Some(Timeout)
exit=Code(1),   stop=None
exit=Unknown,   stop=Some(Cancelled)
```

The full `ProcessTerminal` remains in Job/process state.

---

## Part III — shared Job, receipt, budget and MCP architecture

### 20. Flat finite Job refs

Model/product-facing finite ref:

```text
j:<handle>
j:<handle>/n:<positive-coordinate>
```

Shared typed owner:

```rust
aw_tools::execution_ref::JobRef
```

Important separation:

- finite `JobRef` is the model/product surface;
- hierarchical `ExecutionRef` remains internal ownership/Agent-routing
  identity.

Do not flatten internal AW hierarchy into the external finite grammar.

Fields are private behind validated constructors/accessors.

---

### 21. Shared receipt grammar

Shared receipt owner:

```rust
aw_tools::receipt::{
    Receipt,
    AckMode,
    NextAction,
    render_plain,
    render_handoff,
}
```

Shared receipt types cover:

- JobStatus;
- NodeStatus;
- Handoff;
- Cancel;
- ACK Through;
- ACK Finish.

Shared rendering distinguishes Job lifecycle:

```text
running
cancelling
revoking
settled
cancelled
revoked
```

Process terminal detail is retained separately:

```text
exit=<...>
stop=timeout|cancelled|runtime_error
```

Handoff remains same Job + `no-rerun`; it is never a second process verdict.

`jobs_send` intentionally remains a tiny dotlink product receipt. Do not add
an AW Send receipt merely to remove a few product strings.

---

### 22. Bounded diagnostics

Shared generic diagnostic truncation:

```rust
aw_tools::diagnostic::bounded_diagnostic
```

Use this after D4 for generic Job diagnostics.

Product MCP `ErrorData` serialized-envelope fitting remains dotlink-owned; it
is a transport boundary, not a generic Job diagnostic.

Never reflect hostile caller-controlled Job refs into error bodies.

---

### 23. Raw page vs serialized result budget

These are different dimensions:

```rust
RawPageBytes
SerializedResultBytes
```

A one-byte source page may still require a normal-size MCP/JSON envelope.

Current pre-D5 compatibility behavior already proves:

- `limit=0` rejected;
- `limit=1` valid;
- absolute offsets remain raw-byte offsets;
- delivery proof tracks bytes actually returned;
- ACK Through uses delivered proof;
- response fitting must not mutate retained source bytes / EOF / offsets.

Shared runtime resolver:

```text
JobProfile::resolve_read_page(...)
```

D5 must make raw source reading independent from the serialized envelope
internally, not only at the tool boundary.

---

### 24. Receipt policy extraction

Receipt selection is pure copyable policy:

```rust
aw_tools::receipt_policy::{
    ReceiptCategory,
    ReceiptScope,
    ReceiptProfile,
    ReceiptMetadata,
}
```

It is feature-neutral.

Important semantics:

- host supplies clock timestamp;
- AW/protocol layer owns policy and result shaping;
- stored timestamp wins on replay;
- fresh observation gets a fresh timestamp when policy selects it.

The MCP protocol decorator takes `ReceiptProfile` directly, not a full
Toolset/catalog.

---

### 25. aw-mcp-protocol

Generic reusable MCP model projection is in:

```text
aw-mcp-protocol
```

It deliberately does not depend on the full AW MCP client adapter.

Direct dependency boundary:

- `aw-tools`, default features disabled, **no aw-tools feature enabled**;
- rmcp model types, default features disabled;
- `serde_json`;
- Tokio timing/macros.

It does **not** pull:

- aw-agent-core;
- aw-agent-kernel;
- reqwest;
- jsonschema;
- clap;
- MCP client/server transport stacks.

Full `aw-mcp` depends on and re-exports this protocol layer.

Do not add full `aw-mcp` to dotlink.

---

### 26. MCP result/receipt invariants

Shared helpers:

```text
stored_receipt_timestamp
has_visible_receipt_timestamp
set_stored_receipt_timestamp
validate_complete_result
fit_complete_result
decorate_and_fit_complete_result
```

Metadata rules:

- result `_meta` <= 16 KiB serialized;
- content-block metadata overhead bounded independently;
- timestamp mutation merges into existing
  `_meta["io.abird/receipt"]`;
- malformed non-object reserved receipt metadata fails transactionally;
- unrelated/reserved receipt fields survive timestamp mutation.

Fitting rules:

- actual serialized `CallToolResult` bytes determine fit;
- only text may shrink;
- truncation always carries an explicit marker;
- typed Image/Audio/Resource/ResourceLink remain immutable;
- structured JSON remains immutable;
- block order/metadata remains immutable;
- result metadata/type/error class remains immutable;
- if immutable facts + minimum marker cannot fit -> explicit limit error.

Timestamp sidecar logic:

1. fit against full envelope first;
2. if visible stored `ts=` survives, return with no sidecar reserve;
3. otherwise append marker;
4. if marker needs reserve, shrink the already-fitted body further;
5. do not clone whole media/structured results merely to test marker fit.

At the 32-block limit:

- existing text block may carry the marker;
- all-typed full block set fails explicitly.

---

### 27. MCP heartbeat/progress

Shared protocol API:

```rust
ProgressHeartbeatConfig
ProgressHeartbeat
ProgressHeartbeatSummary
ProgressHeartbeatObserver
await_with_progress_heartbeat
semantic_progress_notification
notify_semantic_progress_bounded
```

Semantics:

- no progress token -> sender is never polled;
- heartbeat ticks monotonically 1,2,3,...;
- heartbeat has unknown total and no fake message;
- product defaults remain:
  - interval 10 s;
  - per-send budget 100 ms;
- operation future remains polled while send is backpressured;
- no detached task/queue;
- if operation completes first, pending notification is dropped;
- expired delivery deadline does not poll sender;
- observer receives only `{attempted, delivered}`.

Real semantic progress uses the existing AW known/unknown total; never infer
percent from heartbeat count, time, revision or output bytes.

---

### 28. Transport replay boundary

AW owns:

- semantic Job replay;
- Job/effect identity;
- output ownership/delivery proof;
- Remote Start authorization;
- no-rerun/uncertainty inner semantics.

Dotlink still owns:

- OpenAI request IDs;
- serialized hosted response body caching;
- HTTP delivery leases;
- tunnel namespace/horizon;
- outer RequestReplayLedger transport policy.

Do not move the outer hosted replay ledger into AW in D4–D7.

---

## Part IV — implemented dotlink convergence

### 29. Product-only pre-repin hardening

Before the final AW repin, dotlink hardened its existing product boundaries so
the shared cutover could be behavior-preserving.

Signed commits:

```text
8a3ed9d  Harden Nix service environment ownership
a36e976  Make lifecycle logging best effort
690cfb5  Harden pre-repin runtime compatibility
8e14ee2  Record pre-repin 0.17 consumer checkpoint
46215fe  Harden pre-repin MCP and shell boundaries
0aea1f4  Record fresh 0.17.x consumer cutover
```

#### Nix

Module-owned environment keys are rejected generically:

```text
DOTLINK_PROFILE
DOTLINK_CONFIG_FILE
DOTLINK_API_KEY_FILE
NGROK_AUTHTOKEN_FILE
DOTLINK_STDIO
DOTLINK_COLOR
DOTLINK_SUPERVISOR
DOTLINK_SSH_AGENT_SOCKET
```

Allowed unrelated env remains supported, including:

```text
DOTLINK_VERBOSITY
RUST_LOG
XDG_CONFIG_HOME
XDG_STATE_HOME
```

#### Logger

Structured lifecycle/TOOL/REQ writes:

- lock stderr per record;
- best-effort/fallible;
- BrokenPipe never panics;
- safe when invoked during Drop while another panic is unwinding.

`LogConfig::lifecycle` is always visible regardless of verbosity.

#### Pre-repin D4–D7 compatibility

Before consuming shared replacements, current main locked behavior with tests:

- 32 KiB hostile Job refs do not inflate stdio/local HTTP response;
- `jobs_read(limit=0)` rejected;
- one-byte jobs_read pages work with independent response envelope;
- one-byte delivery -> ACK Through(1) -> offset 1 continuation;
- typed Image/Audio/Resource survives stdio + hosted path;
- structured content preserved at decorator boundary;
- receipt metadata merge is transactional;
- visible timestamps are replay stable;
- no duplicate visible `ts=`;
- text truncation never silently drops its marker;
- immutable typed/structured facts fail explicitly;
- progress send backpressure never delays completed operation;
- no detached/post-return progress send.

These tests are compatibility oracles for D4–D7.

---

### 30. AW publication and atomic repin

Fresh-review AW source converged through:

```text
7c7de6c Fix shared 0.17 projection boundaries
1a5d0f6 Relax shared MCP protocol dependency ranges
4234447 Record final shared 0.17 implementation pin
```

Why the 1a5d0f6 follow-up exists:

The initial repin attempt exposed that exact Tokio/serde_json requirements in
`aw-mcp-protocol` would downgrade dotlink's already compatible runtime
dependency set.

The shared package manifest was relaxed before final repin.

Final result:

- all AW Git packages pin `1a5d0f65...`;
- no registry package/version churn;
- no path override;
- full `aw-mcp` client adapter is absent from dotlink.

---

### 31. D1–D3 completed process convergence

Signed/pushed source:

```text
36d6533 Repin AW and complete D1-D3 process convergence
```

Docs checkpoint:

```text
4c3b96f Record repinned D1-D3 convergence checkpoint
```

#### D1

Removed/reduced duplicate process semantics:

- private process Guard semantics replaced by `aw_platform::ManagedProcessGuard`;
- private `stop_proven` model removed;
- synthetic Cancelled/DeadlineExceeded physical ProcessExit model removed;
- physical exit uses shared `ProcessExit`;
- semantic stop uses shared `StopRecord / StopReason / StopEvidence`;
- shared `StopTracker` preserves reason across Unknown retry;
- shared `exit_signal` extracts physical Unix signal;
- local signal-name table gone.

Dotlink retains only product cancellation context:

```text
CancelOrigin::{Caller, Connector}
```

This is intentionally not reintroduced into canonical AW stop state.

#### D2

Lifecycle operator logging uses final result, not attempt event.

Reasoned final stop:

```text
LOG job stop id=<id> reason=<reason> outcome=<delivered|gone|failed>
```

Outside physical signal with no reasoned stop sequence:

```text
LOG job exit id=<id> signal=<name-or-number> from=outside
```

Mutually exclusive.

Real binary tests cover quiet/default/verbose.

#### D3

One product mapper now drives:

- sync shell result;
- `jobs_get`;
- Task failure detail;
- lifecycle prose.

Later spool/internal/journal failure cannot erase a previously observed signal
or semantic stop.

Shared `TaskTerminalDetail` is projected into Task failure data.

Compact handoff remains a Job handle, not a second process verdict.

---

## Part V — current remaining implementation queue

### 32. Current queue summary

Only D4–D7 shared-consumer replacement remains active.

Order:

```text
D4 typed refs + shared receipts + shared diagnostics
-> focused qualification
D5 raw page vs serialized result budget separation
-> focused + combined D4/D5 qualification
D6 aw-mcp-protocol receipt/result projection
-> typed-media/metadata/tight-budget qualification
D7 shared heartbeat/progress
-> full Rust/Nix/equivalence qualification
```

Do not skip ahead by deleting all local helpers at once.

Replace one semantic area, run its compatibility oracle, then delete the old
generic implementation.

Exact map:

```text
dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/
  D4-D7-SOURCE-CUTOVER-MAP.md
```

---

### 33. D4 — typed refs

Current duplicate surface:

```text
src/mcp/tools/jobs.rs
  StandaloneRef
  parse_ref(...)
  local ExecutionRef/ExecutionSegment flat parsing
```

Replace with:

```rust
aw_tools::execution_ref::JobRef
```

Rules:

- raw transport string parsed once;
- never echo hostile ref into errors;
- typed JobRef carries trusted finite identity;
- render to string only at model/product boundary;
- checked conversion from node coordinate where host API needs `usize`;
- internal hierarchical AW ExecutionRef remains separate.

Current `JobManager::standalone_ref(...)` string construction should become
typed construction + final Display.

No second flat grammar.

---

### 34. D4 — shared receipts

Current duplicate surfaces include:

```text
bounded_job_status
bounded_job_handoff
bounded_replayed_job_handoff

ControlReceipt
bounded_control_success
bounded_control_replay
```

Shared replacement:

```rust
Receipt::{JobStatus, NodeStatus, Handoff, Cancel, Ack}
AckMode::{Through, Finish}
render_plain
render_handoff
```

For shared-backed Jobs, use authoritative shared Snapshot/NodeState/terminal.

Do not locally reclassify:

```text
running
cancelling
revoking
settled
cancelled
revoked
```

Do not parse rendered receipt prose back into trusted semantic identity.

Replay rules:

- carry typed JobRef through the call;
- validate replay identity against the trusted ref;
- original cached result/metadata remains authoritative;
- narrow response projection never replaces cache or reruns effect.

`jobs_send` stays product-owned and tiny; AW has no generic Send receipt.

---

### 35. D4 — diagnostics

Current:

```text
src/jobs.rs
  MAX_JOB_ERROR_BYTES
  bounded_job_error(...)
```

Replace generic diagnostic truncation with:

```rust
aw_tools::diagnostic::bounded_diagnostic
```

Keep dotlink product MCP `ErrorData` envelope fitting.

Mandatory regressions:

- hostile 32 KiB ref over stdio;
- hostile 32 KiB ref over local HTTP;
- narrow replay same Job/effect;
- no-rerun;
- no rendered-prose semantic parsing.

---

### 36. D5 — raw source page vs serialized response

Current behavior is correct at the outer boundary, but two internal paths still
couple raw source read to serialized payload ceiling.

Goal:

```text
RawPageBytes          = source/read semantic limit
SerializedResultBytes = response envelope limit
```

Do not use one as the other.

Preferred internal request shape:

```text
OutputPageRequest {
    requested_bytes: RawPageBytes,
    response_ceiling: SerializedResultBytes,
    ...
}
```

If changing the struct creates needless churn, unwrap exactly once at the
lowest renderer boundary; still keep the dimensions separate in APIs.

Correct sequence:

1. source window bounded by `RawPageBytes`;
2. raw offset/retained length/EOF fixed independently;
3. serialized projection may return a smaller delivered prefix;
4. only bytes actually present in validated response get delivery credit;
5. ACK proof follows that delivered prefix.

Never mutate retained source offsets/EOF to satisfy response envelope.

Use:

```text
JobProfile::resolve_read_page
```

for runtime limit validation.

Schema and runtime must derive from the same profile.

Mandatory regressions:

- `limit=0` rejected;
- `limit=1` valid;
- one-byte delivery + ACK + absolute offset continuation;
- ACK proof unchanged under response fitting;
- EOF/raw offsets remain source facts.

---

### 37. D6 — shared MCP projection

Delete local generic copies only after shared parity.

Current local helpers in `src/mcp.rs` include:

```text
fitted_text_blocks
fit_call_tool_response_to_budget
stored_receipt_ts
has_visible_receipt_ts
set_stored_receipt_ts
ensure_visible_receipt_ts
stamp_receipt_metadata
LocalMachine::decorate_and_fit_tool_response
```

Shared owner:

```rust
aw_mcp_protocol::{
    stored_receipt_timestamp,
    has_visible_receipt_timestamp,
    set_stored_receipt_timestamp,
    validate_complete_result,
    fit_complete_result,
    decorate_and_fit_complete_result,
}
```

Keep a tiny dotlink wrapper only to:

- convert product/transport result ceiling to `SerializedResultBytes`;
- choose `OperationId`;
- pass `self.shared_toolset().receipt_profile()`;
- supply host UTC timestamp;
- map shared protocol error to dotlink `McpError`;
- leave non-complete response variants unchanged.

Cached Remote Start:

- host decides when to obtain clock timestamp;
- use shared metadata mutation;
- do not fit cached state to one request's response envelope;
- immutable stored timestamp wins on replay.

Keep product-owned:

```text
unix_ts_now
fit_mcp_error_to_budget
product ErrorData envelope fitting
```

Mandatory D6 gates:

- PNG exact identity;
- WAV exact identity;
- PDF Resource exact identity;
- structured-content identity;
- unrelated metadata preserved;
- fields in reserved receipt object preserved;
- malformed reserved receipt object rejected transactionally;
- stored timestamp wins replay;
- visible timestamp survives tight fit;
- no unnecessary sidecar reserve;
- explicit truncation marker;
- immutable typed/structured result fails explicitly if impossible to fit.

---

### 38. D7 — shared heartbeat/progress

Current duplicate scheduler:

```text
src/mcp/tools/mod.rs
  PROGRESS_HEARTBEAT_INTERVAL
  PROGRESS_SEND_BUDGET
  deliver_heartbeat_bounded
  ProgressHeartbeat
  heartbeat_tick
  await_wait_or_delivery
  await_with_heartbeat
```

Replace with:

```rust
aw_mcp_protocol::{
    ProgressHeartbeatConfig,
    ProgressHeartbeat,
    ProgressHeartbeatSummary,
    ProgressHeartbeatObserver,
    await_with_progress_heartbeat,
    semantic_progress_notification,
    notify_semantic_progress_bounded,
}
```

Dotlink still owns request context:

```text
context.meta.get_progress_token()
context.peer.clone()
```

Product send closure:

```text
peer.notify_progress(param).await.is_ok()
```

Per-tick product logging may remain in closure.

Final attempted/delivered summary moves into a product observer that implements:

```rust
ProgressHeartbeatObserver::completed(summary)
```

and delegates to existing `progress_done(...)`.

No detached progress task.

Shell retains its real execution deadline when wrapping waits.

Real semantic progress only uses actual AW cumulative progress facts; never
invent percentages.

Mandatory D7 gates:

- no token -> sender never polled;
- monotonic ticks;
- no fake total/message;
- backpressure cannot delay ready operation;
- pending send dropped when operation wins;
- expired deadline does not poll sender;
- wrapper cancellation still emits attempted/delivered summary;
- hosted `jobs_get` progress remains correct;
- shell progress forwarding remains correct.

---

### 39. Final post-D4–D7 qualification

After all four slices:

- locked all-target/all-feature cargo check;
- main binary full tests;
- all integration tests;
- strict all-target/all-feature Clippy;
- rustfmt;
- `git diff --check`;
- Markdown lint;
- NixOS module checks;
- D1–D3 real lifecycle tests;
- D4 hostile refs/replay/receipt tests;
- D5 zero/one-byte/ACK tests;
- D6 typed media/metadata/timestamp/result-fit tests;
- D7 progress/backpressure/deadline tests;
- full v0.17.x equivalence matrix through published v0.17.5.

Do not claim final equivalence until this rerun is green.

---

## Part VI — product-only/deferred boundaries

### 40. Dotlink product ownership

Intentionally dotlink-owned:

- OpenAI/HTTP/stdio delivery policy;
- request replay transport ledger;
- profile/CLI/environment syntax;
- NixOS/systemd deployment;
- product wire wording;
- `stopped_by`;
- `cancel` vs `shutdown`;
- `LOG job stop ...`;
- `LOG job exit ... from=outside`;
- operator logging destination/verbosity;
- host wall clock;
- MCP ErrorData transport fitting;
- release/version bookkeeping.

Do not move product strings/policy into AW merely to reduce line count.

---

### 41. Frozen sandbox/mount/config-v13 work

AW frozen plan:

```text
.agents/plans/017-sandbox-mount-convergence-2026-11-18/
```

Dotlink deferred pointer plan exists separately.

Do not implement without explicit promotion.

Future designed AW ownership includes:

- generic SandboxMountSpec;
- MountAccess;
- required/optional sources;
- target-keyed merge/conflict validation;
- reserved environment validation;
- pinned source capabilities;
- final bind-time validation;
- descriptor-backed Bubblewrap RO/RW binds.

Future dotlink ownership includes:

- config v13;
- profile mounts;
- `DOTLINK_MOUNTS`;
- repeatable `--mount`;
- developer cache discovery;
- precedence;
- Nix integration;
- product diagnostics/docs.

This scope is not part of D4–D7.

---

## Part VII — non-regression invariants

### 42. Process facts

Never regress:

- physical exit != Job lifecycle != semantic stop;
- no requested stop invents a signal;
- NoTarget != Unknown;
- only Stopped + semantic reason populates terminal.stop;
- external signal does not produce stopped_by;
- reasoned connector stop does not become from=outside;
- unattributed signal does not lose its outside-exit record;
- later infrastructure failure does not erase observed terminal facts;
- attempt lifecycle events are not final product stop records.

---

### 43. Job/output facts

Never regress:

- one canonical shared JobManager;
- exact owner delivery proof;
- ACK Through only delivered bytes;
- Finish only correct owner;
- replay-safe committed ACK after compaction;
- no peek/tail ACK credit;
- no foreign owner ACK;
- no new ACK into compacted-away undelivered bytes;
- writer quiescence before terminal pruning;
- same Job/effect under narrow replay;
- handoff is no-rerun continuation, not rerun permission.

---

### 44. Budget facts

Never regress:

- raw page and serialized envelope are independent;
- source offset/EOF remain raw facts;
- tiny raw page still gets normal factual response envelope;
- response fitting may reduce delivered prefix, not retained source truth;
- only actually delivered bytes receive ACK proof.

---

### 45. MCP result facts

Never regress:

- typed media stays typed;
- structured content stays structured;
- result metadata/error class/type/block order remain factual;
- only text shrinks;
- truncation is explicit;
- malformed reserved metadata fails transactionally;
- stored receipt timestamp wins replay;
- fresh observation may get fresh timestamp;
- no receipt timestamp double append;
- no hidden full aw-mcp client dependency in dotlink.

---

### 46. Progress facts

Never regress:

- token-gated only;
- monotonic heartbeat;
- no guessed total/message;
- bounded send;
- operation completion owns completion;
- no detached queue/task;
- observer summary on cancellation/drop;
- semantic progress comes from real AW cumulative progress.

---

### 47. Authority/replay facts

Never infer authority from:

- labels;
- paths;
- tabs;
- presentation metadata;
- browser auth state without the correct authority chain.

Outer hosted replay remains product-owned.

Do not re-run an effect merely because a compact projection could not fit.

---

## Part VIII — validation history

### 48. Important completed validation checkpoints

Historical examples:

#### Shared Toolset / Job runtime

Repeated full AW core/adapter suites, strict Clippy, minimal/no-default builds,
downstream MCP/host/CLI, and full dotlink binary/integration qualification.

#### Typed MCP media

Real stdio PNG/WAV/PDF + synthetic typed-result bounds.

#### Pre-0.17 convergence

AW and dotlink full suites, strict Clippy, receipt timestamp assertions over
stdio and hosted EmbeddedMcp.

#### Stop/process convergence

AW:

- physical signal identity;
- final minimal stop vocabulary;
- platform Guard evidence;
- ProcessTerminal/Task detail;
- StopTracker;
- final ProcessReceipt stop record;
- receipt stop/revocation projection;
- exit_signal platform fact.

#### Final shared MCP protocol review

Latest AW shared validation before publication included:

```text
aw-tools                  357/357
aw-tools-adapter          255/255
aw-mcp-protocol            12/12
aw-mcp                     20/20
aw-platform-unix           13/13
live PTY                     4/4
cross-UID                    1 intentional ignore
aw-terminal                20/20
```

Plus:

- minimal/no-default checks;
- host-side Windows compile;
- host/CLI/full workspace;
- strict Clippy;
- rustfmt/diff;
- dependency guard proving aw-mcp-protocol enables no aw-tools feature and
  imports no Agent/reqwest/jsonschema/clap transport stack.

#### Post-repin D1–D3 dotlink

Current qualification:

```text
main binary       701 passed / 0 failed / 1 intentional ignored
integrations       42/42
```

Integration breakdown recorded in current handoff:

```text
child-env                 1/1
exit-status              15/15
OAuth live rotation       1/1
private generations      13/13
stdio tools              10/10
real lifecycle exit/stop  2/2
```

Also green:

- locked all-target/all-feature cargo check;
- strict all-target/all-feature Clippy;
- rustfmt/diff;
- Markdown lint;
- NixOS module check.

Real-binary tests prove outside SIGKILL and timeout stop semantics at
quiet/default/verbose.

---

### 49. Native Windows gate

The Windows Job Object / native exit-signal tests are authored.

L5 does not have the Windows Rust std target, so native Windows execution remains
an external gate.

Host-side Windows package compilation is green.

Do not mark native Windows execution complete from L5.

---

## Part IX — current authoritative plans / documents

### 50. AW owning plan

```text
AW .agents/plans/017-shared-runtime-convergence-2026-11-18/
  README.md
  DESIGN.md
  AUDIT.md
  TODO.md
  PUBLISH-REPIN-PREFLIGHT.md
```

P1–P8 source/publish are complete.
P9 D1–D3 is complete.
P9 D4–D7 remains open.
Final cross-repo equivalence remains open.
Native Windows remains external.

---

### 51. Dotlink owning plan

```text
dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/
  README.md
  TODO.md
  D1-D3-SOURCE-CUTOVER-MAP.md
  D4-D7-SOURCE-CUTOVER-MAP.md
  V0174-EQUIVALENCE.md
```

Current status:

```text
D0 repin   COMPLETE
D1         COMPLETE
D2         COMPLETE
D3         COMPLETE
D4         OPEN
D5         OPEN
D6         OPEN
D7         OPEN
D8         COMPLETE
D9 docs    COMPLETE; final equivalence rerun OPEN
sandbox    DEFERRED
```

---

## Part X — exact next action

### 52. Do not re-open D1–D3

D1–D3 is implemented, signed, pushed and qualified.

Do not redesign stop provenance again unless a real failing regression or a new
user requirement appears.

Do not restore:

- stop_proven;
- synthetic DeadlineExceeded/Cancelled physical exits;
- private Guard semantics;
- local signal-name table;
- lifecycle-event-per-log-line behavior.

---

### 53. Start D4

Before editing:

```bash
cd /home/pvl/spaces/abird/src/dotlink
git status --short
git diff --cached --name-status
git rev-parse HEAD
git rev-parse origin/main
git diff --check
```

Expected:

```text
HEAD == origin/main == 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
working tree clean
nothing staged
```

Revalidate AW lock entries:

```text
all AW Git packages -> 1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
```

Then execute only D4:

1. replace raw finite ref parsing with `JobRef`;
2. convert trusted internal ref construction to typed refs;
3. switch shared-backed status/handoff/control rendering to shared receipts;
4. keep tiny product `jobs_send` receipt;
5. replace generic Job diagnostic truncation;
6. stop reparsing rendered receipt prose as trusted replay identity;
7. run D4 focused tests;
8. run strict Clippy/fmt/diff;
9. update TODO/handoff;
10. pause.

Do **not** begin D5 in the same turn unless D4 is truly small and completely
green.

---

### 54. D4 focused compatibility oracle

At minimum retain:

```text
stdio_huge_job_ref_is_rejected_without_reflection
local_http_huge_job_ref_is_rejected_without_reflection

remote_start_duplicate_graph_replays_receipt_without_second_effect
remote_cancel_replay_precedes_revision_recheck
```

Also add/update tests for shared receipt grammar and typed ref construction.

---

## Part XI — definition of done

### 55. Active non-sandbox workstream is complete when

1. D4 uses typed JobRef/shared receipts/shared diagnostics.
2. D5 separates raw-page and serialized-result budgets internally.
3. D6 consumes aw-mcp-protocol and removes local generic fitter/receipt helpers.
4. D7 consumes shared heartbeat/progress and removes local scheduler.
5. All D1–D7 compatibility regressions pass.
6. Full dotlink Rust/integration/Clippy/fmt/Markdown/Nix gates pass.
7. v0.17.x equivalence matrix is fully green through published v0.17.5.
8. No duplicate reusable semantic owner remains in dotlink.
9. Native Windows gate is either executed on an eligible host or explicitly
   remains external/unclaimed.
10. Sandbox/config-v13 remains untouched unless separately promoted.

---

## Part XII — historical references

### 56. Older handoffs that explain how we got here

Use only when deeper historical detail is needed:

```text
AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
AW .agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md
AW .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md
AW .agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md
```

This 2026-11-19 handoff supersedes their state/next-action claims for the active
0.17.x convergence thread.

---

## Part XIII — copy-ready starting prompt

### 57. Starting prompt for the next agent

Copy the prompt below exactly or use the companion NEXT-AGENT file.

```text
Use dotlink---l5 only.

Continue primarily in:

/home/pvl/spaces/abird/src/dotlink

Cross-repo AW:

/home/pvl/spaces/abird/src/aw

Read-only release reference:

/home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve all unrelated work.
Do not inspect or touch AW .agents/plans/ux/**. Do not implement the frozen
sandbox/mount/config-v13 plan unless I explicitly promote it.

Keep each implementation turn to roughly 30 minutes. Aim for the next coherent
checkpoint, update durable docs, then pause.

Read first, in this order:

1. /home/pvl/spaces/abird/src/aw/AGENTS.md
2. /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md
3. /home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md
4. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
5. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
6. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
7. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
8. /home/pvl/spaces/abird/src/dotlink/AGENTS.md
9. /home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md
10. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
11. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
12. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md
13. live source/tests for every owner you touch.

Current expected refs:

AW HEAD == origin/main
4234447612421069a746c925c76a25e3ba27774a

AW_IMPL_SHA consumed by dotlink
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59

dotlink HEAD == origin/main
4c3b96fd5ba9e6772bc159f10a3ad893c9729887

release/0.17 == origin/release/0.17 == v0.17.5
3995448d7bf33794ef84c7bb51150d77cc260ada

D0 repin and D1-D3 are complete, signed, pushed and qualified. Do not redesign
or reopen them without a real regression.

Remaining active implementation is D4-D7 only:

D4 typed JobRef + shared receipts + bounded diagnostics
-> D5 RawPageBytes / SerializedResultBytes internal separation
-> D6 aw-mcp-protocol receipt/result fitting
-> D7 shared heartbeat/progress
-> full v0.17.5 equivalence.

Start with D4 only.

D4 requirements:

- replace local raw finite Job ref parser with aw_tools::execution_ref::JobRef;
- use typed JobRef as trusted replay identity instead of parsing rendered prose;
- use shared Receipt::JobStatus/NodeStatus/Handoff/Cancel/Ack for shared-backed
  Jobs;
- delete local ControlReceipt only after parity;
- keep the tiny product-owned jobs_send receipt;
- replace generic bounded_job_error with aw_tools::diagnostic::bounded_diagnostic;
- preserve hostile-ref compactness and same-Job/no-rerun replay behavior.

Before editing, revalidate status/staging/HEAD/origin/diff and verify all AW Git
lock entries still resolve to 1a5d0f65....

Run focused D4 tests, strict Clippy/fmt/diff, update the owning TODO/handoff, and
pause at that coherent checkpoint.

Do not start D5 until D4 is green.
```
