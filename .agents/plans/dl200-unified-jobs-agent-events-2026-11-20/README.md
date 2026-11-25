# DL-200 — unified Agents, Jobs and Events

Author: @pvl
Co-authors: OpenAI GPT-5.6 Sol
Owner: @pvl

**R6 SEALED (2026-11-21), INCLUDING THE SEALED SHARED PROCESS/RELEASE-CORRECTION AMENDMENT / IMPLEMENTATION PROMOTED / PHASE 0 CORRECTIVE GATE PENDING.**
R6 was explicitly approved for implementation checkpointing on 2026-11-21 after the
final live-owner review. It keeps the public model/API/phases unchanged and tightens the
internal ownership boundaries below. Commit/push/release/deploy remain separately gated.

DL-200 succeeds the sealed DL-150 tool/runtime contracts. Its public resource model and
seven-phase implementation sequence stay unchanged. R6 **removes repeated internal
mechanics**; it does not add public resources, APIs, a new authority service or a global
distributed transaction layer.

## Developer mental model

**One authoritative source for each semantic truth. Mutable truth has one Owner and one
linearization point; immutable truth has one stable Fact identity; uncertain external work
has one Operation custody.** Global names identify things; **consistency and total order
belong to one authoritative store**. Everything derived is a capability, projection,
bounded cursor or adapter—not competing truth.

| Public concept | What it owns or represents |
|---|---|
| **Agent** | Globally named collaboration/security envelope, history continuation and execution |
| **Job** | Agent, Process or Graph work; Nodes belong to Graphs and Turns are an Agent timeline |
| **Sandbox** | Durable logical mutable execution environment and named SourceBindings |
| **Event** | Immutable source-scoped fact, ordered in one local Event store |
| **Checkpoint** | Immutable history anchor plus exact captured named SourceSet (possibly empty) |
| **Extension** | Installed typed definitions, never their runtime authority or activation |

**Process Scope** is private: one dispatch lifetime. Sandbox answers *where/how* it may
run; Scope answers *which processes belong to that lifetime*. Neither owns the other.
Vault capabilities are separate grants, never Sandbox contents or checkpoint secrets.

The public namespace remains:

```text
/d:<domain>/i:<principal>/p:<project>
  ├── /a:<agent>[/t:<turn>[/j:<job>[/n:<node>]]]
  └── /s:<sandbox>
```

`i:` is one user-or-group principal namespace. SandboxRef is not an ExecutionRef.
The Event projection uses dot-separated canonical subject tokens, with reversible handle
encoding and NATS-style single-token / terminal wildcards:

```text
d:acme.i:alice.p:core.a:reviewer.e:changed   system Event
d:acme.i:alice.p:core.a:reviewer.e:#ready    custom Agent Event
```

A global ref never confers visibility, Tool authority, disclosure, cross-store cursor
ordering or an atomic multi-store commit. V1 cross-store Event→Agent delivery and
cross-store ordered Jobs/Event traversal are explicitly unsupported; a product such as
Histate composes stores through its own idempotent saga/outbox.

## Public API, kept small

Model Jobs uses exactly
`jobs_list / jobs_get / jobs_read / jobs_send / jobs_ack / jobs_cancel`.
`jobs_send` is directed Agent input or Process stdin/EOF; no queue/steer/interrupt
send-mode variants or Jobs-only subscription API.

Model Agent construction is `agent(task, from?)`, with one host
`agent.create(from=...)` lower constructor for fresh/exact-history forks.
`checkpoint.restore` uses that same constructor but additionally recreates the
checkpoint's exact SourceSet in a **new** Agent and Sandbox. History ancestry,
delegation lineage and Sandbox derivation remain distinct.

Events use `events_publish` (custom `#name`) and `events_subscribe`; host/SDK
exposes `events.read/subscribe/publish` and bounded subscription listing.
Event subscriptions send typed EventInput to an Agent's existing Input owner; trusted
EventHandlers consume the same log for product integrations. No Signals, topics or
parallel Event bus.

Host staged inputs use `list/add/edit/delete/send`; actual Send crosses the
Agent Input admission boundary. Agent ACL is allow-only and user/group-based.
Project remains a Workspace-backed view; there is no second Project database or
public AW Session API. Extensions expose typed `registry.tools()/views()/...`;
installed definitions never become grants without an explicit runtime binding.

## Six internal roles, no base classes

| Role | Truth/behavior | Example |
|---|---|---|
| **Owner** | Sole mutable authority and commit boundary | `agent_state_current`, Agent Input |
| **Fact** | Immutable record of what happened | HistorySpan, commit-log Event, ProcessTerminal |
| **Operation** | Durable replay/effect/recovery custody | Existing Prepared/Unknown claim |
| **Projection** | Rebuildable index/derived view | Event subject index, Job exposure |
| **Cursor** | Private bounded observation progress | Jobs high-water page, Event reader |
| **Scope** | One Process lifetime | Private child/containment owner |

Neither these role names nor their shared helpers become generic frameworks or new
persistence owners.

### Internal mechanics shared by real consumers

| Mechanic | Canonical rule | Must **not** become |
|---|---|---|
| **`StoreScope + StorePos`** | Existing `store_id` + `restore_generation` fence `commit_log.seq` positions; a backup restore stales prior cursors | Global ordering or new Store identity |
| **Replay / Prepared** | Original authenticated fingerprint, single effect, inspect Unknown, never blind retry | New replay ledger or policy-state replacement |
| **`ExternalEvidence`** | Store-local authority at commit; external immutable proof or held provider fence through acceptance; else Unknown | Generic authorization engine, saved live credential |
| **Projection publication** | Private generation, contiguous safe prefix, head verify/catch-up, atomic swap | Index becoming authoritative truth |
| **Cursor envelope** | Caller, selector, Store affinity, revision/placement and progress checks | Universal cursor traversal engine |
| **EventScan** | One source-first ordered scanner: candidate / proven skip / blocked / stale / end | Handler executor or Input scheduler |
| **`DriverKey`** | Stable driver namespace/local ID only; each persisted binding/anchor/recovery artifact carries its own schema version | Separate driver database or one coupled schema clock |
| **Registry staging** | Typed leaf definitions, local revision, immutable staged publication | Registry as policy/grant owner |
| **`aw-process`** | Dedicated Agent-neutral async Scope, child, pipe, wait/stop/drain and terminal owner for AW/dotlink; Linux launch crosses one private logical Spawn Facility with O(1) stable process-lifetime lanes (N=1 initially), while Processes remain O(n) movable async tasks | Two guards hidden behind a wrapper, thread-per-Process, or host-local launcher runtime |

Keep `rev` (Agent/Process/Graph owner), `event_rev` (source-local), security/group
generations, `inputs_rev` and output byte offsets separate from `StorePos`.
A cursor is not authority. EventScan is not final authorization: the Agent Input owner
rechecks subscription generation, current disclosure and causal budget atomically;
Handlers retain at-least-once delivery and their own effect receipts.

### Concrete crate ownership

```text
aw-tools                 host-neutral Jobs/Graph and ProcessTerminal/Occupancy facts
aw-platform              synchronous, target-specific OS Process-scope operations + strict
                         child-terminal facts/launch flags; no Tokio dependency
aw-process               dedicated Agent-neutral async Scope + process/pipe lifetime,
                         wait/stop/drain/settle; private Linux Spawn Facility with fixed/add-only
                         process-lifetime lanes (N=1 initially) + async bounded admission;
                         small bounded output-sink seam
aw-store                 existing StoreId + restore_generation, StorePos, replay/Agent/
                         Input owners, projection/cursor, EventScan and commit ordering
aw-tools-adapter / host  AW tool policy, authorized launch, output storage and runtime binding
dotlink                  aw-process/aw-tools consumer; MCP transport, output storage/logs,
                         launch/profile plus current-surface bounded-turn projection
                         (v0.17.8: 35m horizon, 10m report reserve, 25m command cap)
extension leaf owners    typed definitions + common staged registry publication
```

Dependency direction: `aw-process` depends only on minimal `aw-tools` facts,
`aw-platform` and async primitives—not AgentCore, MCP, SessionStore or another Process
owner. Hosts prepare an already-authorized command/environment and transfer that prepared
command **by value** into `aw-process`; the shared crate owns lifecycle-sensitive finalization
(containment setup, stdio ownership, spawn, child custody, wait/stop/reap and drain).
Host adapters retain launch authorization, executable/Sandbox preparation, output
storage/retention and bounded sink adapters. A blocked sink cannot hang cleanup or report
dropped output as complete. Linux `aw-process` never forks a prepared command from an arbitrary
Tokio worker: one private logical Spawn Facility owns a small fixed/add-only set of stable
process-lifetime OS-thread lanes and async bounded admission. Start with one lane. Only the short
spawn + exact `DirectScope` attachment occurs there; the resulting `Process: Send` remains a
lightweight O(n) async object whose wait/stdin/cancel/output work can migrate normally across Tokio
workers. Cancellation-safe RAII transfers `{Child, DirectScope, pid}` atomically enough that a
vanished receiver cannot orphan a child. Lanes never retire/shrink while the process lives; a
panic poisons future launch admission without killing the lifetime-anchor thread. N>1 is internal,
benchmark-driven only, never user configuration.

Dotlink v0.17.9 (`576d0a1`) tightens the same seam: the runner establishes one
absolute execution deadline when it enters Running; cancellation/expiry is rechecked on the Spawn
Lane immediately before fork; stale queued launches create no Process/terminal/kill fact; and a
real launched Process keeps that same deadline rather than receiving a fresh timeout after spawn.
Job age may include earlier queueing without consuming execution timeout. Pidfd/SIGCHLD readiness
is only a wake; cleanup waits for a strict non-reaping child terminal fact. Windows non-PTY managed
launch uses `CREATE_SUSPENDED | CREATE_NO_WINDOW`, then Job attach + resume, and its post-exit Job
accounting settle is async + cancel/deadline-preemptible so transient console infrastructure cannot
become false leftovers or delay an actual stop.

`aw-process` owns DL-200 **Process/Job execution** lifetimes. `aw-terminal` remains the
distinct Terminal/PTY custody resource because its reviewed source-release/adoption protocol
can intentionally transfer child/PTY custody; that is not a second Process Job runtime and
must not force release/disarm semantics into ordinary Process Scope. No second scheduler,
transaction coordinator, cross-store Event merger or parallel Source/registry authority.

## Implementation, no eighth phase

| Phase | Coherent R6 work |
|---|---|
| **0 / 0B** | Rename `Tree` → `Occupancy`; create `aw-process` and converge AW/dotlink on one async Process/pipe lifetime; apply the sealed v0.17.7 Spawn Facility/strict-terminal/stop-causality correction plus v0.17.9 stale-launch/absolute-deadline/interruptible-Windows-settle refinements; on the dotlink repin port v0.17.8+ surface policy as a dotlink projection over shared Job facts; reuse replay/effect/external-evidence helpers with Prepared before external effects |
| **1** | Authoritative Agent/ACL migration, existing StoreId + restore-generation scoped StorePos/cursor/projection helpers, immutable history and disclosure DAG |
| **2** | One EventScan + consumer-specific read/Input/Handler completion, Event projection, cursor and EventType leaf |
| **3** | DriverKey, recoverable capture/restore, Sandbox/Checkpoint/Vault owners and existing registry staging |
| **4** | Jobs/Agent public cutover using shared cursor/StorePos/projection helpers; keep owner-specific tree traversal |
| **5–6** | Compose typed leaf registries and dynamic Extensions; reuse the same Process owner |
| **7** | Remove legacy compatibility paths, sync final status/handoffs, grouped cross-repo qualification |

Existing R5 constraints continue to apply: no nested `batch` while an Agent is owned by
an active Graph node; same-store Event→Input admission; scoped causal budgets; disclosure
manifest revalidation; no arbitrary mount/env injection; strong containment fails closed
if required. No frozen 017 mount/config-v13 implementation is promoted here.

R5's implemented Phase-0A code still spells the Process field `Tree`. R6 proposes
`Occupancy::{Empty,Live,Unknown}` and `ProcessTerminal.occupancy` **in the source cutover**;
dotlink retains its existing `tree="leftovers"|"unknown"` wire projection unchanged.
Historical checks remain historical; the rename needs a fresh focused AW+dotlink gate.

## Normative files and next handoff

1. **`SPEC.md`** — normative contracts; section 0.1–0.3 is the single authority for shared
   internal roles/semantics. Detailed owner contracts remain in their existing sections.
2. **`IMPLEMENTATION.md`** — seven phases, implementation boundaries and grouped acceptance.
3. **`TODO.md`** — short current work/approval gates; its design-resolution checklist is history.
4. **`../../docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25.md`** — current
   comprehensive R6/Phase-1/2/3 context, exact chronology through 2b4fac3, owner map,
   historical design appendix and next implementation boundary.
5. **`../../docs/AW-DL200-PHASE3-NEXT-AGENT-2026-11-25.md`** — copy-ready next-agent prompt.
6. **`../../docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md`**,
   **`../../docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md`**,
   **`../../docs/AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md`** and
   **`../../docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md`** — historical implementation/design provenance.
7. **`.agents/docs/`** — dated R1–R5 review history and release evidence.
8. **Live source/tests** — outrank plans for claims of implemented behavior.

Historical references: `../dotlink-tool-port/reference/DL150-LIVE-CA89F959-README.md`,
`../../docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`,
`../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`, and
`../../docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`, and the
[archived R1–R5 design checklist](../../docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md).

**Design gate:** R6 is sealed and is now the normative DL-200 design. The prior separate
implementation promotion remains in force for Phase 0. Do not report any R6 source behavior
as implemented until the code and focused tests prove it. Work on L5 only, preserve unrelated
changes, do not inspect `.agents/plans/ux/**`, and do not commit/push/release/deploy without
separate user authorization.
