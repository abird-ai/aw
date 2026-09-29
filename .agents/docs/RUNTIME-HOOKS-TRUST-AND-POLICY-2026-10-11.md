# aw runtime — typed hooks, trust Envelopes, approvals and policy services

**Status:** Proposed normative contract, 2026-10-11, **DESIGN ONLY**. Companion to [four-primitive architecture](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md) and [Runtime contract](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md). This file specifies the **services** over Runtime, Actor, Message, Envelope. It does NOT install hooks, grant capabilities, enable browser/project trust, choose a process sandbox, change SQLite v33, publish an API, or start E6/R13.

## 0. Contract in one sentence

**Every admitted flow passes through one typed ordered hook pipeline that can only return the next message or defer; every effect also passes a non-bypassable Envelope authorization and durable intent/receipt boundary.** A hook or decider proposes/observes; only the owning Runtime/Agent/host adapter authorizes, performs, journals and publishes.

Exactly four primitive concepts: `Runtime`, `Actor`, `Message`, `Envelope`. Exactly two hook verbs: `Next`, `Defer`. Queue, signal, hook pipeline, approval, policy rules, reviewer, model selection, child spawn and prompt/status/config contributions are derived services, not alternative execution cores.

## 1. Normative hook algebra

### 1.1 Two verbs in typed Rust

~~~rust
// Illustrative pure API; not a current crate symbol or implementation.
pub enum HookFlow<T> {
    Next(Option<T>),            // same, edited, replacement, or consumed
    Defer(ContinuationKey),    // park; resume via Runtime only
}
pub type HookOutcome<T> = Result<HookFlow<T>, HookError>;

// Handler context contains a validated immutable actor/envelope view,
// cancellation, deadline, source identity and explicit allowed operations.
pub trait HookHandler<E>: Send + Sync {
    type View;                  // stage-specific; NO arbitrary authority fields
    fn handle<'a>(&'a self, ctx: HookContext<'a>, event: Self::View)
        -> HookFuture<'a, HookOutcome<E>>;
}
~~~

`Next(Some(same))` = continue or observe; `Next(Some(edited/new))` = edit/replace; `Next(None)` = consume (but the core must still close required provider/tool correlation and report a typed rejection). `Defer` = Runtime continuation; the pipeline resumes exactly once when a validated reply arrives or resolves to an explicit timeout/cancel failure.

`Branch` = host-authorized Actor spawn (or use existing actor) + bounded continuation. `Emit`/`Fork` = `Runtime::send` of an authorized ActorMessage. `Await` = a defer with a wakeup. `Decision` = a hook handler with a typed verdict body. None adds an action to the primitive hook enum.

**Important:** The hook may not call the raw provider or tool runner directly. If a trusted native extension has same-UID arbitrary execution privileges, those are OS powers, **not a policy guarantee**. The supported high-level host services must still enforce the Envelope; do not claim to sandbox linked arbitrary Rust/C code in the same process.

### 1.2 Stage- and kind-specific types

Do not give all handlers a mutable `serde_json::Value` with a flag that says "trusted". Use a stable typed `Stage` enum, typed sub-kind and a typed projection. A single registry/dispatch engine may internally erase handler types behind a validated adapter, but **capabilities stay distinct**:

~~~rust
pub enum Stage {
    Turn, Step, Input, Prompt, Message, Output, Tool,
    Signal, Event, Lifecycle,
}
pub enum HookRegistration {
    Observe(ObserverRegistration),
    Transform(TransformRegistration),
    Gate(GateRegistration),
}
pub enum StageIntent {
    Turn(TurnEventKind),           // start/end/steer
    Step(StepEventKind),           // start/end/model_select/compact
    Input(InputEventKind),         // user/child/queued
    Prompt(PromptEventKind),       // assembled/authorized
    Message(MessageEventKind),     // append/serialize projection
    Output(OutputEventKind),       // public text / arguments / media
    Tool(ToolEventKind),           // call/result
    Signal(ControlEventKind),
    Event(ObservationKind),
    Lifecycle(LifecycleKind),
}
~~~

`StageIntent` is an ergonomic sum-type, not a new primitive. `Event` and `Lifecycle` are strictly observe-only. `Tool/call` is a veto/narrow gate; `Tool/result` allows **model-visible presentation transformation** only. `Prompt` can transform approved contributions but not permissions. `Step/model_select` is a host-reviewed proposal seam, not direct credential/account switching. `Message` may transform a derived display projection; the factual journal record is immutable.

### 1.3 Stage surface and allowed changes

| Stage/sub-kind | Stage payload | Allowed hook action | Hard boundary |
|---|---|---|---|
| `turn/start/end/steer` | Turn ID, actor, existing admitted input | Observe; propose bounded scheduling | Never retarget a running Session or rewrite a settled Turn |
| `step/start/end` | Step ID, attempt reference and tool batch status | Observe / scoped supplemental context | A model call/receipt is already journal-governed |
| `step/model_select` | current pinned model/provider/account identity | Propose option within existing allowed registry/custody | No implicit payer/account/provider change |
| `step/compact` | source-pinned context, cut/summary proposal | Approve/deny/transform bounded summary | Preserve original evidence, summarize through paid receipt |
| `input/user/child/queue` | source-labeled text and control options | Redact/edit/consume | Input cannot become trusted system/tool grant |
| `prompt/system` | approved typed sections/provenance | Append or explicitly owner-authorized replacement | Mandatory policy declarations remain host-owned; resources data-only |
| `message/user/assistant/tool/system` | transcript projection | Safe display/wire mapping by consumer | No mutation of immutable durable original events or private continuations |
| `output/text/think/args/audio/item` | incremental content with privacy and finality flags | Consumer-appropriate redact/transform | Never publish private signed reasoning or raw secrets |
| `tool/call` | complete call, source/effect/schema/grants | Deny or narrow arguments; optional review/defer | Every edit revalidated; mandatory intent before effect |
| `tool/result` | factual ToolReceipt + public view | Transform bounded **view** | Cannot turn Unknown into Success or alter receipt/call ID |
| `signal/*` | addressed control, issuer/target and generation | Observe/deny/narrow delivery on specific allowed sub-kinds | Mandatory cancel/revocation not blockable by third party |
| `event/*` | immutable event projection | Observe only | Never a journal, never authorization |
| `lifecycle/*` | actor transition evidence and status | Observe only | Terminal event cannot be suppressed or rewritten |

### 1.4 Ordered dispatch

- One ordered registry. Handler order ascending `order` then stable monotonic registration ordinal, scoped by stage/sub-kind, owner extension and host trust class. No implicit parallel hook handlers on a single item.
- Snapshot the registration set at entry. Hot changes affect the **next** item, not an in-progress walk. Unregister/unload invalidates tokens for future use, cancels pending defers and handles the current callback lifetime without use-after-free.
- Every transform returns an owned validated next message or a borrowed unchanged view whose lifetime cannot escape. Host owns argument buffers; no cross-await borrow of temporary provider JSON or raw secret-bearing data.
- Continuation is a bounded token tied to ActorKey generation, Envelope revision, stage instance, handler generation, source ID, deadline and one-shot completion. Cancellation/revocation/invalidation turns it into a settled typed failure.
- Max depth and per-item active-handler reentry set prevent recursive loops. Bound handler count, payload bytes, total expansion, depth, branch count, total hook wall time and chain hops; provide cheap `wants(stage)` hot-path guard.
- Errors default to **fail closed** for admission/authorization/gating stages (including ToolCall); **observe-only** faults produce diagnostics and never change factual effects. A failed optional display transform may fall back to the original **public-safe** view, never to private bytes.
- Each hook includes source provenance/owner identity and hash-before/hash-after for safe audit, with secret-value redaction. Redaction/audit hooks cannot replace the journal.

### 1.5 Internal, MCP and external tools

There is **one tool admission pipeline** for internal `aw-tools`, explicit `aw-mcp` capabilities and `aw-extensions` tools; `RunnerKind::Internal | Process | Mcp` may select the execution adapter. The new Process Actor is the resource backing shell/stdio subprocesses, **not a separate tool grant source**.

- Tool **registration** publishes capability descriptions, not permission.
- Complete schemas and exact source/effect metadata are pinned and validated before effects.
- Hook/decider may return narrowed arguments only; the host **revalidates** schema, exact grants, roots, source revision, execution site, destination and budgets after changes.
- There is a documented **mandatory** envelope/journal pipeline outside optional hooks. `tools.hooks=off`/`NO_HOOKS` (if offered) can remove optional observations, not grants, receipts, approval triggers or cancellation.
- An unauthorized tool (including unknown remote hints, a management `process` tool or dynamic `MCP` tool) returns a correlated typed denied tool result. No unpaired tool calls or provider errors for ordinary tool validation failures.
- Tool execution follows existing parallel wave rules: source-order intent acknowledgements; only exact Read or separately authorized model-directed Bash waves; no Write/Unknown speculative execution; receipts recorded before model continuation.

## 2. Envelope: precise authority lattice

### 2.1 Existing kernel predicate is the base, not a compatibility path

Retain `aw-agent-kernel::capability::{Source,Capability,Grant}` and `TurnMachine` as load-bearing. The effective host-issued grant is matched on:
`principal/actor` + `execution site` + `source kind/id/epoch` + `capability name` + `effect` + `current catalog revision` (as applicable). These are host-authored; remote names/descriptions or model arguments are untrusted.

**Proposed cleanup:** remove `ToolPolicy.actor_grants`, `ToolPolicy::permit` and `ToolSource::is_legacy_host` trusted-host name-only special case **after** converting host tool selection to exact host-issued grants (`SourceKind::Host` plus minted source ID/epoch). Preserve host usability via the same explicit `--tools-mode`/`--tools-allow` host policy, but have one authorization predicate. Do not remove the existing permission gate until replacements pass denial/revocation tests.

Default permission is `Off`. The P6 `RW/workspace/auto` default does not apply in aw. An explicitly granted `workspace` mode may allow a particular operation inside its root without repeatedly asking, but absence of an exact grant is still denial. Model-facing prompts are merely declarations, never a substitute for execution enforcement.

### 2.2 Envelope fields and policy order

~~~rust
// Illustrative split: pure constraints + host-verified bindings.
pub struct Envelope {
    pub revision: EnvelopeRevision,
    pub allowed_tools: ExactGrantSet,
    pub access: AccessMode,                // ReadOnly | ReadWrite
    pub roots: ScopedRoots,               // descriptor/no-follow bindings
    pub network: NetworkPolicy,           // Deny | Allowlist | explicitly trusted
    pub process: ProcessPermissions,      // shell/PTY/background/write/freeze
    pub media: MediaPermissions,          // microphone/speaker: disabled by default
    pub model: ModelSelectionBounds,      // allowed account/provider/payer/model/site
    pub budget: BudgetEnvelope,           // parent/root ceilings, time/usage/output
    pub approval: ApprovalPolicy,         // OptionalAsk | HostRules | Reviewer
    pub authority: AuthenticatedBinding,  // principal/owner/Session/site/writer
}
pub enum PolicyVerdict {
    Admit(AdmittedProposal),
    Reject(Denial),
    NeedsApproval(PendingApproval),
}
~~~

A `PolicyVerdict` is a **service data type**, not a fifth primitive or a new hook verb. The hook still returns `Next`/`Defer`. Controller effects are ordinary validated Messages admitted individually; a policy object cannot directly spawn an actor or change durable state.

Precedence: hard root/OS denies > authenticated explicit human denial/revocation > exact source/site/effect authority > host policy constraints > optional hook/reviewer narrowing > display prompt guidance. A later "allow" never overrides an earlier hard denial.

**Monotonicity law:** `effective_child = parent ∩ requested ∩ current_host`. A child may reduce read/write, roots, network, process, mic/speaker, tool set, provider/account set, budget, depth, deadline and reviewer scopes. A child may not turn `ReadOnly` into `ReadWrite`, add a server, change execution site or increase budget. A separate owner authorization may create a **new** root grant revision with explicit audit, but does not rewrite the old child Envelope.

### 2.3 Resource access vs kernel confinement

- Filesystem: `aw-platform-unix::scoped_fs` and Windows equivalent enforce descriptor-rooted operations for in-process tools. Path string checks alone are not sufficient against symlinks, rename races, hardlinks or mount changes; do not claim universal containment of uncooperative same-UID actors.
- Process/PTY: spawned OS commands and native extensions execute with the launching user privileges unless a separately verified sandbox backend is configured. Cleared environment, restricted cwd and process groups **do not** confine filesystem or network.
- Required sandbox: backend capability probe and policy are explicit; unsupported backends fail with `SandboxUnavailable`. Do not silently fall back from `Strict/WorkspaceSandbox` to `None`. Linux/macOS/Windows confinement remains target-specific validation and is not claimed implemented by this document.
- Network: MCP/HTTP endpoints, remote process dispatch and provider connections use explicitly pinned destination/identity constraints. A child's chosen host cannot expand the parent's network set.
- Grants are checked at both initial admission **and again after every awaited hook/journal/decision before I/O and before output disclosure**.

## 3. Approvals: typed, scoped, revocable service

A request is host-generated from an exact proposed operation, not an unverified text line.

~~~rust
pub struct ApprovalRequest {
    pub request_id: ApprovalId,
    pub actor: ActorKey,
    pub envelope_revision: EnvelopeRevision,
    pub owner: PrincipalId,
    pub session: SessionId,
    pub operation: AuthorizedOperationFingerprint,
    pub source_epoch: SourceEpoch,
    pub effect: ToolEffect,
    pub scope_options: ApprovedScopeOptions,
    pub expires_at: MonotonicDeadline,
    pub reason: ApprovalReason,
}
pub enum ApprovalVerdict {
    Deny { reason: Denial },
    AllowOnce { expected_fingerprint: Digest },
    AllowStep { expected_step: StepId, expected_fingerprint: Digest },
    AllowSession { expected_session: SessionId, expected_fingerprint: Digest },
    AskHuman { reason: ApprovalReason },
}
~~~

The `Allow` variants are only a **decision record**. Actual execution still requires all current Envelope checks and a durable effect intent. `project`-scope persistent approval is **not automatic**: only a separately user-approved project policy and source-pinned trust revision can enable it. Use existing config CAS/lock. An invalid response, timeout, invalid fingerprint/actor/epoch, mismatched effect or missing approver denies; do not interpret a reviewer as a user identity.

Approval lifecycle:
1. Verify original action at `tool/call`; derive fingerprint including normalized arguments, exact tool source/epoch/effect, actor, site, Session, policy revision.
2. Evaluate deterministic host policy first. If decision definite, no model review expense.
3. If review required, `Defer` with persisted/enforced deadline, typed `ApprovalRequest` and one owning controller; parent/owner handles child requests through a pumped runtime.
4. Human, allowed external program, or bounded reviewer answers. Program/MCP/model output is untrusted verdict DATA, never instructions.
5. Validate verdict and authorized scope; record decision evidence; narrow arguments as specified and **revalidate**. Commit effect intent, then execute and factually receipt.
6. On cancel, revocation, process disconnect, restart or timeout, resolve approval as denied/unknown; no unanswered "pending forever".

No approval while waiting may block the Runtime mailbox drain. Approval scope cache keys include current actor/site/source/epoch/effect/config digest, so a "same command" under a different source never inherits consent.

## 4. Deciders, router and guardian: hook handlers, not new engines

P6 R-D20/R-D23/R-D28 are adopted: routing, approval, spawn, model_select, compact, steer_policy and escalation are **intents of typed handlers** at `input`, `tool` or `step`. They are not stages, primitive types or independent Agent loops.

Backends: `LocalRules`, `Program`, `Mcp`, `ModelReviewer`, `Human`. Backends run through explicitly selected host/provider/tool policy and separate journal/budgets as appropriate. Model review may be expensive and should be disabled by default; deterministic local rules short-circuit.

~~~rust
// A reviewer may return only a bounded proposal. No raw action-list executor.
pub struct ReviewProposal {
    pub verdict: GateProposal,           // Allow | Deny | Narrow | Ask
    pub narrowed_args: Option<ValidatedArgsProposal>,
    pub route: Option<RouteProposal>,
    pub next_model: Option<ModelProposal>,
    pub child: Option<ChildTaskProposal>,
    pub queue: Option<QueuedInputProposal>,
    pub explanation: BoundedReason,
}
~~~

Each optional effect is separately mapped to `Runtime::send`, an authorized Actor spawn or a host-approved model/account setting change. Applying one approved tool-call verdict **does not imply** approval for child spawn, model switch, queue action or durable publication. If the proposal contains unsupported, unbounded, unrecognized or unauthorized effects, reject the record or those effects atomically by a documented policy.

- **Planner:** existing tool-free `PlannerExecutor` produces validated task proposals and retains existing provenance, not a new tool engine.
- **Guardian:** existing tool-free `ReviewExecutor` returns restrictive verdicts and never mints authority. Optional per-tool review uses same backend with its own paid/unknown receipts and max-review budget.
- **Router:** existing pinned child routing uses host-selected account/payer/destination revision, not text-selected endpoints. A classified `input` handler can suggest a route; host restricts it to the approved set.
- **Controller:** one hook handler can propose a multi-effect record, but the Runtime admits each effect through its normal Envelope, epoch and durable rules; no privileged JSON interpreted directly by core.
- **Loop guards:** max delegation hops (proposed 2), actor depth/budget, cycle detection and per-intent idempotency; no recursive model reviewing itself without an explicit bounded route.

**Replay:** persisted proposal/decision/approval is **evidence of a previous decision**, never an executable action. Reconstruction of a Session may display it but does not call the decider again or re-run child process/paid model.

## 5. Process control policy

Model-facing management surface may expose **three** narrowly granted tools, as P6:
`process {op}`, `agent {op}` and `queue {op}`. This is the **management tool family**, not the entire tool catalog. Existing read/write/edit/ls/find/grep/bash remain ordinary model-facing capabilities, all through the same `tool` pipeline.

| Operation | Required additional conditions |
|---|---|
| `process.spawn` | Exact Process effect grant, authorized executable/shell/site/root/env/deadline; effect intent before spawn |
| `process.read/list` | Exact actor/owner/site and output visibility; no arbitrary process enumeration |
| `process.write` | Separate effect admission, bounded bytes and effect receipt; stdin write can itself be external effect |
| `process.resize` | Owner/attach authority, bounded dimensions, supported PTY |
| `process.kill/cancel` | Controller authority, graceful/force policy; confirmed exit vs unknown |
| `process.freeze/resume` | Controller and platform support; frozen timer semantics; no false OS-level freeze |
| `process.attach/detach` | Authorized read/UI session; never transfer ownership or rights |
| `agent.spawn` | Explicit tool/Session/root-budget/worktree/providermodel grant, max depth and routing revision |
| `agent.wait/send/interrupt/result` | Parent/owner controller authority, current Actor generation, live Session |
| `queue.add/update/delete/reorder/pause/start` | Controller-owned pending input, journal/audit; no grant or effect scheduling bypass |

`aw agent` on CLI already denotes AwNode/Machine runtime. Do not reassign it to child Agent management. New model tools and RPC operands are explicitly named and initially unadvertised; no hidden aliases for old bash APIs. `process` and `bash` may share the same resource supervisor, but a legacy `bash` alias is not required. Existing `bash` is a separate explicit coding tool for foreground trusted execution; that semantic difference is intentional.

### 5.1 Process lifecycle safety

`Starting → Running/Waiting/Stalled/Frozen/CancelRequested → Settled/Failed/Cancelled/Unknown`; progress and stall are advisory. Process cancellation: gracefully close stdin/TERM the group, then force after cap, reap/confirm; on Windows use native job/process semantics, not assumed Unix signals. Freeze process only if platform success reported; an Agent can cooperatively quiesce at safe points. Freeze does not advance into a new mutation, but cannot roll back a running subprocess/paid attempt. Deadlines suspend only for *truly paused* phases. Runtime teardown reports uncertain children instead of claiming cleanup always succeeded. PTY creation/resizing must fail clearly when unsupported, never silently run pipes instead.

Output: bounded pending unread ring + tail and optional full spill with 0600 ownership; explicit truncation count; stdout/stderr merge only when requested and recorded. A spill failure must not advertise the truncated buffer as a full transcript. No unbounded reader allocation. Model disclosure limit remains distinct from process output-storage limit.

## 6. Settings service, prompt contributions and status

### 6.1 One typed settings registry per extension owner

`settings.<extension-id>.<key>`, no parallel "group object". Declared `Bool|Int|Float|String|Enum|Path|StringList` with type, bounds, default, label, optional restart and source-scope flag. Resolution: explicit host CLI > *trusted and eligible* project > user config > declaration default; unknown keys/invalid values do not become granted authority. Secret values stay in the existing credential/SecretStore custody; never place bearer tokens in the project config, journal, model/tool prompts or `get_settings` RPC by default.

Settings edits use current config CAS/lock and validated policy. A cross-user project settings file may not loosen grants or alter a connected principal's device credential. SDK/TUI/RPC share one settings service; front end is only a viewer/editor of authenticated allowed fields. No implicit project or external extension initialization on `/settings` listing.

### 6.2 `add_prompt` as explicit system-prompt contribution

Adopt P6's precise terminology:
- `add_setting`: typed configuration;
- `add_prompt`: **system-prompt contribution**, not user input;
- `prompt` stage: transform host-composed prompt within authority bounds;
- `input` stage: user/task text.

Four anchors: `Preamble`, `Tools`, `Rules`, `Environment`, plus `All`. Modes: `Append(Before|After)` and `Replace`, deterministic per-anchor priority and registration. Explicit `--system` is the owner-selected override and wins in the declared configuration layer; an authenticated trusted host can replace the preamble/All, but project markdown/MCP/extension data alone cannot replace authoritative policy. Even with an owner-supplied system prompt, **ToolPolicy/Envelope/journal checks remain actual execution gates**.

Canonical assembly order:
1. Resolve current approved host/actor envelope, live source-pinned project context and currently authorized tool metadata.
2. Resolve host-approved `Replace(All)` or per-anchor replacement only when caller has that authority; no silent project/user-credential escalation.
3. Render `Append(Before)`, winning anchor renderer, then `Append(After)`. Tool descriptions/guidelines derive from live **authorized** tool table, not remotely claimed permissions.
4. Apply a `prompt` transform with source/provenance, time, size, and policy checks.
5. Journal the final prompt digest/source manifest before any paid provider invocation; never export provider-private signed continuation as prompt "text".

### 6.3 One status contribution and notice channel

Adopt P6's `region + lifetime` design as a **service over existing `aw-ui-extension`**. Current Slots are `AboveComposer`, `BelowComposer`, `AfterTurn` and `FullscreenStatus`; proposed richer semantic regions `AboveComposer`, `BelowComposer`, `StatusLeft`, `StatusRight`, `BelowStatus`, `StepEnd`, `TurnEnd`, `FinalOutput` map to the same renderer, not another status footer.

Lifetimes `Persistent` (version-driven chrome), `PerTurn` and `Once` (turn/step notices). A registered contributor returns bounded semantic spans/lines with tone/style, never arbitrary ANSI control or HTML. A per-step/per-turn line is a display **notice** event, not a fabricated assistant/tool result; immutable original Session evidence stays intact. Status cannot modify the composer, bypass scrollback ownership, shift terminal geometry unpredictably, or expose private reasoning. UI extensions keep *presentation-only* trust, distinct from Agent hooks.

### 6.4 Voice as derived service, OFF

Add opaque `content_type`/media metadata only where needed in typed Message. A voice helper may be an explicitly selected `Process` Actor with bidirectional framed pipe, using the same process supervision as MCP stdio; no extra Actor kind or core tool. `MediaPermissions` includes separate `mic` and `speaker` (deny by default); model tool calls from realtime items still cross the standard `tool/call` gate and journal. Barge-in queues a turn cancel, preserving unknown paid/effect state.

No default capture, recording, listening device access, cloud upload or voice helper start. Audio, transcription and storage opt-in are separately documented with retention/security before implementing a real backend. No browser microphone claim or E6 on this design track.

## 7. Privileged process extensions vs UI extension hosts

Existing `aw-extensions` is an explicitly selected versioned bounded process tool/hook service, with source-minted revision and a selected absolute executable. It is NOT an OS sandbox. `aw-ui-extension` is immutable UI projection only. Trusted compiled Rust code shares the host OS principal; its callbacks may be controlled by type-level host APIs but cannot be sandboxed in-process against malicious code.

All three attach to the **same typed pipeline** through a trust-class-aware adapter; there must not be a separate unsafe hook engine for MCP, before_tool, reviewer, web or child Sessions. Still enforce exact source/hook name allowlists, execution site, process selection, no self-registration grants, bounded cancellation, hook owner teardown and poison-on-ambiguous external effect. External program/tool output is data and may never register a trusted host hook or alter credential destinations via deserialization.

## 8. Security tests and failure proofs

A plan is not complete without adverse cases. Each line is an **acceptance requirement**, not a claimed passing test.

| Test ID | Must demonstrate |
|---|---|
| HP-01 | Exactly two hook verbs; observing event/lifecycle cannot mutate authoritative values |
| HP-02 | Stable stage/registration order and snapshot isolation across register/unregister |
| HP-03 | Hook reentry, cyclic branch and depth/size/time limits terminate deterministically |
| HP-04 | Defer token is single-use, generation/owner/epoch/deadline bound; delayed callback after cancel rejected |
| HP-05 | Tool prehook modified args get complete revalidation; invalid mutation denied before intent/effect |
| HP-06 | `NO_HOOKS` and missing extension cannot disable mandatory ToolPolicy/journal/revocation |
| HP-07 | Host/MCP/extension/management tools all require exact actor/site/source/epoch/effect grant |
| HP-08 | Remove legacy host-name-only grant path; same-name MCP tool gets no host grant |
| HP-09 | Child ROOT ∩ parent ∩ requested Envelope never widens; parent revocation disables awaited child effect |
| HP-10 | Reviewer malformed/timeout/off-schema/widening output fails closed; human approval cannot bypass hard deny |
| HP-11 | Approval replay/cross-session/other actor/site/source epoch/changed args denied |
| HP-12 | Hook/model reviewer cannot change payer/account/model/endpoint/credential without independent host grant |
| HP-13 | Paid reviewer invocation separately journaled; lost result not reissued automatically |
| HP-14 | Tool intent acknowledged before effect, receipt before next model; failure preserves unknown hazard |
| HP-15 | Source-signed private reasoning absent from hook-visible public text and journal export |
| HP-16 | Process kill/freeze/PTY unsupported do not falsely report termination or confinement |
| HP-17 | Per-step status notices are not assistant messages and render consistently in native/JSONL projection |
| HP-18 | Project setting/prompt/theme/resource revocation rechecked after awaits and never creates tool grant |
| HP-19 | E3/E4/E5 same kernel transition trace still holds; hosted functionality returns Unavailable when unsupported |
| HP-20 | No implicit startup process/MCP/reviewer/voice/provider call and no added tool visibility without opt-in |

## 9. Critical rejected designs

1. A new independent policy/approval/router Agent loop, proxy or HTTP middleware: extra authority and paid attempts.
2. Generic JSON-typed mutation at every stage, including immutable event/lifecycle and tool receipts.
3. Default `RW/workspace/auto` as an actual tool grant; source discovery ≠ authorization.
4. A model reviewer dictating a multi-effect action list without host admission.
5. A hook bypass flag disabling security, journal or accounting.
6. Automatic re-execution after ambiguous transport, paid model stream, process kill or tool effect.
7. Treating process group, cleared environment, an extension subprocess or path sanitizer as a verified OS sandbox.
8. Dynamic per-feature tools/RPC/host methods for every signal or process operation; use a small op vocabulary over the four primitives.
9. Treating P6's JSONL session records or actor PID as equivalent to aw's durable attempt/Session/writer epoch.
10. New historical phases, new migration support, compatibility aliases, premature native certification or embedded firmware work.

## 10. Architecture traceability

See [current aw architecture](ARCHITECTURE.md), [TOOLS](TOOLS.md), [SDK](SDK.md), [UI-EXTENSIONS](UI-EXTENSIONS.md), [TERMINALS](TERMINALS.md), [CLI](CLI.md), [STATE](STATE.md), [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md). Relevant current source:
- `crates/aw-agent-kernel/src/capability.rs` and `turn.rs` — shared exact grant and paid/effect state machine.
- `crates/aw-agent-core/src/{tool,extension,agent,journal,planner,reviewer}.rs` — current canonical host permissions, hook seam, Agent and governance review.
- `crates/aw-tools/src/shell.rs` / `aw-terminal/src/lib.rs` / `aw-platform-unix` / `aw-platform-win` — existing OS process/PTY contracts.
- `crates/aw-extensions/src/lib.rs`, `aw-mcp` — explicit external capability source and process hooks.
- `crates/aw-ui-extension/src/lib.rs`, `src/tui`, `src/web` — presentation-only slots and one shared reducer.
- `crates/aw-store/src/journal.rs`, `crates/aw-store-jsonl/src/lib.rs` — durable receipts; no new side-car event store.

The canonical implementation plan is [Runtime contract](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md). Update this document and the base architecture/SDK/TOOLS/CLI/UI references with **as-built, verified** changes at every accepted wave, rather than claiming a proposed API exists.

---

## As-built AW-P6-5A: hook and effect authorization constraints (2026-10-11)

The earlier host-only `Next|Defer` hook chain remains the **only ActorMessage hook service**: `Input` edits reenter its typed stage; `Control::Cancel(Turn)` cannot be swallowed by optional handlers; unsupported wide/privileged controls fail closed. The Agent's existing extension callbacks for provider/tool/summary/planner/reviewer are **not yet consolidated** into all ten stages, so do not claim AW-P6-6 implemented. The current safe-point gate is an internal **service port**, not a third primitive hook verb. It gates before provider dispatch and after a completed provider response before a fresh tool batch intent; no control intervenes between external effect start and durable factual result.

In AW-P6-5A, the single ToolPolicy plus narrowed Envelope is rechecked at *all* Agent-visible tool decisions, including later source-liveness transitions: registered Host/MCP/Extension capability, actor and exact source epoch/effect/site must remain authorized even if a previously unavailable tool becomes available after initial preflight. No project text, reviewer output, client Actor ID, source metadata or UI event grants missing authority. Explicit cancellation during a possible external effect preserves reconciliation; a cancelled provider stream retains its factual paid-attempt record and is never silently auto-retried. Pending queue acknowledgement remains in-memory until separately persisted under an owning writer with CAS/epoch fences.

---

## As-built AW-P6-6A — typed extension hooks and immutable effect boundaries (2026-10-11)

In actual source, `Extension` now publishes exactly one `on_hook(ctx, HookEvent)->HookFuture<HookFlow<HookEvent>>` and `name()`; old per-operation Rust methods were **removed**, not aliased. The hosted `HookEvent` sum type is an *untrusted transformation view* with variants `Prompt`, `ToolCall`, `ToolResult`, `Step{kind,phase,evidence}` and `Observation{stage,label}`. It uses the shared kernel ten-stage enum and only `Next/Defer` outcomes. Dispatch lives in `aw-agent-core/src/hooks.rs` for both Actor (generation-fenced synchronous Message hooks) and Agent/auxiliary (ordered async typed hook handlers). The Agent and the original standalone summary/planner/guardian/router reuse the **same** existing paid/effect journals and strict output publication checks; no new governance model executor exists.

- **Prompt**: same user input and all previously prepared context messages are immutable; an explicitly selected trusted linked Rust extension may append only bounded System-prefix contributions to the provider projection, never rewrite a prior user ToolCall or private signed provider continuation. All resulting context budgets and message/correlation schemas are revalidated.
- **ToolCall**: original complete Assistant model proposal is already journaled. `Next(Some)` must preserve the exact call/effect/spec; `Next(None)` denies before any effect; unowned `Defer` is rejected. A safe future argument rewrite requires a distinct journaled call-revision and fresh exact ToolPolicy + Envelope authorization, not mutation of the original receipt.
- **ToolResult**: the actual ToolRecord/journal receipt (including output/error/truncation) is immutable, whereas only `visible.content` may be transformed/redacted before next model disclosure under the existing fair per-tool context/output limit. Original call ID/status remain paired. Post-effect denial cannot erase factual OS effects, cannot justify replay, and prevents further model calls.
- **Step**: existing planner/reviewer/routing/summary auxiliaries expose restricted typed `DecisionKind` and `Before/After` with immutable proposal/evidence. Hooks may narrow/deny with `Next(None)`; no automatic decision-generated tool/process/account authority.
- **Observation**: Turn, Message, Output, Event and Lifecycle have bounded public-safe **labels**; hook faults/timeout/veto/mutations are counted by `Agent::hook_observer_failures`, not printed to terminal and not used to alter factual journals. Synchronous Actor Runtime maintains stage-typed Input and Signal (plus event projections). Bounded handler count and order, stage-interest (`wants_stage`) optimization with panic containment, one-shot Runtime continuation, recursion max depth 8, per-handler and total chain time limit apply.
- **External process hooks**: explicitly selected `aw-extensions` strict JSONL **ABI v2 only**, with `prompt`, `tool.call`, `tool.result`; old v1 header and wire names are absent from executable sources. Rust/C v2 tools never receive host credential grants merely from advertised manifests. The user's OS process authority is still **not** a sandbox; same ToolPolicy and intent-before-effect journal remain mandatory.

**Scope warning:** AW-P6-6A is a successful native implementation slice, **not full AW-P6-6**. Async `Defer` does not yet reach the owning Runtime's one-shot continuation from a standalone Agent, and pre-tool Next(None) currently stops the Turn instead of generating a corresponding model-visible denied ToolResult for every provider batch entry. These are recorded explicitly as AW-P6-6B with the required journal/model-order contracts in the master appendix. Do not add a compatibility shim or mark the stage hooks as a second provider/tool engine. Actual native and embedded evidence is captured there.

---

## As-built AW-P6-6B — faithful Next/Defer and host-journaled tool proposal control (2026-10-11)

The typed Agent/Actor `hooks.rs` service now supports *owned* async `HookFlow::Defer` by using the current authenticated `HostRuntime` Actor Session and writer epoch. A standalone Agent/auxiliary executor has no minting authority and denies unowned Defer. An explicitly owned `Continuation` is a generation/Envelope-revision/Turn-bounded one-shot mailbox reservation, not persisted provider-private continuation data. The host can resolve only `Next(Some/None)` through `AgentSessionController::resolve_hook`, and cancellation/dropped futures clear the parked reservation. This leaves Runtime, Actor, Message and Envelope as the four primitives and Next/Defer as the only hook verbs.

Every provider tool-call batch is still validated by the same kernel TurnMachine, current ToolPolicy and narrow-only Actor Envelope. **Complete-batch denial** is a separate no-effect `JournalEntry::ToolDenied` for *every* original call ID, plus one corresponding bounded model-visible Tool-role error per ID; it is never an executed tool intent. An **argument revision** is *not* an authority mutation: a linked Rust hook may propose changed argument data, but only an independently installed host `ToolRevisionPolicy` can authorize that exact original/revised pair, and an immutable `ToolRevised` journal entry must be acknowledged before any real `ToolStarted`. Host policy and exact tool source/effect/actor/site/Envelope are rechecked before dispatch and result publication; host panics deny safely. Neither path grants access by name, silently replays a paid call, or changes original Assistant/provider-signed thinking evidence.

SQLite v33 and JSONL v1 validate original Assistant correlation, unique denial/revision, exact revised `ToolStarted`, exact factual `ToolFinished` and no effect hazard for a denied call. The original full/minimal/stateless Agent loop and E3–E5 portable executions stay intact. Detailed native evidence, tests and *still-open authenticated registration and persistent owner/RPC integration* are in the master AW-P6-6B as-built appendix. No general process sandbox, queued-control persistence, firmware E6 or multi-host release is claimed.

---

## As-built AW-P6-7A — child Actor/decider hook authority boundary (2026-10-11)

The optional bound child service uses the same `HostRuntime` Actor Message (`Input`, `Signal`, child-to-parent `Event`) stages and current narrow-only `Envelope` with no new hook verb. It authenticates the parent Session, exact durable child `SubagentInfo`, selected account/payer/executor identity and child task SHA256 *before* a paid model call. Only original audited local Host read/write/edit/delegation tools can appear in the narrowed child grant set; a model, MCP hint or extension manifest cannot widen child authority by making tool metadata visible. Notifications contain only bounded factual status and require consumption before identity release; they are not root transcript output. Cooperative `Freeze` is NOT OS freeze and cannot interrupt an external effect between intent and receipt.

The original planner/router/guardian/summary services continue using typed `HookEvent::Step` and their own authorized model budget/intents; AW-P6-7A does **not** claim they now share a new persistent Actor registry or that review decisions are executable authority. Full cross-site worktree Actor delegation, durable child control admission/CAS, and unified reviewer approvals remain open AW-P6-7B/9 work.

---

## As-built AW-P6-7B: parent-Actor Step admissibility is not a new grant (2026-10-11)

AW-P6-7B adds a host-only Step-admission check for existing planner, guardian and classified routing services. The current `ToolContext` must identify a live, writable, actor-owned SQLite parent Session branch with an active Turn; the currently selected independent reviewer/classifier account must revalidate its own actor/principal/payer/credential source; and the owning Actor's Envelope execution site and lifecycle must match that context. A forged sibling Session, stale branch, cancelled context or different site is rejected *before* the existing classifier/guardian paid attempt. The approved Step is still the same `HookEvent::Step` stage; it is NOT permission to run tools or publish a reviewed result. Actual routing candidate constraints, review source hashes, approval gating, root-only publication, durable receipts and attempt/budget counters remain in the original SessionStore and existing Agent executor.

For an opted-in Git worktree child, `SiteTransferBinding` is minted only after the durable execution binding and canonical child tool root are verified. Transferring the existing exact host-approved read/write/edit/subagent capability subset to a new `subagent:<id>` site does not transfer unrelated file, network, Bash, MCP, extension or model/provider credential authority. The new child Envelope and original ToolPolicy are rechecked before effect dispatch and result disclosure. Cross-machine delegation, trust negotiation and durable owner/queue/RPC/approval state remain explicitly out of scope; see the AW-P6 master appendix.

---

## As-built AW-P6-9A — owner/writer gate is mandatory independently of optional hooks (2026-10-11)

The existing typed `AgentSessionController` can now install a trusted native `WriterEpochFence` bound to the REAL current SQLite SessionStore Workspace writer epoch. It checks before Agent binding, the exact claimed Input/Turn, each model and tool-batch boundary, exact tool admission and post-effect disclosure, one-shot Hook Defer reservation/verification and after the returned paid Turn. A prompt, extension or remote client cannot provide this guard as a mutable JSON permission or skip it with NO_HOOKS. An old owner retaining a valid ActorKey loses authority immediately if the SQLite writer epoch moves. If a provider request was already paid when the writer disappeared, the ClaimGuard conservatively remains Unknown; no second model request or OS effect can be automatically triggered.

SQLite admission facts are separately required to match the actual claimed Actor Input's source/target generation, correlation ID, priority, Turn and SHA256. A queued `Accepted` audit event never equals a `ToolStarted`/provider receipt; only the existing factual run/hazard journal may settle it. Pending edit cannot launder changed text past the admission gate; crash/restart facts are read-only inspection obligations, not actor/control grants. The public durable-feature Rust SDK remains host-only; unprivileged browser/R12 RPC, multi-message payload persistence, durable approvals, and external process control are intentionally NOT exposed in AW-P6-9A. See the newest master-plan as-built appendix and explicit AW-P6-9B gates.

---

## As-built AW-P6-9B1 — Control is a writer-owned Signal, not a new hook verb (2026-10-11)

A host-only `ActorControlAction` is limited to `CancelTurn`, cooperative `Freeze/ResumeTurn`, and cooperative `Freeze/ResumeActor`. Before the SAME Runtime Signal hook pipeline processes a Control, the actual SessionStore appends an immutable SQLite v33 `actor.control Prepared` fact tied to the **currently Dispatching** original Input, live original owner/writer epoch, exact Turn, sender/target Actor generation and current owner process incarnation. After the actual Runtime `Delivery::Applied`, it appends `Applied`; `Consumed` is Rejected, ambiguous/deferred sends are Unknown and require explicit inspection. The same sender sequence ledger is shared with ordinary Input. Unknown or crashed control is never replayed or accepted as OS Process completion. The existing `AgentSessionController::WriterEpochFence` now checks synchronous send before hooks/admission; no client string, rendered label, optional hook or stored audit fact can grant source/actor authority.

Only the trusted durable-feature Rust SDK exposes the typed operation values. Standard R12 stdio rejects `actor_query`/`actor_op` as unsupported until an authenticated, writer-epoch/CAS-fenced owner driver is explicitly installed; no unauthenticated shell/Process/Approval escape exists. `NO_HOOKS` or an observer cannot disable the mandatory writer/Session/ToolPolicy/Journal gates. This is **not** a persisted multi-message queue nor a crash-resumable pending Control system. See the master AW-P6-9B1 appendix for negative and native proof.

---

## As-built AW-P6-9B2A — immutable queue facts cannot confer hook/tool authority (2026-10-11)

A current host/writer/Session/Actor-Envelope-bound Input queue now persists only encrypted private text custody plus immutable SQLite v33 queue receipt metadata, not private continuation, provider account, source grant, review approval or runnable OS instruction. A new owner may rebind an **unclaimed Pending** Input only after independent exact owner/writer/site/Actor/ToolPolicy checks; changed writer epochs fail closed. `Claimed` is an inspection/Unknown obligation after process loss. A real `AgentSessionController::send` must first accept the current Actor Message under the same Input-hook pipeline before 9A/queue Accepted receipts are committed. Neither Next/Defer nor optional observers can suppress the mandatory original 9A claimed-message hash/Turn check, pre-effect source-policy gates or original factual TurnJournal. A queue `Accepted` status is not paid, OS, approval or policy evidence, and an unknown provider/tool attempt is NEVER retried on restart.

Host-authored queue reads project `ActorQueueView`, redacting encrypted-item references, raw hash, process incarnation and text. The corresponding owner-verified store API remains internal Rust and unavailable to unbound R12 stdio, remote browser or automatically advertised management tools. E3–E5 and minimal/stateless cuts never gain SQLite queue activation. Native adverse tests (stale writer, lost key, tampered ciphertext, duplicate IDs, source sequence, capacity, restart, in-flight cancellation, no replay) and remaining custody/orphan and multi-class queue work are recorded in [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9B2B observational-only Notice and Hook trust boundary (2026-10-11)

`actor.notice` is only a **read-only, authenticated child status observation** with encrypted private ciphertext and immutable Session-writer-fenced delivery/read dispositions. A signed or stored Notice fact does not grant any SourceKind/effect, ToolPolicy bypass, Control success, approval/guardian decision, or Hook `Next`/`Defer` continuation. `Prepared` and `Unknown` can never become automatically redelivered messages after restart. Only an existing, current child Actor via original Runtime can yield `Delivery::Queued` for same-process Accepted; encrypted reading after restart is data retrieval, NOT re-execution. Existing owned async Defer remains memory-bound, generation+writer+Envelope fenced and non-replayable; effectful Control/Hook persistence is expressly out of this wave. `actor.custody` never follows unknown orphan file references or deletes unrelated provider credentials. See [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9C1 local stdio R12 trust/Hook limitations (2026-10-11)

Only an **explicit original Host-local process** with an already verified own SQLite Session and `--agent-actor-owner` receives Actor methods in the R12 `hello`. No raw JSON ActorKey/Session owner writer epoch, process ID, principal string, `tools_mode`, source/effect grant or tool name is authority. `ActorOperation::from_rpc_value` enforces exact fields even on Serde internally tagged unit variants. The original persisted WriterEpochFence, Actor generation/current state, per-site narrowed Envelope and original ToolPolicy continue to gate delivery. No external active `Cancel/Freeze/Resume` is exposed because serial stdio cannot prove an actively concurrent controlled Turn; `Next/Defer` Hook execution, external Process controls and ToolPolicy/source grants are **not** expanded. An interrupted original Agent run (provider stream, SIGKILL, cancellation, failed output pipe) remains an immutable Unknown/inspection obligation and must not be auto-retried. Public metadata strips private ciphertext/process markers, Notice private reading is deliberately separately named and only for the current owning Session. See [accepted history](../plans/accepted-history/TODO.md); remote user/principal authentication, grant revocation, full approval/guardian/service registration and security audit remain OPEN.

---

## AW-P6-9C2 remote Actor trust, revocation, Control and Hook limits (2026-10-11)

New **signed** AwNet MachineLink `ActorQuery/Inspect/Enqueue/Step/Control/NoticeRead` capabilities never imply a Process/PTY/File/Git/MCP grant, paid account selection, approval/guardian, or Hook `Next`/`Defer` permission. The original owner separately appends a narrow, expiring, revocable SQLite `actor.peer_grant` fact binding one authenticated paired Ed25519 peer key + security epoch to an exact current Session/branch/site/writer and operation family. Every incoming request is rechecked, `Step` reevaluates revocation and original Actor generation while the provider runs, and private replies are checked before disclosure. Revocation is possible **while the exclusive owner process is active** using the current owner-local bounded stdin command; signed peer QUIC cannot invoke it. The canonical local `actor_op` current-Turn `Control` (including Cancel) is applied only after the existing `Service::control` proves original active Input/Turn, writer and Actor generation; an `Applied` control fact is **not** model/vendor charge settlement. No new Hook registry, durable Defer replay, portable remote human/OIDC claim, automatic permission migration or generic remote shell is introduced. The real 2-node signed transport and independent concurrent paid-req revoke tests are recorded in [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-8A extension settings / prompt source security (2026-10-11)

Registering a typed setting or host-approved prompt contribution does **NOT** register a Tool, authorize a Model/Account, grant Process/Filesystem/Network privileges, change `Envelope`, bypass `ToolPolicy`, approve a guardian, or install a new Hook verb. Manifest settings are accepted under only the manifest's exact extension ID; forged external IDs, stale config CAS and invalid types/bounds fail closed. User TOML is data only; ordinary config/project/RPC SET/GET/RESET of Secret settings are expressly denied, not silently stored in plaintext. Project values are rescanned with descriptor-relative no-follow custody and checked against *current* pinned source hashes; revoked/changed project data remains denied even when a CLI override would mask it. Project markdown prompt contributions are quoted reference data only, cannot Replace/alter Rules, Tools or All anchors, and are subject to deterministic bounded ordering. Host text contributions require direct trusted Rust host registration; generated prompt bytes are consumed at **initial Agent construction only**, with one original journal and no runtime paid retry. A separate authenticated SecretStore adapter, status/event regions, consent-gated media and approval flows remain future 8B/8C. See [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-8B — encrypted extension-setting confidentiality and v2 event trust (2026-10-11)

Extension setting definitions are inert registration data. An explicit host-only secret SET/READ/RESET now requires **both** the registered current `SettingKind::Secret` bounds and original current 9C `Service::verify_rpc_state()` (real Session owner, writer epoch and Actor generation/Envelope). The SQLite v33 fact commits after authenticated write-once ChaCha20-Poly1305 ciphertext sync; subsequent rotations/deletions are auditable CAS facts, never a plaintext config variable, URL, browser/RPC projection or provider credential grant. Returned secret bytes use zeroizing non-`Debug`/non-`Serialize` buffer; public view/list is REDACTED and secret owner access is denied after the original Actor settles. A ledger-proven retired-only sweep cannot claim physical overwrite, arbitrary power-loss certification or removal of unreferenced orphan blobs.

Protocol v2 `UiExtension::event` is a presentation-only typed factual notification with bounded semantic spans, per-Session monotonic EventRail and control+bidi/size sanitation. The v1 original `project` and TUI performance extension contract remain valid; no Hook verb is added. Original `AgentEvent::InvocationFinished/Finished` is converted to metadata-only `StepEnd/TurnEnd/FinalOutput`; no model/Tool output or private provider signature is automatically disclosed, no unknown/cancelled attempt fabricates FinalOutput, no new durable queue or paid replay is introduced. New opt-in status regions use existing config CAS/terminal span sanitation and cause zero layout changes for original defaults. [accepted history](../plans/accepted-history/TODO.md). Media/voice consent, human approval/guardian, actual remote user/OIDC identity and release certification remain 8C/future OPEN.

---

## AW-P6-8C media consent OFF, human authority and factual Activity rail (2026-10-11)

A caller-controlled `ActorMessage.content_type` cannot grant media, Tool, Process or Hook access: the original portable mailbox rejects nontext before an Input or Control is admitted. `aw-tools::media_port` frames are DATA only on already host-approved/journaled Process stdin/stdout; opaque binary/image data can never activate a device. Microphone, Speaker and VideoCapture kinds remain hard-denied without exceptions (and no realtime provider exists). Explicit positive owner/user/Session/device consent, TLS backend authentication, private retention and secure deletion, process isolation, audio logging safeguards, revocation and independent native device/paid-provider certification are prerequisites of *future* activation, not implied by this source code.

Existing Phase50 guardian review remains a *separately authenticated initiating human* workflow. The local `aw::approval_ui::pending/inspect/decide` uses live original 9C Session/Actor generation/writer plus exact immutable evidence SHA; the existing Store checks user identity/waiting state, and original `review_release` separately rechecks original parent invocation/source digest. The new canonical `aw review pending SESSION BRANCH` is bounded **read only**; it does not authorize via browser cookies, human impersonation or model text, and does not add a second approval DB. Opt-in `show_event_notices` uses only factual, sanitized, ephemeral v2 presentational lines in existing Workspace Activity, never an executable Hook `Defer`, Agent transcript Message, paid provider success or default extra chrome. Read [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D1 — source-grant/ToolPolicy gates on read-only model Tool projections (2026-10-12)

Merely naming a model-visible `agent`/`queue`/`process` tool in an external schema, extension, project prompt or model reply never grants it. Trusted `aw::model_management::install` explicitly verifies the SAME current journaled original Agent, real existing Session owner/writer/Runtime generation and **every** pre-minted exact Envelope Host source+epoch1/Read grant before adding an original `ToolRegistry`/`ToolPolicy` entry; it is OFF by default and not a general RPC or user-controlled source grant. The original `ToolPolicy`/`AgentStepGate` independently rechecks all name/source/epoch/effect/site permissions before execution and disclosure. Its true ToolContext MUST be the exact currently immutable `Dispatching` original Input plus current Runtime active positive Turn, same Session+branch, site, full selected account/payer/credential identity and uncancelled source. Private encrypted queue bodies, stored ciphertext refs, Process PIDs, commands, roots, stdout/stderr text/bytes, credentials and full native guardian decisions are never projected to the model: only bounded factual Read metadata. The optional Process map contains host-registered opaque handles to the ORIGINAL already independently authorized/journaled `PipeProcess` (source custody is a trusted host obligation; this reader does not manufacture an effect receipt). No effectful `queue.enqueue`/`agent.cancel`/`process.kill`/`review.approve`, payment, ToolPolicy mutation, child spawn, OS signaling, or live media is exposed; these remain a separate reviewed D2 and production-audit boundary. A failed/drop/revoked/Unknown original Actor claim is not replayable or observable via a cached ToolContext. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2A — independently reviewed Host Write queue grant (2026-10-12)

An opt-in model-visible `queue_enqueue` Tool requires its OWN exact **Host/trusted-host/epoch1/**`Write`** grant in the ORIGINAL Actor Envelope **before binding the Agent**, plus the independent exact `ToolPolicy::permit_source` Write source/site grant created by the current trusted local host. D1 `agent/queue/process` **Read** grants or project/model/tool schema suggestions can never promote it to Write; the existing `AgentStepGate` rechecks current original owner/writer/Actor/Session/site and tool origin/effect before model advertisement, each dispatch, and post-result disclosure. Every actual Write requires an active original immutable SQLite `actor.input Dispatching` and matching Runtime Turn, original ToolContext selected payer/provider identity and an uncancelled Tool. The host mints one deterministic `mq_` request ID from the actual Session+branch+Turn; inside original SQLite queue's BEGIN IMMEDIATE the writer mints monotonic source sequence across all original Input/Control/Queue/Notify facts. The accepted future Input is encrypted in original SecretStore, its bounded queue metadata is redacted, and ONLY explicit current host owner claim may ever run the next Agent Turn. Duplicate attempts cannot create another encrypted Pending; an uncertain second Write may leave original Tool/Turn hazard requiring explicit inspection instead of false settlement/replay. The *original Agent journal* truthfully records model ToolCall proposed `text` as invocation evidence; do not claim that text is absent from all SQLite files, only from the queue-specific immutable audit. No model `process.spawn/kill`, `agent.cancel`, `queue.update/delete`, `review.approve`, remote Human identity or microphone/voice is enabled by this grant. D2B and production security are OPEN. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B1 — separately exact-granted model queue UPDATE/DELETE with source provenance (2026-10-12)

Original Actor/Envelope Host source epoch1/**Write** and original Agent ToolPolicy independently permit each explicitly trusted installed `queue_update` or `queue_delete` name. A prior D2A `queue_enqueue` Write, any D1 `queue` Read, model request, extension/MCP/remote SourceKind, forged original ActorKey/writer/priority/target or a model-provided credential does **NOT** grant a queue disposition. Original `Service::verify_model_observation` requires the one current immutable `Dispatching` original Input, actual Runtime active Turn, current original owner/writer/Session/branch/site/Actor generation, uncancelled ToolContext and selected account/payer; source/effect are rechecked before/after each model Tool dispatch and disclosure. A queue target must be *actually derived* by the shared D2A model hash from a **previous Settled original Agent Input Turn**, same sender/branch/site, not just begin with `mq_`. It must be Pending with exact expected revision; no claimed/accepted user or unknown-source mutation. Existing SQLite v33 BEGIN IMMEDIATE atomically appends both encrypted queue state revision and once-per-current-origin-Turn immutable `actor.model.queue_disposition` marker; a second contradictory Update/Delete cannot commit or silently replay after crash. Private content is kept out of queue/disposition audit; original Agent's immutable model ToolCall proposal text remains recorded as evidence (not whole-database secrecy). A delete is a queue disposition, not a human guardian approval or future paid success. An uncertain Write/retired-ciphertext unlink leaves original Tool/Turn Unknown or inspection-required and cannot auto-replay. This does not add new Hook verbs, an execution loop, Process/OS freeze/signal, child Agent control or model-origin human approvals. See [accepted history](../plans/accepted-history/TODO.md); further D2B2 effectful Agent/Process/human flows require an independently designed original grant/intent/receipt.

---

## AW-P6-9D2B2A — original Owner/Host Write for one preexisting Process stdin close (2026-10-12)

Model Process control does NOT flow from naming a Tool, model/Plugin/project text, D1 `process` Read, any queue Write, MCP/peer Site or a forged PID. Exact original `Host/trusted-host/epoch1/Write` `process_close_stdin` pregrant in the current Actor Envelope plus the same original ToolPolicy/site/source/effect/selected payer and live original SQLite writer/Session+branch/`actor.input Dispatching` + Runtime active Turn are required BEFORE executing any I/O, and rechecked before/after. The Process is a separately trusted host-registered original `PipeProcess` referenced only by opaque handle; its custody grant is not created by this model Tool. The model can only close stdin and cannot supply bytes, kill, signal, freeze or spawn. Original SQLite v33 `actor.model.process` records domain-separated stable one-per-origin-Turn Prepared intent **before** effects and same exact owner-Process-incarnation terminal Applied/Rejected/Unknown receipt; restart may inspect only, and Unknown original Tool/Turn payment hazards remain separate, no replay. `Applied` is NOT Process child exit/paid effect/peer receipt. Model payload excludes PID/program/argv/env/stdout/stderr and no new Hook/Message/billing executor or schema is created. Model-origin Agent self-cancel/freeze/child and Process arbitrary stdin/kill/termination, as well as authenticated human guardian approval, remain separately unauthorized D2B2B actions requiring original current Actor/effect/human source policy and immutable no-replay receipts; never use raw `actor_op` as a model privilege shortcut. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B2B1 — original SourceKind/Host Write and irreversible Process termination guard (2026-10-12)

A model Tool name, Process Registry opaque handle, existing Process observation Read or stdin-close Write grant is NOT authorization to terminate an OS Process. A trusted owning host must separately approve the original Process for termination and register its live pointer using `register_terminable_owned`, pinned by a random 32-byte private per-Process incarnation; default `register_owned` remains nonterminable. Current originally immutable Runtime Actor Envelope exact Host/trusted-host/epoch1/**Write** `process_terminate` permission and SAME original ToolPolicy/site/source/effect selected provider account/payer are independently rechecked before/after each model Tool effect. Its JSON is exactly `{op:terminate,handle:opaque}`, never model PID/ActorKey, OS signal/command/environment/source credentials or human approval. The original actual immutable SQLite `actor.input Dispatching` and active original Turn must match owner/Session/branch/WriterEpoch/Actor generation and unmigrated payer. Existing SQLite v33 `actor.model.process` commits one deterministic per-originating-Turn Prepared **before** signaling, and the original Process sets `termination_attempted` before group teardown to deny repeated signals. Terminal Applied is written only if the original bounded group-stop returned Cancelled and drained direct-child output, from the very same owner Process incarnation; otherwise Unknown/Prepared inspection only, no replay or false OS exit/payment certificate. Applied does NOT prove all descendants stopped or full process-group security. Duplicate original model Write ToolCalls may mark the original Tool/Turn Unknown; never automatically repeat paid/model/Process work. Realtime voice/device and model-origin self Agent cancel, child/PTY arbitrary Process IO/kill, guardian human approval/OIDC remain strictly separately scoped. No raw model `actor_op`, another runtime or third Hook verb. [accepted history](../plans/accepted-history/TODO.md).

---

## AW-P6-9D2B2B2 — model Write source cannot cancel its own paid Turn; owner-only future resume (2026-10-12)

In-flight `CancelTurn`/`FreezeActor` from the currently billed model Tool could destroy the original ToolStarted/ToolFinished or provider settlement receipt, so it remains **FORBIDDEN** as model-origin Control. The only new original Agent Tool `agent_pause_after_turn` requires preexisting exact current original Actor Envelope Host/trusted-host/epoch1/**Write** and matching original `ToolPolicy` name/source/effect/site, original current Runtime active positive Turn, factual immutable `actor.input Dispatching`, current owner/WriterEpoch/Actor generation/Session+branch and chosen provider/payer; no model JSON can supply another principal/Actor/Turn or human approval. It commits one immutable `actor.model.future_pause Armed` v33 audit (bounded stable per-originating-Turn ID) and lets the current provider/Tool finish normally. A bounded original Store admission guard is invoked inside BOTH the original Input and queue Claim writer transactions, before any future acceptance/decryption/claim. An Unknown source paid/Tool Turn must remain held even after restart; an `Inspected` paid action does not substitute for `Settled`. An **authenticated current owning Host**, not the model, must explicitly release with a printable note and verify the exact originating Turn Settled, no active Run or unresolved independent Input hazards. A release is an original audited `Released` fact, not an OS/remote Actor Control/guardian approval; duplicate releases and model self-resume deny. No generic model actor_op, paid replay, child/remote cancellation, arbitrary OS process signals, OIDC/user approval or new Hook/Message/Engine. See [accepted history](../plans/accepted-history/TODO.md).
