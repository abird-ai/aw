# AW + dotlink + DL-200 next-agent entrypoint — 2026-11-20

Use `dotlink---l5` only.

Continue in:

- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`
- read-only release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect or touch AW `.agents/plans/ux/**`.
Sandbox/mount/config-v13 implementation remains frozen unless explicitly promoted.
Keep implementation turns to roughly 30 minutes and stop at the next coherent checkpoint.
Do not commit, push, tag, release or deploy unless explicitly asked.

## Read first

1. `AW/AGENTS.md`
2. `AW/.agents/docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
3. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md`
4. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md`
5. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`
6. `AW/.agents/docs/CURRENT_HANDOFF.md`
7. `AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
8. AW 017 shared-runtime README/DESIGN/AUDIT/TODO
9. `dotlink/AGENTS.md`
10. `dotlink/.agents/docs/CURRENT_HANDOFF.md`
11. live source/tests for every owner touched.

The comprehensive DL-200 handoff embeds the complete prior canonical AW+dotlink
implementation handoff verbatim, so all earlier 0.17/runtime/replay/process chronology
and validation evidence remains available.

## Current published heads

```text
AW
f035a00031b8f5879f4d77f3fd14bfe02b89305c

dotlink
bbd29c13904b7255eb28e2ed6c776c3590ac122a

reusable AW implementation consumed by dotlink
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
```

All 0.17 D0-D7/shared-runtime/process-control/Remote-Start issuer work is published and
qualified. Do not reopen it without a real regression.

The current AW working tree now contains Phase 0A+0B implementation source changes. Shared
Process facts/direct platform observation and non-reaping settlement ordering are
implemented and focused-green; dotlink consumption/runtime-horizon projection is the exact
resume boundary.

## Current DL-200 state

Status: **R5 SEALED / IMPLEMENTATION PROMOTED / PHASE 0 IN PROGRESS** (R1 sealed 2026-11-20; R2-R5 amended/resealed 2026-11-21; R5 authoritative).

Implementation order authority:
`../plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md`.

The latest design pass added/changed the following and supersedes older DL-200 handoff
text where it conflicts:

### Public identity / Jobs

```text
/d:<domain>/i:<principal>/p:<project>
  ├── /a:<agent>/t:<turn>/j:<job>/n:<node>
  └── /s:<sandbox>
```

- `i:` is one user/group principal handle namespace.
- Agent/Turn/Job/Node is the concrete `ExecutionRef` branch.
- Sandbox is a Project-level sibling with a distinct typed `SandboxRef`; it never enters
  Jobs/control mutation grammar.
- Agent is the public AW security/history/collaboration/execution envelope.
- There is no public AW Session or `session.*` API.
- Internal SessionStore/session_id/branch/journal may remain for persistence/recovery.
- Model Jobs remain exactly:
  `jobs_list/jobs_get/jobs_read/jobs_send/jobs_ack/jobs_cancel`.

### One Agent constructor

Host:

```text
agent.create(
    from?=<Agent | settled Turn>,
    project?,
    title?,
    task?,
    access?,
    sandbox="auto"|<SandboxRef>
)
```

Model:

```text
agent(task, title?, background=false, from?=<Agent|settled Turn>)
```

- No host `agent.fork`.
- Fork remains a semantic immutable-history relation.
- `from` selects history ancestry only; it does not grant/select Sandbox/Vault/source
  authority.
- Exact history forks preserve inherited identity/content/provenance and structurally
  share HistoryBase/HistoryGrant.

### Checkpoints / SourceSet

Host remains:

```text
checkpoint.list
checkpoint.get
checkpoint.create
checkpoint.restore
checkpoint.delete
```

Checkpoint is:

```text
exact HistoryBase/head
+
immutable named SourceSet
```

The SourceSet can contain multiple typed anchors such as:

```text
code    -> Git anchor
infra   -> Git anchor
data    -> versioned/content-addressed object/bucket manifest
```

- Git is the mandatory first SourceDriver.
- Dirty Git uses AW's hidden synthetic commit/tree + private index, without moving user
  HEAD/branch/index.
- Multi-source capture is all-or-nothing at one fenced settled Agent boundary.
- There is no generic tar/archive fallback.
- `checkpoint.restore` is intentionally retained because it is stronger than
  `agent.create(from=...)`: it creates a **new Agent and new Sandbox** and restores every
  captured SourceAnchor.
- Restore never rewinds an existing Agent/Sandbox/worktree.
- Checkpoints do not contain Vault secrets/grants/live leases or runtime state.

### Logical Sandbox

Sandbox is durable logical mutable execution state, **not** one persistent Bubblewrap
namespace.

Host:

```text
sandbox.list
sandbox.get
sandbox.create(project?, from?=<SandboxRef>)
sandbox.close
sandbox.sources.list
sandbox.sources.set
```

Subagent modes selected by trusted host/delegation policy:

```text
share    -> same SandboxRef
branch   -> sandbox.create(from=parent)
fresh    -> sandbox.create(project=destination Project)
```

Multiple Agents may share the same mutable Sandbox while holding different authority.
Every actual Process gets one fresh private Scope selected from current Sandbox launch capability +
current Agent/tool/Vault authority.

The frozen `017-sandbox-mount-convergence-2026-11-18` remains the separate lower
mount/pinning/Bubblewrap implementation plan and is **not promoted** by DL-200 design.

### Vault

Vault is orthogonal privileged secret/service authority.

- Never mount a whole vault as a readable directory.
- Host attaches selected `VaultEntryRef` values through Agent-scoped `VaultGrant`.
- Prefer broker/socket/short-lived credentials; bounded ephemeral file/env fallback is
  allowed for tools that require it.
- Same Sandbox does not mean same Vault authority.
- History fork, Sandbox derivation/sharing, ACL inheritance and checkpoint restore never
  implicitly inherit Vault grants.

Minimal Agent binding surface:

```text
agent.vault.list
agent.vault.set
```

VaultProvider/embedding host owns secret creation/rotation/storage.

### Events

One Event concept; no Signal.

Subject projection:

```text
d:<domain>.i:<principal>.p:<project>.a:<agent>
[.t:<turn>][.j:<job>][.n:<node>].e:<event>
```

Wildcard cardinality:

```text
k:*   one token of the declared kind
>     one-or-more trailing tokens, terminal only
```

Names:

```text
e:changed        system
e:settled        system
e:#ready         custom
e:#tests_passed  custom
```

- Bare Event names are system-owned forever.
- Custom publish requires `#[a-z][a-z0-9_-]{0,62}`.
- There is no `e:#*` partial-token wildcard; `e:*` matches one system/custom Event token.
- Events carry stable opaque `Event.id` for provenance/idempotency.
- Existing immutable `commit_log` remains the one Event history/order owner.

Host:

```text
events.read
events.subscribe
events.publish
events.subscriptions.list
```

Model:

```text
events_subscribe
events_publish   // custom #name only
```

### Product extension / Histate seam

Do not use arbitrary `Agent.metadata` as a product database.

DL-200 now defines:

- typed non-authority `ResourceRef` for projection;
- one plain `Extension` registers through one typed `ExtensionRegistry`;
- ExtensionRegistry exposes typed Tool/Hook/Action/Resource/View/EventType/EventHandler/
  Provider/ProviderHook/ProcessHook/UI/Setting/State/Compactor plus native SourceDriver/
  SandboxDriver/VaultProvider registries with a common registry lifecycle;
- registration is atomic across subregistries and extension-scoped for ownership;
- native Rust registration preserves concrete types and returns typed handles; stable
  Extension/local IDs + schema versions, not Rust TypeId, own durable/wire identity;
- JSON/erasure is only a heterogeneous-registry or real transport/storage/model boundary;
- runtime callbacks still use scoped `ctx.tools`, `ctx.state`, `ctx.ui`, `ctx.models`, etc.;
- Histate may register fully typed Actions/Views/EventHandlers/UI/Tools/Hooks while keeping
  its durable semantic graph in its own typed store.
- Dynamic Extensions reuse/generalize current `aw-extensions`: any explicitly selected executable may speak one bounded full-duplex versioned stdio protocol that projects the same registry definitions and scoped `ctx.*` ports.
- TypeScript/JavaScript is the first ecosystem SDK; Node/Bun/Deno run directly. AW does not own npm/pip/cargo/package-marketplace lifecycle in v1.
- Do not embed a JS runtime or Wasmtime initially. WASI can experiment today as an external `wasmtime run extension.wasm` process; an embedded WasmExtensionHost is optional later.

Histate remains a separate product owner:

```text
AW:
  execution/history
  Projects/Agents/Turns/Jobs
  Sources/Checkpoints
  Sandboxes/Vault grants
  factual Events

Histate:
  Builds
  product Sessions
  Decisions/Options/Evidence
  summaries
  semantic/decision graphs
  commit classification
  artifacts/semantic edges
```

A Histate Session may reference many AW Agents + zero/many Sandboxes; it is not an AW
Session. A Histate checkpoint may compose one/more AW Checkpoints with decision/summary/
semantic graph heads. Cross-store product/AW mutation uses an idempotent product-owned
saga/outbox; AW does not provide a distributed transaction.

Keep these three DAGs independent but cross-referenceable:

```text
AW History DAG
Source DAG
Histate Decision DAG
```

### Shared Process scope / 0.17.6 adoption

- AW models Process as one supervised execution scope, not one leader PID.
- Shared truth remains only `ProcessTerminal { exit, stop, tree, output }` with
  `Tree::{Empty,Live,Unknown}` and per-stream completeness.
- `PipeProcess` owns one private async `Scope`; direct-host fallback is `DirectScope` with stable leader + containment identity (pidfd when available on Linux, unreaped-PID fallback otherwise). No free readiness/disarm/guard API leaks upward.
- Linux strong target is a tiny AW PID-1 supervisor in a per-command PID namespace with the
  main command as PID 2; a real host/Sandbox launcher instantiates it when promoted.
- Sandbox and Scope are orthogonal: Sandbox answers where/how execution may run; a fresh
  private Scope owns one Process lifetime. Unsandboxed/direct is first-class, sandboxed does
  not imply PID namespace, and stronger Scope selection stays private to the host/launcher.
- ProcessReceipt stores ProcessTerminal directly; Outcome/completeness/status are derived.
- `Tree` is frozen pre-cleanup occupancy; `Tree::Empty` means the full semantic Process
  scope is proven empty, while weak direct-Unix negative evidence is `Unknown` if escaped
  descendants cannot be excluded. Current source still needs this semantic correction.
- Closed-containment-required launches fail rather than downgrade. PID-1 report loss before
  main-exit fact yields ProcessExit::Unknown.
- Dotlink must consume the same shared Process lifetime owner, not merely compatible
  ProcessTerminal fields, and its real consumer must compile against the shared owner
  without importing the full Agent adapter solely for Process lifetime.
- dotlink owns `leftovers/unknown`, success/log projection, non-login Bash and the runtime
  policy: ~30-minute checkpoint guidance + 35-minute hard conversational command horizon.
- AW gains no turn-horizon API.

## Immediate next step

Continue **Phase 0 only**. Preserve the concurrently active Phase 0B
replay/admission/exposure work and its dirty files until that grouped gate is complete.
For later phases, preserve the **R5** owner contracts: `agent_state_current` activates via
one exclusive Agent+ACL migration transaction under write exclusion—no v1 dual-write bridge.
Phase 3 durably claims Prepared before external construction work; SourceDriver capture and
restore share one prepared-operation contract, named sources are bounded (default 128, hard
1024), descriptors are non-secret and pin exact driver registration+recovery schema, and
store-open validates all persisted live driver/schema dependencies before source effects.
External publication evidence is immutable/non-revocable or held by a provider fence through
the complete AW commit. Replay guarantees stable effect/created-resource identity while
payload may compact to namespace-lifetime-bound never-retry tombstones. Event consumers use
authority-scoped store affinity + gap-free indexed-through progress. Authorization is source-
first: hidden source denial skips privately without deeper provider checks; visible-source
external dependencies require immutable evidence or a provider fence through admission/read
authorization. `events.read` returns candidate-oblivious retryable
`authorization_unavailable` with unchanged cursor for unresolved visible-source authority.
Agent subscription delivery is same-transaction-store only in v1. Migration sets
`delivery_cutover_rev = current next_event_rev`; accepted legacy Inputs keep identity and
receive conservative immutable disclosure/provenance sidecars rather than re-admission.
Typed leaf registries use one staged-publication primitive from first use. DisclosureManifest
is a structurally shared bounded-fanout DAG whose authority roots only locate live owners to
recheck, not current grants. `jobs_list` is exposure-store-local, gap-safe, ordered exactly
`(publication_position DESC, canonical_ref ASC)`; final rebuild swap verifies/catches up the
publication head, and a resumed tree frontier reauthorizes every Agent ancestor gate before
descending. Nested Agents are allowed but nested batch remains transitively denied under
active Graph ownership. The future PID-1 report channel is supervisor→host only; commands/
descendants never inherit a writable report endpoint or forge terminal facts.
The Process owner is now the private Scope/DirectScope design in SPEC 25.11 and needs no
further guard/PID API expansion. After Phase 0B closes, cut dotlink over to ProcessTerminal,
non-login Bash, one tree/success/drain/signal lifecycle projection and the ~30-minute
guidance / 35-minute hard bounded-turn runtime policy. Do not start Phase 1 yet.
