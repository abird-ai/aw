# aw runtime — four primitives, two hook verbs, derived services

**Status:** Proposed design, user-directed 2026-10-11. **NOT IMPLEMENTED** by this document. This is the canonical target architecture for the unnumbered AW-P6 consolidation track, not Phase 69, a database migration, an agentc executable compatibility target, or a claim about native certification. Implementation instructions, dependencies and gates: [Runtime contract](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md). Normative security and hook details: [Hooks, trust and policy](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md).

**Grounded implementation baseline:** aw `crates/aw-agent-kernel`, `crates/aw-agent-core`, `crates/aw-tools`, `crates/aw-mcp`, `crates/aw-extensions`, `crates/aw-terminal`, `crates/aw-ui-extension`, `aw-store`, `aw-store-jsonl` and their feature cuts at Git `6b3a426` (the handoff HEAD; inspect live Git before work). Source `0.15.0`; SQLite schema `v33`; E1–E5 already separately proven in their defined scopes; E6 parked. No tool defaults change, remote/browser privilege broadening, new release, push, tag or existing-source edit is authorized merely by this plan.

**Reference inputs:** Agentc P6 `AGENTS.md`, `P6-HANDOFF.md`, `P6-RUNTIME.md` (R-D1…R-D31, `2.1), `P6-PRIMITIVES.md`, `P6-SURFACE.md`, `P6-API.md`, `P6-EXTENDING.md`, P6-ACTORS/SIGNALS/HOOKS/TRUST/SUBAGENTS/DECIDERS/INTEGRATION/SETTINGS/STATUS/VOICE; actual agentc headers, C actor and job loop, event and extension code were inspected separately. Agentc is a **design reference**: no external executable conformance, binary invocation, source port, imported C ABI, or automatic future synchronization.

---

## 0. Foundational decision

There are exactly **four semantic runtime primitives**:

1. **Runtime** — host-driven scheduler, bounded continuation registration, time, wakeups, actor ownership and coordinated shutdown.
2. **Actor** — a scoped addressable unit of work with a stable generation-fenced identity, lineage, state, an inbox and an effective Envelope. Exactly two kinds: `Agent` or `Process`; PTY is a process mode, not a kind.
3. **Message** — the only addressed communication unit, with class `Control | Input | Notify | Hook`, typed kind, target, correlation, bounded payload, delivery ordering and provenance. A queue is its scheduled `Input` subset; a signal is its `Control` subset.
4. **Envelope** — the immutable effective authority/resource ceiling attached to an Actor, composed monotonically from a host-owned root and restrictions. It never originates from model/provider/MCP/extension text.

There are exactly **two hook verbs**: **next** (the same/replaced/consumed message) and **defer** (park and resume via a bounded Runtime continuation). Branch/emit/fork, approvals, routing, decisions, tools, child Sessions, queue, journal, status, prompts, settings, voice, RPC and persistence are services/combinators or adapters over those primitives. **Do not introduce a fifth runtime primitive or parallel Agent/control/journal engine for a feature.**

This is a conceptual algebra, not an instruction to merge every Rust module into four physical crates. Rich Rust types, platform adapters and storage modules are necessary to **enforce** the same four concepts. There is no general-purpose `PolicyEngine` or `ApprovalManager` that can execute independently of Runtime + Actor + Message + Envelope. The durable journal is a mandatory host service at the effect boundary, never an optional hook.

### 0.1 Why aw differs from agentc's C implementation

| Property | P6 conceptual goal | aw implementation decision |
|---|---|---|
| Primitive vocabulary | Four primitives | Adopt identically |
| Actor kinds | Agent and process | Adopt identically |
| Scheduler | One cooperative C pump, no core threads | One logical coordinator and deterministic admissions; Tokio tasks in the hosted std adapter, host-pumped bounded stepping in embedded |
| Agent execution | Synchronous `agentc_agent_submit` loop | **Keep the existing one** Rust Agent and shared no_std `TurnMachine`; no duplicate provider/tool loop |
| Lifecycle ownership | Process-local registry | Generation-fenced actor references pinned to owner Session, principal and execution site |
| One queue/message bus | Turn and priority | Adopt; strict stale-target refusal and no unsafe mid-effect preempt |
| Envelope | Workspace/RW/auto by default | Same algebra; **ToolPolicy Off/no implicit grants** by default |
| Durability | JSONL custom control records | Existing durable effects/receipts, SQLite v33 and bounded JSONL/memory-only feature cuts |
| Hooks | Next or defer on ten stages | Same verbs/stages, **typed stages and immutable security/evidence fields** |
| Model review | Decider can propose multiple effects | Reviewer returns evidence-based proposal; host must independently admit each effect |
| UI | One TUI | Shared native + SolidJS projection, no browser-specific Agent |
| Low level | Freestanding C23 OS layer | Host Rust std OS adapters and no_std+alloc pure kernel + E5 versioned C ABI |

## 1. Canonical names and ordering

Terminology is normative across native UI, SDK, RPC, persistence and embedded traces:

- **Session**: durable user-owned conversation/work binding; child Agent work is a normal child Session, not an alternative in-memory transcript type.
- **Turn**: one user-/controller-admitted prompt to a settled outcome, containing one or more model **steps**. Never use "turn" to mean one provider HTTP request.
- **Step**: one model invocation + its proposed complete tool batch, if any. Several tool calls in a batch are **one step**. An invocation attempt and a retry candidate are not automatically two steps.
- **Transcript message**: user/assistant/tool/system message with content blocks. This is **not** the same type as an addressed runtime `Message`. Explicitly use `TranscriptMessage` vs `ActorMessage` (or equivalent) in Rust.
- **Block**: text, private reasoning/thinking, tool call, and future typed media. Provider-private signed content is not normal transcript text.
- **Call/batch/result**: complete provider proposal; one immutable ID per tool call; a result is correlated to the exact call, even on denial, with source-order publication.
- **Attempt**: separately identified model dispatch or external effect. Paid usage may be unknown after loss of acknowledgement; "retry" cannot reuse a possibly executed attempt.
- **Settle**: the terminal recorded Turn result; Actor may stay live for later Turns.
- **Actor**: Agent Session controller or OS process, never a substitute for authenticated user/principal.
- **Controller**: authorized human/parent/runtime issuer of a particular control. A child may notify its parent; it cannot steer or widen its parent.
- **Envelope**: static capability ceiling + runtime fences; neither a prompt instruction nor a sandbox claim alone.
- **Journal receipt**: factual durable record; UI events and hook callbacks are projections, never receipts.

### 1.1 Ten stages (no synonyms)

`turn` (user cycle start/end/steer), `step` (model cycle start/end/model_select/compact), `input`, `prompt`, `message`, `output`, `tool` (call/result), `signal`, `event`, `lifecycle`. No separate `route`, `decision`, `tool_approval`, `agent_spawn` or `status` stage; those are typed handlers/service intents at existing stages. Legacy alias hooks must not be maintained. Internal code may use separate Rust methods but they project into these stage kinds.

### 1.2 One minimal core; adapters carry complexity

~~~text
Users / CLI / TUI / SolidJS / RPC / SDK
                  |
             typed adapters
                  |
Hosted Runtime (actor registry, mailbox, continuations, control/admission)
     |             |                         |
   Actor        Message                  Envelope
     +--------------+-------------------------+
                  |
  ONE Agent + ONE TurnMachine + ONE ToolPolicy + ONE Journal path
                  |
Services: process supervisor, queue, hook chain, tools, approval, decision,
          routing, child Sessions, prompts, settings, UI projections, media
                  |
Platform and store adapters: Tokio/OS/PTY/MCP; SQLite-v33/JSONL/memory;
                     no_std embedded host ports
~~~

The hosted Runtime orchestrates the existing Agent; it must not become another "Agent orchestrator" with separate model continuation, tool effect order or retry policy. Actor kinds share lifecycle and control semantics; only the kind-specific I/O implementation differs. A process actor is not entitled to tool grants by its existence.

## 2. Kernel/host separation

### 2.1 Portable `aw-agent-kernel` (no_std + alloc)

Owns the small **pure** representation and transition rules: actor IDs/generations, actor kind/state/lineage checks, queue target ordering, message class/kind validation, envelope intersection math and exact source-capability admission, terminal/nonterminal state transitions, continuation-token identity, Turn/Step boundary predicates, and existing `TurnMachine`.

No Tokio, threads, socket/HTTP/TLS, OS process, database, credential store, filesystem, wall clock, UI, direct allocator/heap policy, host callback or background scheduler. Existing `Source/Capability/Grant` and `TurnMachine` remain the *same* model/tool/effect authority. Use the existing alloc footprint/caps: extend only when a pure state transition actually requires it.

### 2.2 Hosted `aw-agent-core` and collaborators (std)

One Runtime owner per authorized execution binding (owner/principal/store/site) holds an actor directory and bounded pending control/continuation state. Tokio schedules I/O; **admission is serialized per Actor and fenced to the Session writer**. Existing `Agent` and its `TurnJournal` remain authoritative for model/tool execution.

A Runtime may schedule several actors, but it may not own an independent transcript persistence mechanism. The existing store's writer/CAS/epoch mediates durable messages. On process or extension disconnect, pending effects are labelled Unknown when factual completion is not established.

### 2.3 Embedded E1–E5 host

No_std hosts advance the **same pure transitions** through an injected port (`poll/wake/time` callbacks supplied by the host). No POSIX syscall is a kernel requirement. E3 zero-import WASM, E4 libc-free Linux ELF and E5 C ABI keep their current demonstrated semantics; new runtime capabilities unavailable in an embedded host must return explicit `Unsupported`/`Unavailable`. E6 firmware/board power-loss recovery remains parked. Never force std-based actor/process/voice features into embedded builds just to make the type names match.

## 3. Primitive: Runtime

**Responsibilities:** allocate/fence actor identities; accept validated messages into bounded inboxes; select fair admission at boundaries; register and resume continuations; propagate cancellations; expose liveness; orchestrate graceful shutdown and terminal escalation; project runtime snapshots for UI/RPC; connect to current Journal and policy services.

**Non-responsibilities:** issuing grants, inventing principals, selecting a payer/account by default, deciding to replay an uncertain effect, replacing the Agent's TurnMachine, storing secrets, becoming a daemon at ordinary startup.

### 3.1 Illustrative Rust contract (DESIGN ONLY)

~~~rust
// Pure shape (names subject to implementation review; not compiled API).
pub struct RuntimeKey { pub id: NonZeroU64, pub generation: NonZeroU64 }
pub struct ActorKey { pub runtime: RuntimeKey, pub id: NonZeroU64, pub generation: NonZeroU64 }
pub struct ContinuationKey { pub actor: ActorKey, pub id: NonZeroU64, pub generation: NonZeroU64 }

pub enum RuntimeStep {
    Idle,
    Progress { actor: ActorKey },
    NeedsHostPoll,
    Rejected { error: ControlError },
}

// Hosted adapter. Deterministic admission; concurrent I/O is an implementation detail.
pub trait RuntimeHost {
    type Error;
    fn step(&mut self, budget: StepBudget) -> Result<RuntimeStep, Self::Error>;
    fn send(&mut self, message: ActorMessage) -> Result<AdmissionReceipt, Self::Error>;
    fn snapshot(&self, principal: &PrincipalId) -> Result<RuntimeSnapshot, Self::Error>;
    fn shutdown(&mut self, reason: ShutdownReason) -> Result<ShutdownReceipt, Self::Error>;
}
~~~

A callback/pump may wake Tokio tasks or an embedded host; it is **not a second async executor**. Continuations have a single owner, cancellation token, timeout, source epoch and finite lifetime. A stale/dead continuation is rejected, not revived.

### 3.2 Scheduling and bounds

- Admission/dispatch ordering deterministic at safe points, even with Tokio concurrency. Use a bounded per-actor mailbox, explicit priority/sequence, fixed drain budget and fairness between active actors.
- Negative priorities request urgent handling; they are **not authority**. Work already entered into an external-effect phase cannot be unsafely interrupted and replayed.
- Startup of bare `aw`, `aw --help`, a source inspection or a memory-only conversation must not initialize long-lived process actors, network listeners, project/MCP scans, credentials, media or background model calls.
- Lifecycle/approval/control termination never silently drops on a full buffer. Reject producers or reserve critical capacity; low-value progress reports may coalesce with a visible dropped count.
- Shut down with a bounded grace→force sequence per process, then report any surviving/unknown external action. No "terminated" acknowledgement absent confirmation.

## 4. Primitive: Actor

Two kinds: `Agent` (the existing Agent/Session) and `Process` (owned OS child). A terminal is `Process { tty: true }`; an MCP stdio server and a voice helper are likewise Process actors **only when explicitly authorized/started**, and their protocols are separate services.

~~~rust
// Pure tags; hosted routes are separately checked.
pub enum ActorKind { Agent, Process }
pub enum ActorState {
    Starting, Running, Waiting, Stalled, Frozen, CancelRequested,
    Settled, Failed, Cancelled, Unknown,
}
pub struct ActorDescriptor {
    pub key: ActorKey,
    pub kind: ActorKind,
    pub parent: Option<ActorKey>,
    pub depth: u8,
    pub state: ActorState,
    pub envelope_revision: NonZeroU64,
}

// std-only authenticated projection; never inferred from an untrusted ID.
pub struct ActorBinding {
    pub key: ActorKey,
    pub owner_principal: PrincipalId,
    pub owner_session: Option<SessionId>,
    pub execution_site: ExecutionSite,
    pub writer_epoch: Option<WriterEpoch>,
}
~~~

Do not add a separate `SubAgent` actor kind. A child has its own Session and invokes the same Agent type with its own history, receipts and output. A child inherits the **currently pinned** parent provider/model/thinking/transport when permitted, unless a host-authored route with separately authorized account/destination supplies another. "Direct inheritance" must not override provider custody or payer choice; reference route revision, source and account decisions are recorded. Child history is isolated by default; bounded context import is an explicit source/destination disclosure operation (not a raw parent transcript share).

Agent and process actors share query/list, labeled state, cancel/freeze/resume semantics **where supported**. Kind-specific implementation remains separate. Parent shutdown cancels descendants and only releases identities once terminal evidence or an Unknown state is recorded.

**Identity:** ID reuse is allowed only after incrementing generation. `ActorRef` includes runtime instance and generation; authenticated owner/Session/site checks occur for every operation. A raw PID, integer slot, display label, client id, source-provided `actor_id` or imported transcript cannot address an Actor by itself. Attach/detach never changes owner.

## 5. Primitive: Message

An ActorMessage represents *communication*, not a chat transcript line. One core data path with four classes:

- `Control` — validated instructions like Cancel, Freeze, Resume, ProcessWrite/Resize, SetModel/Thinking, Settle, ApproveResponse, Attach/Detach. Privileged control remains a typed host service and requires an exact issuer/target check.
- `Input` — queued work/steering carrying a Turn target, signed priority and source sequence. It forms the only queue.
- `Notify` — child/process to controller progress/status/question/result hints, with typed producer/lineage provenance; never parent steering.
- `Hook` — typed stage event/transform carriage, with a bounded recursion depth and request ID.

~~~rust
pub enum MessageClass { Control, Input, Notify, Hook }
pub enum MessageBody {
    Text(alloc::string::String),
    Structured(alloc::vec::Vec<u8>),
    OpaqueMedia { content_type: ContentType, bytes: alloc::vec::Vec<u8> },
}
pub struct ActorMessage {
    pub message_id: MessageId,
    pub source: ActorKey,
    pub destination: ActorKey,
    pub class: MessageClass,
    pub kind: MessageKind,
    pub target_turn: TurnTarget,
    pub priority: Priority,
    pub source_sequence: u64,
    pub correlation: Option<CorrelationId>,
    pub expected_generation: NonZeroU64,
    pub body: MessageBody,
}
~~~

These are sketches, not license to attach raw JSON to trusted internal commands. Runtime envelopes should be typed in Rust with `serde(deny_unknown_fields)` at untrusted RPC boundaries and borrowed/owned views as appropriate. The transport `content_type` is opaque to the core, but it is bounded and never interprets bytes as permission.

### 5.1 Queue delivery (derived service)

Use one per-Agent `Input` queue: default `turn=Auto, priority=0`. Optional explicit future Turn. Proposed signed range `-20..19`, consistent with P6, but out-of-range input **rejects** rather than clamps without notice. The deterministic order is target Turn, phase, priority, sequence and arrival index.

`priority < 0` requests urgent preempt **only when the current model phase is safely interruptible**. `priority >= 0` lands at the next safe model/Tool batch boundary. A priority is not proof of user authority. Persistent pending input cannot bypass Session lock or current context-source authorization.

**Deliberate P6 deviation:** an explicit stale absolute Turn fails with `TurnSettled` rather than silently being moved into the next Turn. `Auto` is the opt-in ergonomic "current if safe, otherwise next"; recorded `client_id`/idempotency prevents accidental duplicates. A pending item for a non-steerable phase (compaction/review/intent/effect/receipt/uncertain) is deferred with a reason or refused; it must not discard a paid result or running effect.

**Cancel:** `Scope::Turn` cancels a user cycle, preserves Actor and pending queue; `Scope::Actor` cancels it and owns descendant cleanup; `Scope::Subtree` and `Scope::Runtime` are controller-only. Never alias "turn cancel" to arbitrary "step cancel". If a precise Step-only stop is ever needed, explicitly define a new typed operation without changing the existing meaning.

### 5.2 Mailbox reliability

Enqueue acknowledgement = **accepted for processing**, not "action executed." Execution acknowledgement requires a separate, factual result/receipt. Delivery class/phase/priority/producer sequence are logged or persisted only under an owning writer. Duplicate or out-of-order RPC commands fail or dedupe against an exact idempotency key as specified; no regenerated tool/model attempt. Noncritical progress may be coalesced, **never** terminal/approval outcome events. No deadlocks: a waiting parent/approver must continue servicing the logical runtime control path.

## 6. Primitive: Envelope

A per-Actor effective ceiling. It is not merely RO/RW or a sandbox label. It includes:

1. Exact host-issued ToolGrant set (source kind/id/epoch, effect, actor, site); explicit ToolPolicy defaults to OFF. A model's advertised tool name or `readOnlyHint` is descriptive, not a grant.
2. Coarse access (read-only/read-write), rooted filesystem policy, network destinations and subprocess/PTY/background/media flags, all subject to host capabilities.
3. Principal, Session, parent/root lineage, execution site, expected store/writer epoch, provider/account/payer/custody/credential delegation and routing revision.
4. Budgets (model/tool call counts, token/cost ceilings, actor count/depth, memory/output bytes, CPU/wall time, queue depth, approval expiry).
5. Trust revision, revocation time and pending approval policy. A restarted Session can reconstruct **data**, not credentials, capabilities or permanent approval from a transcript.

~~~rust
pub struct Envelope {
    pub revision: NonZeroU64,
    pub grants: ExactGrantSet,
    pub access: AccessMode,
    pub process: ProcessPermissions,
    pub network: NetworkPolicy,
    pub roots: RootBindings,
    pub budget: RuntimeBudget,
    pub execution: ExecutionBinding,
    pub approval: ApprovalPolicy,
}
impl Envelope {
    pub fn intersect(&self, requested: &EnvelopePatch) -> Result<Envelope, Denial>;
    pub fn authorize(&self, subject: &AuthorizedAction) -> Result<AuthorizedIntent, Denial>;
}
~~~

Actual types may be split into pure kernel descriptors and hosted authenticated bindings. **Only the trusted host constructs the full effective Envelope.** Child Envelope = parent ∩ requested constraints ∩ current policy, never union. An owner wanting more authority performs a **new explicitly authorized root transaction with a fresh revision**, not a silently widened child envelope. Tools and model destinations are rechecked **after awaits and immediately before effects/disclosure**. Revocation creates a new epoch; stale continuations and tool results cannot cross it.

Sandbox backend is a separate enforcement adapter: `Root` protects in-process file paths, while ordinary same-UID Bash/process extensions are *not* a sandbox. If a user requests kernel confinement and that backend is unavailable, fail closed with `SandboxUnavailable`. Do not advertise confinement on Windows/macOS/Linux unless independently tested.

## 7. Hooks: one pipeline, two verbs

The pipeline is a **service** over Message + Runtime continuation, not a primitive. Normative contract in [Hooks, trust and policy](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md). Schematic:

~~~rust
// Exactly two primitive hook actions; domain types constrain each stage.
pub enum HookFlow<M> {
    Next(Option<M>),           // same/edit/replacement or consume (None)
    Defer(ContinuationKey),   // resume exactly once from the Runtime
}
pub type HookOutcome<M> = Result<HookFlow<M>, HookError>;
~~~

A `ToolCallGate` returning `None` must still produce a properly correlated denied ToolResult and audit record; it cannot leave a provider call unanswered. A hook may send messages or spawn an authorized Actor **only through the same validated Runtime/Envelope API**. Event and lifecycle handlers are observational; they never mutate durable evidence. Mandatory authorization/journaling is outside optional hook disable switches. A hook cannot replace a paid invocation receipt or a private provider continuation with generated text.

Stages in deterministic order within each Stage: ascending order, then registration sequence; snapshot handler list at entry; reentry/recursion limit and per-handler budget. All hook callbacks have an owning trust class (trusted in-process Rust, explicitly selected process extension, UI projection only) and an appropriately restricted context.

## 8. Derived services — one mapping

| Service / feature | Composition, canonical owner and boundary |
|---|---|
| Input queue/steering | Actor inbox + `Input` messages; Runtime safe-point admission; host journal when durable |
| Cancellation, freezing, resuming | `Control` messages + Actor lifecycle + Runtime time; platform signals behind host ports |
| Process supervision | `Process` Actor + Envelope + Runtime I/O/clock + typed output and exit receipts |
| PTY/ConPTY | Process Actor with `tty=true`, existing `aw-terminal` I/O driver |
| Agent tree | `Agent` Actors whose work is existing child Sessions, no new Agent engine |
| MCP | Tool runner/service; server process is a Process Actor only for opted-in stdio, HTTP MCP stays network adapter |
| Process extension | Explicitly selected Process Actor, strict versioned wire and source-minted grants |
| Tools | Existing named capabilities through one Tool admission/execute/result stage, not three universal management-only names |
| Hook pipeline | Ordered transforms over ActorMessage and Runtime continuations |
| Decider / router / guardian | `input`/`tool`/`step` hook handler with optional bounded model/program/MCP backend; typed proposal validated by host |
| Approval | Envelope gate + typed request/reply Messages, durable audit and scoped expiry; never a direct grant by model |
| Journal | Mandatory durable service around paid dispatch/external effects; `TurnMachine` remains source of effect order |
| Provider catalog / accounts | Existing provider adapters and authenticated account selection; not a primitive |
| Settings | Typed namespace per extension over config + trust-gated scope; credentials stored separately |
| Prompt contribution | Typed, source-approved System/Tools/Rules/Environment context renderer + prompt stage |
| Status/notifications | Read-only UI projection service and transcript `notice` at completed boundaries |
| Voice/media | Opaque content-type on Message + opt-in Process helper + explicit mic/speaker consent |
| CLI / RPC / SDK / browser | Typed adapters to the same Runtime/Actor/Message/Envelope; no second policy |
| Embedded C ABI | Versioned host adapter over same kernel state machine; no std dependency |

P6's three op-based management tools (`process`, `agent`, `queue`) may be selectively implemented as **capabilities**, but they do **not** replace `read`, `write`, `edit`, `ls`, `find`, `grep` or `bash`; all tools still pass the same `tool` gate. Model-facing names do not define authority. CLI `aw agent` remains the AwNode/Machine service; do not overload that noun with "child Agent".

## 9. End-to-end canonical flows

### 9.1 Normal user Turn

1. Authenticated user/owner sends an `Input` message addressed to an Agent Actor.
2. Runtime validates Session ownership, actor generation, writer epoch, bounded queue and Envelope; persist admission when a durable backend is selected.
3. `input` hook runs on approved data, with proper timeout and provenance. `turn/start` and `step/start` event projections are emitted. Context is re-authorized and composed by the current approved source graph.
4. Existing Agent/`TurnMachine` emits `ModelStarted` and obtains the durable invocation reservation/receipt via `TurnJournal` before paid dispatch. Provider adapter streams typed content without exposing private signed reasoning.
5. Complete model proposal validated. Every model-facing tool goes through one `tool/call` gate, exact grant and argument validation, per-call intent durable append **before effect**, and same grant recheck after awaiting journal append.
6. Tool runner performs effect under the authorized execution site. Result/unknown is recorded in original call order before `tool/result` presentation hook and subsequent provider disclosure. Tool call IDs remain paired. Mutations serialize.
7. Next model step begins only when required receipts are settled; `step/end`, `turn/end` and `lifecycle` projections fire. Source history retains immutable evidence.

### 9.2 Background subprocess

Host explicitly grants `process.spawn` or `bash`; a model proposal alone does not start it. Process Actor is registered with owner, grant and deadline; spawn is intent-journaled before OS entry, then actual pid/group/PTY handle is stored as ephemeral resource with a stable generation-fenced ActorRef. `process.read` drains bounded output; `write` is separately authorized and records effect intent. Process exit has factual code/EOF; timeout/cancel without reliable termination remains Unknown. Background lifetime never exceeds owning Session/runtime without an expressly designed durable ownership transfer.

### 9.3 Child Agent

`agent.spawn` requires explicit parent grant, root budget reservation, child Session/worktree allocation and target revision. Envelope is narrowed and model/payer account pinned. Child uses the **same Agent** and journal through its own Session. Parent may wait/send/cancel; child notifies upward but cannot steer parent's Turn. Completion is a normal authorized, source-labeled tool result; child transcript remains isolated and a reviewer proposal does not publish by itself.

### 9.4 Approval/decider

At an authorized `tool/call` gate, static policy may ask for human approval or an optional model/program reviewer. Reviewer runs a separately budgeted and journaled inference/process action; returns strict verdict *data*. Host verifies actor/source/epoch/args/budget/expiry again. Approval is a scoped host receipt, not a capability minted by reviewer JSON. On timeout/malformed/missing reviewer: deny or require human, never auto-allow.

### 9.5 Steering and unknown effects

During a model stream, a high-priority steer can request cancel; any prior paid attempt remains recorded as paid/unknown until factual evidence resolves it. During an external tool effect, urgent steering is enqueued but cannot erase the effect or skip its receipt. If the tool's termination is uncertain, further effected work blocks for inspection. An old absolute target Turn is rejected; no silent "move to next Turn".

### 9.6 Settings/voice/UI

Typed settings are keyed by their extension owner, with project scope only on approved resource source. UI displays read-only Actor status and status lines, never grants execution authority through rendering. Voice uses `ContentType` and a Process helper but is not initialized or given mic/speaker/remote API access by default. Audio/transcription storage is opt-in and privacy-gated.

## 10. Failure and recovery matrix

| Boundary/failure | Required behavior |
|---|---|
| Invalid/stale ActorKey or wrong owner/site | Deny without process/model effect |
| Full queue | Reject new input with typed error; never lose accepted terminal/approval receipts |
| Hook timeout at an authority gate | Deny, journal outcome; no effect |
| Optional observe hook crashes | Bounded failure projection; factual execution unchanged |
| Provider stream lost after dispatch | Preserve paid/unknown attempt; no automatic retry from visuals |
| Tool effect uncertain after cancellation | Mark Unknown/NeedsInspection; never assume rollback or replay |
| Process group not confirmed dead | Keep Unknown and recorded attempt; avoid reuse of actor identity |
| Child Session route/account revoked during await | Refuse dispatch/disclosure; no implicit fallback |
| Journal append fails before effect | No effect starts |
| Receipt append fails after effect | Block next effect/model, preserve hazard and inspection requirement |
| Actor frozen while effect active | Quiesce future admissions; do not claim active external effect frozen without OS proof |
| Worker/runtime restart with pending controls | Replay persisted *facts*, revalidate or reject pending items, never replay paid/effectful actions |
| Unsupported sandbox, PTY, voice, process in a host cut | Explicit Unavailable/Unsupported, not silent unsafe fallback |
| Bad external MIME/body/JSON | Bounded parsing/denial, never interpretation as authoritative instruction |

## 11. Source modules and intended ownership

**Current source** (do not claim yet refactored): `crates/aw-agent-kernel/src/capability.rs` and `turn.rs`; `crates/aw-agent-core/src/agent.rs`, `tool.rs`, `extension.rs`, `journal.rs`, `planner.rs`, `reviewer.rs`; `crates/aw-tools/src/shell.rs`, `search.rs`; `crates/aw-terminal/src/lib.rs`/`driver.rs`; `crates/aw-platform-unix` and `aw-platform-win`; `crates/aw-extensions/src/lib.rs`; `crates/aw-ui-extension/src/lib.rs`; `crates/aw-store/src/journal.rs` and immutable migration records; `crates/aw-store-jsonl/src/lib.rs`; `src/main_full.rs`, `main_minimal.rs`, `cli_taxonomy.rs` and `src/tui`.

**Proposed additions, contingent on implementation review:**
- `aw-agent-kernel/src/actor.rs`, `message.rs`, `envelope.rs`, `hook_contract.rs`: **pure** types/machine checks, not OS access.
- `aw-agent-core/src/runtime.rs`, `actor_host.rs`, `hook_pipeline.rs` and optionally `control.rs`: one hosted orchestration/service facade delegating to the existing Agent.
- Process resource ownership in `aw-tools`/`aw-terminal` with platform-specific ports; no new separate subprocess executor crate unless the current owners cannot express the contract.
- Existing `aw-store-api`/SQLite v33/JSONL/memory-only as the persistence adapters; add no migration until a concrete unrepresentable durable fact justifies separate user approval.
- Typed public adapter layers in current CLI/RPC/SDK and `aw-ui-extension`; preserve one renderer/editor reducer.

The numbered wave plan sets exact change ownership and a dependency graph; do not use these filenames as a mandate to create duplicate wrappers.

## 12. Design acceptance and invariant registry

| ID | Mandatory invariant |
|---|---|
| FP-01 | Only Runtime, Actor, Message, Envelope are primitives; all additional capabilities are services/adapters. |
| FP-02 | Hook algebra exactly Next or Defer; no per-feature disposition engine. |
| FP-03 | Agent, child Agent, and portable E1–E5 share the same TurnMachine/ToolPolicy/journal semantics. |
| FP-04 | Runtime actor key is generation-fenced, site/owner-bound; raw ID/PID does not authorize. |
| FP-05 | Exactly two Actor kinds; terminal/voice/MCP processes are flavors/resources. |
| FP-06 | Every tool (host/MCP/extension/management) uses current exact host grants; no name-only legacy exception. |
| FP-07 | Root/child Envelope narrowing is monotonic; reviewer/router/text cannot widen grants. |
| FP-08 | Mandatory security/paid/effect journal gates are not optional hooks. |
| FP-09 | Unknown paid/model/tool/process outcomes are never silently replayed, cancelled-success or billed-zero. |
| FP-10 | Provider-signed private thought never leaks into public hooks, cross-provider history or logs. |
| FP-11 | One typed queue with deterministic admission, fairness, bound and safe-point steering, not unsafe forced replay. |
| FP-12 | One owning writer/CAS/epoch for durable Session; SQLite v33 remains current until separately authorized. |
| FP-13 | Process same-UID access is not claimed to be an OS sandbox; required confinement fails closed. |
| FP-14 | CLI/SDK/RPC/TUI/browser are projections of the same Agent and authority; no separate web Agent. |
| FP-15 | No automatic background side effects on bare invocation, --help, listing, or read-only actions. |
| FP-16 | Full/JSONL-minimal/stateless + no_std embedded supported by feature-aware honest Unavailable responses. |
| FP-17 | No legacy command/tool/hook/migration/compatibility aliases, no external binary comparison. |
| FP-18 | E6, Phases64/65 and Phase69 stay excluded; R13 native certification/release is separate. |

### 12.1 Explicit design differences from P6 that are improvements, not competing principles

P6 allows RW/auto inside the workspace by default; aw rejects implicit grants. P6's stale absolute Turn may roll forward; aw rejects it unless caller explicitly used Auto. P6's broad hook mutation is typed and postvalidated; mandatory policy/evidence is non-bypassable. P6's process-local integer actor slots get generation fencing and authenticated owner/Session/site. P6's occasional "step cancel" naming is resolved to `Scope::Turn` for a user cycle. P6's three-management-tool proposal becomes three **optional management capabilities**, while aw retains ergonomic explicit tools. P6's "return a model/program verdict then perform effects" becomes a host-validated proposal with receipts and budgets. P6's process restart/checkpoint ability remains optional and not claimed.

None of those differences requires a fifth primitive, a second scheduler, or a new Agent loop.

## 13. Navigation and handoff

- [Runtime contract](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md) — dependencies, decision mapping R-D1…31, source-specific waves, gates, cleanup and starting prompt.
- [Hooks, envelopes, approval, deciders](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md) — authoritative typed Rust contract and security behavior.
- [current plans](../plans/TODO.md); [START_HERE](../plans/START_HERE.md); [Architecture](ARCHITECTURE.md); [Tools](TOOLS.md); [SDK](SDK.md); [CLI](CLI.md); [State](STATE.md); [Validation gate](../plans/TODO-SPEC.md).
- Historical archived designs and the exhaustive handoff remain immutable; this document records the newly **chosen target** and must be updated with an **as-built addendum** for every implemented wave.

---

## As-built hosted Process service note — AW-P6-4A (2026-10-11)

The target design above remains normative. In the local Rust source, `aw-agent-kernel` supplies `ActorKind::{Agent,Process}` plus generation-fenced identities and narrow-only envelopes, while `aw-agent-core/src/runtime.rs` now binds a Process actor to a real Agent parent *metadata* Session under exact Process grants, parent child budget, writer epoch and lineage. The OS implementation lives in `aw-tools/src/process.rs`: an explicitly host-launched supervised pipe resource retains a bounded prefix and tail, drains both streams while not polled, has cancel/deadline/explicit stdin and conservatively incomplete output receipts. The trusted `bash` model tool reuses this host service after the existing Agent has authorized/journaled its Process call. Optional `aw-tools/pty` delegates entirely to the existing `aw-terminal::Terminal` driver; minimal CLI does not gain this dependency. Linux group termination is TERM→bounded grace→KILL; other OS native semantics remain pending actual tests. This is deliberately not a sandbox, daemon, additional tool execution engine, model-facing process-management API, durable process resurrection, PTY rewrite, or E6 firmware functionality. See the AW-P6-4A as-built appendix and acceptance receipts in the companion master plan.

---

## As-built AW-P6-5A: one hosted mailbox→original Agent control service (2026-10-11)

The portable mailbox now orders and bounds `ActorMessage` without new primitives: Input/Control/Notify use one per-Actor sequence ledger, signed priorities, fair original-order ties and bounded urgent streak. Parent/owner-authorized Turn cancellation is a **synchronous `Control` admission with its own reserved capacity**, not an independent signal scheduler or an implied effect receipt; a full Input queue cannot block it. Normal Input is consumed through one *tracked* Turn claim, not a raw dequeue. Pending edit/delete and stale sender generation/`TurnTarget::Exact` rejection are explicit. Cooperative `Freeze`/`Resume` of the Agent stops only at controlled model/tool boundaries and does not pretend to OS-freeze a Process Actor.

`aw-agent-core/src/control.rs::AgentSessionController` uses `HostRuntime`, a Tokio wake service and one `Agent::prompt_with_step_gate` adapter on the same journaled `prompt_as_inner` loop. It never creates another model/tool executor or serializes a provider-private continuation. Urgent Input requests interruption *after* a completed provider reply but *before* the first tool intent, or before the next provider request; it remains a separate future Turn rather than unjournaled intra-turn text injection. Explicit active Turn cancel uses the existing Agent cancellation future, with original provider/tool factual receipts and `Unknown` reconciliation if needed. RAII guard on dropped execution does not replay or mark a paid call zero. No hidden background executor, OS process, RPC endpoint or model-facing queue tool activates by importing this module.

`Envelope` remains only the authenticated outer ceiling; `ToolPolicy` still grants the exact source/effect. Hosted tool advertisement, batch admission, just-before-effect check and post-effect disclosure **all** use the shared exact Envelope predicate (including on source revival during awaited provider calls), never name-only or a source-declared read-only hint. Pending control acknowledgement is ephemeral and cannot be presented as durable accepted Session data until AW-P6-9 writer/CAS integration is separately complete. Native evidence and remaining work are recorded in the new AW-P6-5A appendix of the master plan.

---

## As-built AW-P6-6A — a single typed hook service over the four primitives (2026-10-11)

The Rust extension API now has one `on_hook` method and the portable algebra `HookFlow::Next|Defer`, not many `before_*`/`after_*` methods. A stage-typed, bounded `HookEvent` is a **service projection**, not a fifth runtime primitive. `aw-agent-core/src/hooks.rs` is the sole hosted hook dispatch module for Actor Message and Agent/auxiliary event paths; its owner-specific sync/async registries and continuation adapters remain to be unified in AW-P6-6B. Agent, summary, planner, guardian and router use the existing Agent/TurnMachine/provider/journal and only current host-authorized tools. Factual ToolResult/paid-attempt receipts remain host owned and immutable; extension output may at most redact the bounded model-visible projection. Only bounded observation labels reach read-only lifecycle observers, whose failures are tracked in a counter rather than printing in TUI.

The explicitly selected external process extension protocol is **v2** (`prompt`, `tool.call`, `tool.result`), rejecting v1 without legacy support. Actual independently compiled Rust/C process fixture, full/minimal CLI, Read/Bash waves and E3/E4/E5 embedded targets were executed; details and honest AW-P6-6B limitations are in the newest appendix of the AW-P6 master plan. The four primitives and the two hook verbs remain the complete foundation; approval, renderer, process, decision and journal paths are services and mandatory effect gates, not competing execution cores.

---

## As-built AW-P6-6B — deterministic no-effect denial and one-shot hosted Defer (2026-10-11)

The shared `no_std + alloc` kernel `TurnMachine` gained an *explicit no-effect batch denial* transition (`BatchRejected` → ordered `DenialRecorded`) rather than adding another tool execution engine. Each provider-proposed ID consumes the normal bounded tool-call budget, needs a host journal acknowledgement, and receives an original-ID model-visible denial. Unknown/Write/Process effects are never inferred from an admitted denial. The existing SQLite/JSONL factual store rules verify original proposals and separate revisions/started effects, with no new schema or replay path.

Optional approved argument changes are explicitly hosted policy decisions (`ToolRevisionPolicy`), committed as immutable `ToolRevised` evidence before actual tool intent with exact current source/epoch/effect, and reauthorized at every awaited effect boundary. Async typed hooks can now Defer *only* with the current owned Actor/Session `HostRuntime` reservation; no fifth primitive or extra verb. The cross-owner registry/lifecycle and normal persistent CLI/RPC/Session adapter remain future services. All actual native and embedded execution tests and remaining gates are detailed in the newest appendix of `../plans/AW-P6-FOUR-PRIMITIVES-MASTER-PLAN-2026-10-11.md`.

---

## As-built AW-P6-7A — existing child Session as opt-in bounded Actor (2026-10-11)

`src/child_actors.rs` is a host-only service mapping a previously admitted **SQLite-owned** `subagent_id` + `child_session_id` to a generation-fenced Agent Actor with a narrowly inherited parent Envelope. Its ActorKey is volatile identity, NOT a provider credential, new Session writer, process ID or a durable effect result. `src/subagents.rs::ServiceConfig::with_child_actors` is deliberately opt-in for existing **model-only** child execution; ordinary child Sessions and managed Git worktree subagents stay on their unchanged original path. `AgentSessionController::run_once_selected_with_cancel` invokes the SAME Agent/TurnJournal/model selection, using the exact stored root/child/parent allocation and task hash, and preserves original root-only publication.

A host may query durable child `status/list/wait`, cooperatively signal only an *active* child Turn (`Cancel`, `Freeze`, `Resume`), read a bounded status-only notice from its exact parent Actor, and explicitly release a terminal child only once its direct parent consumes the final notice. No private transcript, unreviewed child merge, remote site grant or process effect is inferred from a notice or a control acknowledgment. An Unknown Actor cannot be released. Source/site/epoch/budget caps and exact current ToolPolicy remain mandatory even when an extension wants to transform the event. The optional bridge immediately refuses managed-worktree children with `execution_site=subagent:<id>` until a separate site-bound Runtime can authenticate them; it does not lie about site or skip Git safety.

The source-of-truth as-built evidence and remaining AW-P6-7B/9 gates are in the master AW-P6 plan. This service introduces no fifth primitive and no second child Agent loop or automatic restart replay. A separate agent exclusively owns `.agents/plans/ux/`; this phase must not stage, stash, reset or commit that directory, and must serialize its own path-only Git checkpoint atomically with HEAD compare-and-swap.

---

## As-built AW-P6-7B: a new child Actor Envelope at a verified local worktree site (2026-10-11)

An Agent Actor's `execution_site` is the site in its own validated `Envelope`, **not** necessarily the site on its root Runtime. The explicitly bound HostRuntime remains one owner/ActorDirectory with one parent chain, a bounded mailbox per Actor, and one real Agent/TurnJournal per Session. A same-site descendant uses `Envelope::narrow` from its **immediate parent**, including an already-transferred parent's site. A cross-site child is a **new** Actor binding through typed `SiteChildRequest` and host-minted `SiteTransferBinding` after the original SQLite managed worktree is *already* in `bound` state and its child Session tool root/working directory and parent repo/lineage have been revalidated canonically. Only exact source kind/id/epoch/effect grants present in its immediate parent may be reissued at the new site, under nonwidening process, network, access and budget ceilings. The parent Actor is never retargeted, and no proxy Runtime or second model loop is created.

`src/child_actors.rs` now supports opted-in managed local Git worktree sites `subagent:<id>` for ReadOnly and Workspace, while `subagents::Service` preflights the host-issued root tool grants **before** git/FS side effects. The same protected tools, journal and native worktree resolver handle all actual effects. `AgentSessionController` checks the child's Envelope site instead of demanding every child use its ancestor Runtime root site. Parent cancellation and child `Notify` are still ordinary typed Actor Messages in one Runtime inbox. This does NOT authenticate a different physical device or confer an OS sandbox; provider/private Session data remains source-authenticated and no unknown effect is retried.

`src/child_actors.rs::authorize_decision_step` adds a narrow opt-in read-only parent-Actor/Session/branch/site/cancellation gate for `DecisionKind::{Routing,Planner,Guardian,Summary}` before the existing paid classifier/guard source journals; planner/guardian/team wrappers reuse that gate when the same `subagents::Service` carries Actor authority. Existing SQLite decision evidence, route candidates, root budget and human approval remain the sole durable decision artifacts. See the latest AW-P6-7B master as-built appendix for actual cross-site/recursive/negative/native/embedded checks and explicit AW-P6-8/9 limitations.

---

## As-built AW-P6-9A — SQLite writer-fenced Actor Input admission, never a replayable receipt (2026-10-11)

The public durable-feature `aw::actor_owner::Service` composes exactly one existing Agent, `AgentSessionController`, ActorKey/generation/Envelope, and the existing SQLite v33 SessionStore. It reuses SQLite's active Workspace writer-epoch predicate, branch head/revision/next-Turn CAS, immutable `audit` and `security_audit_events` instead of adding a second queue table or model journal. Typed `ActorInputAdmission` / `ActorInputFact` events record source/target Actor generation, original request ID, bounded priority and a SHA256 of the accepted Input **without storing prompt text in the audit**. Stages Prepared, Accepted, Dispatching, Settled, Rejected, Unknown and Inspected distinguish durable admission facts from factual provider/tool completion. Settled is accepted only after a real terminal Session run with no unresolved hazard; uninspected in-flight facts block later automatic admissions, even across SQLite store restart.

The Host SDK verifies the exact claimed Actor message against its Accepted fact **before** forwarding it to `Agent::prompt_as_inner`, then revalidates live writer epoch on every provider/tool safe point and after the paid result. Dropped runners, stale writers, altered pending payload or failed acceptance acknowledgements poison the owner/claim and never silently reenqueue or retry a paid operation. This first slice deliberately admits **one** outstanding Input per Session and persists no replayable payload: it is an honest admission ledger, NOT a general durable queue. Its code only exists with the `durable` Rust feature; minimal JSONL, stateless, embedded and public CLI/RPC remain unchanged. Native proof, exact source paths and remaining AW-P6-9B gates are in the newest master-plan appendix. The other agent exclusively owns `.agents/plans/ux/` and all its staged work.

---

## As-built AW-P6-9B1 — typed Control admission facts, one Runtime control path (2026-10-11)

The public durable-feature Rust `aw::actor_owner::Service` now exposes typed one-owner `ActorOperation` / `ActorOperationResult` for `QueryInput`, `QueryControl`, `ListControls`, `Control` and explicit inspection of stranded Input/Control facts. SQLite v33's **existing immutable `audit`** records `actor.control` with bounded source/target generation, current `writer_epoch`, exact active Turn, original `actor.input` correlation, monotonic shared message sequence, action, owner process incarnation and outcome: `Prepared → Applied|Rejected|Unknown → Inspected` (Unknown/Prepared only). The same owner must have an actual current `runs.status=running`, the exact already-Dispatching Actor Input, the real local Workspace writer and the same process generation. The Store never persists replayable Control text/process commands; an audit ACK is not a provider/tool/OS receipt. Full negative tests include actual disk owner restart, duplicate/out-of-order control IDs, wrong source/target/turn, forged scopes, writer fencing, and actual in-flight Cancel/Freeze/Resume via the existing Agent/TurnMachine and ControlToken.

Ordinary JSONL-minimal, stateless, no_std and the existing `aw rpc --agent-stdio` do **not** gain privileged control access. `actor_op`/`actor_query` remain explicitly unsupported without the future authenticated host/Session RPC owner; no new listener/daemon/CLI parser or model-facing management tool exists. This slice does NOT persist private Actor Input bodies or resume multiple messages after a crash. See the AW-P6-9B1 as-built appendix in the master plan for precise native proof and AW-P6-9B2 work.

---

## AW-P6-9B1 — durable Actor owner SDK (host-only; no new RPC) (2026-10-11)

With the full `durable` feature, `aw::actor_owner::{Service,ActorOperation,ActorOperationResult}` composes the existing `aw-agent-core::AgentSessionController` with the current authoritative SQLite v33 `SessionStore` writer/Session/branch. Typed operations are `QueryInput`, `QueryControl`, `ListControls`, `Control`, `InspectInput`, and `InspectControl`. Control actions are the strictly bounded `aw_store::ActorControlAction::{CancelTurn,FreezeTurn,ResumeTurn,FreezeActor,ResumeActor}`. Each new action has a bounded unique request ID and monotonically increasing original-sender message ID shared with Input. The Store must commit a `Prepared` audit fact for the exact active journaled Turn and current owner process **before** the original Runtime's synchronous Control service is entered. Only its `Delivery::Applied` yields the independent `Applied` audit fact; it is not a provider/tool/OS-effect receipt or permission grant. Crashed/missed Control outcomes are Unknown/Prepared and may be explicitly inspected by the new current writer process but **never replayed**.

These Rust types are intended for a future authenticated canonical host driver. `aw rpc --agent-stdio` still rejects unbound `actor_query` and `actor_op` as `unsupported_method`; it does NOT gain an Actor endpoint, trusted Bash/process command, credential disclosure or model-facing Actor tool from deserializing an `ActorOperation`. The current public CLI flag/command grammar is unchanged. The single-Input AW-P6-9A host remains bounded to one outstanding Input and does not persist replayable text or multi-message payloads. Further authorization and durability rules are in the AW-P6 master-plan 9B1 appendix.

---

## AW-P6-9B1 — Control audit provenance is not a queue replay log (2026-10-11)

SQLite v33 retains its original immutable `audit`/`security_audit_events`, Session FKs, Workspace writer epochs, `runs`, `hazards` and normal Agent/Tool Journal. New first-party, bounded `actor.control` audit payloads record an exact original `actor.input` ID while that Input is `Dispatching`, current Session/branch/running Turn and writer epoch, sender/target Actor generation labels, action, message source sequence and **same-process owner incarnation**. The immutable state transition is `Prepared → Applied|Rejected|Unknown`, with explicit `Prepared|Unknown → Inspected`; a new process can inspect, not retroactively claim Applied. Query/list APIs require the stable Session owner; writes require the live local Workspace writer. The original journal alone can prove a tool, paid call or external effect settled. A Control ACK never does.

No SQLite migration, new table/index, JSONL version bump, raw provider-private data or automatically replayable Control/Input payload was added. Ordinary R12 stdio/CLI and JSONL-minimal/stateless do not expose Actor control. Further durable payload custody, multi-message queue CAS and authenticated external RPC remain AW-P6-9B2+ requirements.

---

## As-built AW-P6-9B2A — private multi-Input durability remains a derived Message service (2026-10-11)

The durable full-feature Rust `aw::actor_owner::Service` now composes the existing `HostRuntime` Agent Actor/Message mailbox with a bounded private encrypted Input custodian and SQLite v33 immutable `actor.queue` writer-CAS facts. Only pending encrypted payloads are reconstructible after explicit new owner startup, under the SAME current Session writer epoch, branch, site and independently validated current Agent/Envelope. Claims are committed before dispatch, and per-recipient ACK is committed only after actual 9A inbox Accepted. Original `AgentSessionController::run_once_verified`, `TurnMachine`, `ToolPolicy` and original factual `TurnJournal` are the sole paid/tool execution authority; `actor.queue Accepted` does not claim any effect. Unknown previous claims/paid attempts cannot be replayed and require explicit owner inspection. Queue priority is bounded -20…19 with 32 pending/claimed items per recipient and four-urgent fairness; no client-provided ActorKey or source label is an authorization.

Encrypted queued bodies are held in owner-private `LocalSecretStore`, never plaintext in the queue audit; current Session history still records the real prompt when the original Agent Turn actually begins. The query operation returns only `ActorQueueView`, without encrypted-item references, body hashes, process-incarnation metadata or private text. `Queue` is still NOT a fifth primitive, separate scheduler or universal persistent signal bus. No durable in-flight Control/Notify/Hook, external actor RPC, daemon/background consumer, remote-site execution or multi-process platform certification is implied. Read the [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9B2B observed class vs executable Input — current as-built (2026-10-11)

The original four primitives remain **Runtime, Actor, Message, Envelope**. The original owner Runtime accepts child `Kind::Notify` data only; a derived host-owned `actor.notice` service now commits private ciphertext and immutable Prepared/Accepted/Read/Unknown/Inspected facts through the existing SQLite v33 `audit`, never creating a fifth primitive or second bus. The same original ActorKey generation, child-parent relation, Envelope/site, current Session owner and writer epoch gate actual `Delivery::Queued`. Prepare is NOT Accepted, Accepted is NOT user-read, and read is NOT Agent/Model/Process execution. Private Accepted text may be read under current owner after restart without trying to replay the original Actor's retired key. `actor.custody` removes only ledger-proven retired encrypted queue/notice file references with current writer CAS and authenticated Session owner; never infer unreferenced ciphertext belongs to this service or attempt to delete credential files. See [accepted history](../plans/accepted-history/TODO.md) for native receipts, bounded audit/retention and OPEN orphan quarantine.

---

## AW-P6-9C1 current as-built — opt-in owner-bound local R12 Actor service (2026-10-11)

The original **Runtime, Actor, Message, Envelope** remain the *only* primitives, **Next/Defer** remain the only hooks, and the original Rust `Agent`/`TurnMachine`/ToolPolicy/Journal remain the only provider/execution engine. The existing R12 v1 stdio adapter can now explicitly opt in with canonical `aw rpc --agent-stdio --agent-actor-owner --session-id ID` to one existing current **local** SQLite-owned Session. The CLI, not a caller JSON string, selects the Session and binds current owner/principal, branch, writer epoch, randomly new RuntimeKey/Actor generation, Envelope execution site and zero exact tool grants. The real WriterEpochFence + current `ActorState::Running|Waiting`/Session/Envelope are checked per operation; raw `ActorKey`, epoch, `admin`, source/effect or grant JSON cannot authorize anything. Tool effects remain OFF in this version.

`actor_enqueue` reuses the 9B2A encrypted bounded Pending Actor Input queue with immutable SHA and exact source sequence; `actor_step` executes at most one claimed Input as a new original journaled Agent Turn, with factual settled/Unknown receipts and no replay of previously claimed paid attempts. `actor_query` and conservative `actor_op` expose only selected branch/site metadata, bounded to 128 rows, and explicit inspect/delete/read-ACK dispositions. A private `actor_notice_read` is deliberately distinct from metadata and limited to previously Accepted encrypted observational child notices. There is no alternative bus, hook engine, owner registry, web listener, daemon, cross-host identity, or parallel paid provider. Unbound R12 Actor methods still fail closed. Current platform certification and unreferenced ciphertext quarantine stay OPEN. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9C2 signed MachineLink Actor gateway — current as-built (2026-10-11)

The **9C local+remote owner/RPC slice is now complete** without altering the exact four primitives (**Runtime, Actor, Message, Envelope**) or adding another Agent, ToolPolicy, Journal or durable execution bus. Existing typed R12 `actor_op` can apply an exact *currently Dispatching* owner-verified Turn Control from its independent bounded reader, while the same original Agent is awaiting a provider; immutable 9B1 `Prepared/Applied/Unknown` facts remain the only Control evidence. No `actor_live_control` alias exists. Separately, an explicitly opt-in **Linux AwNet signed Ed25519/MachineLink QUIC** Actor owner binds one live original Agent/Session/branch/site/writer/Envelope with tools OFF. The paired Node signature and negotiated typed capability lease are necessary **but not sufficient**: new SQLite v33 append-only owner-issued `actor.peer_grant` permits only exact peer public key/security epoch, Session+branch+site+writer epoch, short TTL and operation family. Every operation rechecks the independent grant; active Step rechecks grant, Actor generation and signed lease expiry roughly every 150ms and the response is reauthorized before any private text is returned. The owner can revoke an already running peer using strictly typed, bounded **local stdin**, not remote QUIC. SIGKILL/cancel/revoke/expiry never replays an uncertain original paid effect. Read [accepted history](../plans/accepted-history/TODO.md). Real remote *human* identity, production paid-vendor billing, Windows/macOS/ARM networking and release certification are separate OPEN tracks.

---

## AW-P6-8A typed extension settings / approved prompts — opt-in host SDK (2026-10-11)

The foundation remains exactly **Runtime, Actor, Message, Envelope**, with **Next / Defer** the only Hook verbs and a single original `Agent`/`TurnMachine`/`ToolPolicy`/`TurnJournal`. Extension manifests may now supply bounded, own-ID-scoped `add_setting` definitions (no group authority); registered Boolean/Integer/Text/Select types validate persistent **non-secret** user settings through the existing atomic TOML file lock and exact key-CAS, with independent same-group stale-edit merging. Overrides resolve **CLI > freshly approved, hash-pinned project > user > default**. The source-pinned `.agents/settings/<extension>.<name>.json` is inert typed project *data*, rescanned/verified through the original descriptor-relative `Catalog`; no implicit auto-trust or new provider/tool authority.

Approved `PromptRegistry` contributions use typed anchors **Preamble/Tools/Rules/Environment/All**, Before/After/Replace and deterministic priority/id ordering; ordinary approved project `.agents/prompts/<name>.md` content can appear only as quoted reference data before/after Preamble/Environment and is rehashed on every construction. The opt-in `aw::extension_prompts::configured_agent_builder` calls the **existing** Agent factory with a composed startup `system_prompt`, not a new prompt executor or live journal mutation. `SettingKind::Secret` is redacted and cannot use regular TOML/project/CLI setting values; the separately authenticated encrypted `SecretStore` setting backend is future P6-8B. Existing presentation-only status-line/performance builtins remain unchanged; left/right/event/status/media/approval are subsequent P6-8B/8C. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-8B — verified original-owner SecretStore + semantic status/events (2026-10-11)

The exact **four primitives Runtime/Actor/Message/Envelope** and exactly **two Hook verbs Next/Defer** remain unchanged. The current trusted host can opt into **encrypted extension Secret settings** via `aw::extension_secrets` guarded by a current, generation-valid existing `aw::actor_owner::Service`, canonical Session/branch/site, active WriterEpochFence, and currently registered `SettingKind::Secret` declaration. It stores bytes **only** in the existing ChaCha20-Poly1305 private `LocalSecretStore`; SQLite v33 `extension.secret` immutable owner audit contains revision/opaque ref but never plaintext. Replace/reset uses exact CAS with committed fact before superseded ciphertext unlink. An owner-only bounded sweep removes *only ledger-proven retired* refs, not unknown orphan blobs or provider credentials. Non-secret TOML/project/CLI settings stay v8A, and registered secret values are never exposed in public views or auto-enabled through web/stdio/QUIC.

Existing `aw-ui-extension` protocol v2 introduces typed version-driven slots `LeftStatus/RightStatus/BelowStatus/AfterStep/AfterFinalOutput` while keeping v1 manifests limited to original slots. `StatusSurface` is an ephemeral, bounded semantic region map whose version advances on a real change; `EventRail` is an ephemeral per-Session 32-item queue for *presentation-only* original factual `StepEnd/TurnEnd/FinalOutput`, not an Agent Message or another journal/event bus. Original `AgentEvent` metadata maps through `aw::extension_status` without private Turn prompt/answer/provider continuation; a completed original Turn alone may indicate `FinalOutput`. UI status spans reject terminal-control/bidi injection. Opt-in Workspace layout in existing renderer supports configurable side and below-status regions with **no default extra header/footer or pane**. Broader v2 event consumer integration is explicit host opt-in, not a newly running background process. No default sound/video/microphone input, external Hook execution, new grants, SQLite migrations/ABI versions or paid-model duplicate. Read [accepted history](../plans/accepted-history/TODO.md) for exact native tests and 8C open work.

---

## AW-P6-8C — original Message content type, bounded Process frame, human approvals and opt-in notices (2026-10-11)

There remain **exactly four primitives: Runtime, Actor, Message, Envelope** and **exactly two Hook verbs: Next/Defer**, a single original `Agent` and its `ToolPolicy`/TurnMachine/TurnJournal, SQLite v33/JSONL v1, no migration/new Engine/bus/provider. The pure portable `ActorMessage.content_type` field now carries `Text` (default), `Image`, `Audio`, `Video` or `Binary` **only as inert metadata**. Current original text-only mailbox admissions reject ALL nontext types BEFORE consuming an actor/source-sequence or invoking an Agent/Tool/Hook: there is no unauthorized media-to-text promotion. This is a Rust SDK struct-field source change requiring an explicit `content_type: Text` in literal external code; no E5 C ABI, SQLite or JSONL schema changed.

New `aw-tools::media_port` borrows an existing host-authorized, original journaled `PipeProcess` for small bounded `Text/Binary/Image` DATA frames. The Process host must still independently establish effect intent/source permission/Envelope: this adapter neither spawns a process nor fabricates an external receipt and *never* auto-retries a partial/unknown write. Microphone, Speaker and VideoCapture are **unconditionally denied**. Actual realtime audio needs future explicit positively authenticated owner/user consent, OS device permission, revocation, private retention/erasure, backend TLS, process isolation and full certification; no device/mic/provider is enabled in this phase.

Existing Phase50 `SessionStore::review_decide_human` and `review_release` remain the sole human decision/evidence release authority: host-only `aw::approval_ui` narrows pending/inspect/SHA-fenced decide to original currently bound Actor owner, Session and branch, and `aw review pending SESSION BRANCH` gives the same currently initiating local user a bounded, sanitized/redacted READ-ONLY list. No model/browser/cookie/signed peer can impersonate the actual human. Existing v2 `EventRail` is optionally activated via `UiConfig.show_event_notices` (**false by default**, original config atomic CAS); factual metadata from original Agent yields ephemeral StepEnd/TurnEnd/FinalOutput lines only in the **existing Activity panel**, never assistant transcript/default header/pane, and is discarded on Session switch/restart. No alternate journal or new default media UI/approval modal. See [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D1 — opt-in model observation through the original ToolRegistry/Agent (2026-10-12)

**Exactly four primitives** Runtime/Actor/Message/Envelope and exactly two Next/Defer Hook verbs remain unchanged. The original single `Agent`, `ToolPolicy`, `ToolRegistry`, `TurnMachine`, `TurnJournal`, host `AgentSessionController` and SQLite v33 encrypted Input queue/control/notice facts are the ONLY execution and durable record paths. `aw::model_management::install` is an **explicit owning host Rust SDK** action, OFF by default in CLI/R12/TUI/web, requiring exact current original Session+branch/current writer+Actor generation and preexisting original Envelope Host/trusted-host/epoch1/`Read` grants for the requested names `agent`, `queue`, `process`; no parser/model/remote peer may mint them. Original trusted `Agent::register_host_tool` then issues the matching ToolPolicy exact name/source/effect/site Read grant; the original ToolRegistry/ToolPolicy and AgentStepGate recheck this pair at original provider advertisements, dispatch and post-result disclosure.

The three strictly typed model Tool ops are `agent {op:status}` (active immutable original Turn metadata only), `queue {op:list|query, request_id?}` (max32 redacted same owner branch/site metadata, encrypted payload and SHA unavailable) and optional `process {op:status,handle}` (max16 host-registered opaque shared handles to original already authorized Process, numeric captured counts/truncation/failure only, no PID/path/env/stdout/stderr/private body or process control). Model-visible `ToolContext` must match current original selected account/payer, Session/branch/site, uncancelled positive Turn, original Controller active Input and immutable SQLite `actor.input` **Dispatching**; stale/foreign/settled/Unknown/retired Actor state denies. Source grants may only be preinstalled by the owning Rust host; no new Agent, Tool effect, model runner, Process, listener, DB schema, ABI/version or auto-replay. Future **9D2** separately reviews effectful model-origin queue/agent/Process management with audited intents and independent owner permission; P6-10 cleanup/certification remains open. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2A — one original Agent model-origin queued future Input (2026-10-12)

The **exact original Runtime / Actor / Message / Envelope four primitives**, two **Next / Defer** Hook verbs, single original `Agent`, `ToolRegistry`, `ToolPolicy`, `TurnMachine`, immutable original TurnJournal and SQLite **v33** audited actor.queue remain unchanged. The trusted host may explicitly `aw::model_effects::install_queue_enqueue(&Service,&mut Agent)` only when its same original authenticated live Session/writer/Actor and previously minted exact Envelope `Host/trusted-host/epoch1/Write` source grant already authorizes `queue_enqueue`. The original `ToolPolicy` independently grants the same exact cached Host/Write tool, and existing provider source/effect gates and immutable ToolStarted/ToolFinished/Unknown receipts remain the only execution path. Ordinary CLI/R12/TUI/web/signed MachineLink do not install it. The model JSON is only `{"op":"enqueue","text":"bounded UTF-8"}` and cannot supply ActorKey/writer/sequence/priority/target/source/approval. The original active immutable `Dispatching` Input and real exact Turn/selected payer are rechecked before/after enqueue. The host derives **exactly one deterministic `mq_` SHA256 request per originating Session+branch+Turn**; the original SQLite queue `BEGIN IMMEDIATE` allocates the next historical sender sequence with original cross-Input/Control/Queue/Notify collisions and reserves immutable future audit capacity. Success yields only an encrypted **Pending** future queue fact, NOT a model/effect settlement or permission to execute next Turn. Current owner must later explicitly claim and invoke the original Agent; duplicate model Write hazards may leave original Turn Unknown/inspection-required and are NEVER automatically replayed. The queue-specific audit remains plaintext-free, while the original Agent's immutable provider ToolCall arguments may legitimately include the model-generated proposed text. No new Process owner/daemon/media backend, queue recursion, Hook, C ABI, JSONL or SQLite schema was created. More model-origin Agent/Process/guardian/queue mutation operations are future separately scoped D2B, not assumed granted by `queue_enqueue`. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B1 — original model-owned pending queue disposition (2026-10-12)

The immutable original **four primitives Runtime/Actor/Message/Envelope**, two Hook verbs **Next/Defer**, single original Agent/ToolPolicy/ToolRegistry/TurnMachine/TurnJournal and SQLite v33 encrypted actor.queue remain sole execution and durable authority. The trusted owner may explicitly install distinct `queue_update` and `queue_delete` original `ToolEffect::Write` entries only when matching **separately pre-minted Host/trusted-host/epoch1/Write** exact Actor Envelope grants exist for each name, on the same live journaled Agent. No default CLI/RPC/web/remote source/host grants, another executor, tool alias, schema/ABI/version migration or automatic second paid Turn. Strict model JSON names only an existing `mq_` queue ID and positive exact revision (plus bounded replacement text for update); it cannot supply origin Actor/Session/site/epoch/source, recipient, priority, target or authority. An `mq_` prefix alone does not grant ownership: D2B1 verifies the exact shared D2A deterministic request hash belongs to an **earlier factually Settled original Session+branch+Actor-sender Input Turn**. The target must still be Pending on that same original Actor sender/site; ordinary user queue or claimed/accepted work is forbidden. One originating immutable current `actor.input Dispatching` original Turn computes one deterministic `md_` disposition intent shared across Update and Delete. The original SQLite writer transaction verifies exact owner/writer/originating Input, CAS revision and native queue future-closure capacity, and atomically commits the next `actor.queue` fact AND an append-only `actor.model.queue_disposition` marker to prevent any second mutation/replay per originating Turn. Encrypted write-once ciphertext for Update precedes commit; superseded or Deleted blob unlink follows commit, subject to unknown/inspection on error. The original Agent tool journal still records model-proposed text; queue/disposition metadata does not. Future edited content is only run by a separately explicit original owner `admit_next_queued/run_once`, while Delete prevents delivery. Process/Agent/guardian control model effects are NOT included in D2B1 and require separately audited source/receipt/revocation. See [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B2A — original Process-hosted stdin close, no second effect executor (2026-10-12)

The **2026-10-11** four-primitives/two-hooks architecture stays exact: Runtime, Actor, Message, Envelope; only Next/Defer, one original `Agent`, ToolRegistry/ToolPolicy/TurnMachine/immutable TurnJournal, SQLite v33/JSONL v1. The trusted current host can explicitly enable one **`process_close_stdin` Write** Tool through the existing original Agent only after its original Actor Envelope was bound with the exact **Host/trusted-host/epoch1/Write** name/source/effect grant. A D1 Process Read or D2A/D2B1 queue Write grant does NOT authorize this action. `aw::model_process_control::install_close_stdin` uses the existing `ProcessRegistry` of ≤16 exact original Session/branch/site/principal/writer-epoch-bound opaque handles to independently authorized/journaled `PipeProcess`, not PID lookup/spawn/signal/new process owner. The model supplies only `{op:close_stdin,handle:<opaque>}`, no PID, stdin bytes, command/root/env, source or target. Under current real `actor.input Dispatching` and actual active original Runtime Turn, exact owner/writer/Actor generation/provider/payer and uncancelled ToolContext, the host mints a deterministic once-per-Session+branch+Turn `mp_` ID and commits an existing SQLite v33 `audit`/security audit `actor.model.process` **Prepared** fact in `BEGIN IMMEDIATE` before closing exactly that original Process stdin. Only the same original owner Process incarnation can append Applied/Rejected/Unknown; after restart Prepared/Unknown is host **inspect-only**, no model/Tool/Process replay or fabricated external receipt. Applied says only that original stdin was closed, not paid provider/Process exit/OS kill. Private Process PID, program, args, env and stdout/stderr are never exposed in model ToolOutput/audit. No new default UI/RPC/model Tool, actor cancellation, generic Process write/signal or model human guardian grant; D2B2B and P6-10 remain separate. Read the [accepted history](../plans/accepted-history/TODO.md) and existing original Process custody rules.

---

## AW-P6-9D2B2B1 — exact original host-approved Process termination, one immutable model intent (2026-10-12)

Exactly the original 2026-10-11 four primitives Runtime/Actor/Message/Envelope and two Next/Defer Hook verbs remain; ONE original journaled Agent, ToolRegistry, ToolPolicy, TurnMachine, SQLite v33 `actor.model.process` and JSONL v1. `aw::model_process_control::install_terminate` is explicit TRUSTED owning-host opt-in, OFF in ordinary CLI/R12/TUI/web/AwNet, requiring its **OWN** current original Actor Envelope exact Host/trusted-host/epoch1/**Write** `process_terminate` grant plus matching original ToolPolicy. A D1 Process Read, D2B2A Process CloseStdin Write or tool/model text does not grant it. A Process must have separately been registered with `ProcessRegistry::register_terminable_owned`, which mints a host-private **random 32-byte Process incarnation** and checks the same authenticated original owner/Session/branch/site/current WriterEpoch; default read-only `register_owned` is ineligible. The model may supply ONLY `{"op":"terminate","handle":"opaque"}` and no executable/PID/stdio/signal/root/source/Actor/approval. The host validates original current immutable `actor.input Dispatching`, actual Runtime active Turn and selected payer/credential before deriving the same stable per-originating Session+branch+Turn `mp_` ID shared with D2B2A stdin-close. The existing SQLite v33 `BEGIN IMMEDIATE` appends `actor.model.process Prepared` with `ModelProcessAction::Terminate` and private Process binding BEFORE the original PipeProcess OS group-stop/reap/drain; old CloseStdin JSON omits binding and remains compatible without schema change. Original Process in-memory `termination_attempted` guard irreversibly forbids a second signal. Only the same original owner Process incarnation may append Applied/Rejected/Unknown; crash/new owner Inspects only. Applied means original direct child termination attempt reaped and drained, Process group best effort, NOT verified descendants or paid provider settlement. Source/envelope, Process binding, current writer, original Tool journal, no-replay and no-default-grant invariants hold, with no extra Agent/OS executor, SQLite schema/ABI/version migration or new Hook. Model-origin Agent self-cancel/freeze/child, arbitrary Process stdin/signal/PTY and real human guardian approval remain OPEN; [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B2B2 — durable future original Agent admission hold, NOT an in-flight Control (2026-10-12)

Exactly the 2026-10-11 original four Runtime/Actor/Message/Envelope primitives and two Hook verbs Next/Defer, one original Agent/ToolRegistry/ToolPolicy/TurnMachine/TurnJournal, SQLite v33/JSONL v1/E5 ABI and original `Service` remain. A trusted current owner may explicitly install only a model-facing `agent_pause_after_turn` original ToolEffect::Write under independently pre-minted original Actor Envelope exact `Host/trusted-host/epoch1/Write` grant. Normal CLI/R12/TUI/Web/QUIC do not enable it. Model JSON permits ONLY `{op:pause_after_turn}`. While the currently original journaled immutable `actor.input Dispatching`/Runtime active Turn and original owner/writer/Actor generation/source/site/account/payer are valid, the original Store commits one domain-separated Session+branch+Turn `ap_` `actor.model.future_pause Armed` immutable audit fact. It NEVER calls `ControlKind::Cancel/Freeze/Resume` on its own running Turn, never interrupts the current provider or Tool receipts. The existing original `actor_input_prepare` and `actor_queue_claim_next` check the same durable Armed branch hold *inside their original SQLite BEGIN IMMEDIATE* before appending Input Prepared/Queue Claimed, including direct Store callers and after restart. Existing encrypted Pending queue items remain retrievable but not executable until a new independently original authenticated idle owning Host `Service::release_future_pause` explicitly writes Released, only after original source Turn factually `Settled`, no active Run or other unresolved Actor Input. An original Unknown paid provider/Tool keeps the hold armed; duplicate proposed model Write cannot fabricate settlement or replay. This is a derived Session+branch admission fence ONLY, not universal Actor FreezeActor or another background actor runtime. Child cancellation, Process arbitrary signals/stdin and human guardian approval remain separately NOT enabled. See [accepted history](../plans/accepted-history/TODO.md).
