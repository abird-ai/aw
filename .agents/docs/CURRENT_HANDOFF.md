# CURRENT HANDOFF — unified Jobs + Agent runtime, AW + dotlink

Author: coordinating implementation agent
Owner: @pvl
Prepared: 2026-11-14
Machine: **L5 only**
Canonical AW checkout: `/home/pvl/spaces/abird/src/aw`
Canonical dotlink checkout: `/home/pvl/spaces/abird/src/dotlink`

> This is the current cross-repo entrypoint for the unified Jobs + Agent runtime
> implementation. It preserves the historical line, states the current normative
> design, records the reviewed/pushed implementation checkpoints, and gives the exact next
> boundary. It does not replace Git, live source/tests, or the authoritative design.
>
> **Authority order:** live source/tests/executed results > current `AGENTS.md` >
> current plan index/queue > unified `DESIGN.md` > checkpoint `TODO.md` >
> this handoff > older handoffs/plans.
>
> The large older handoffs remain archival sources for the September–October
> product/runtime history. Their old "current" statements are snapshots, not present
> authority.

---

## 0. Immediate state — do not edit before reading

### AW

- Repo: `/home/pvl/spaces/abird/src/aw`
- Branch: `main`
- Normative design authority remains
  `890e9a383fe2edc3e0bd0aa34a2b217f2724475c`
  (`Resolve shared Job, replay, wake, and Graph ownership design`).
- Reviewed/pushed implementation checkpoint:
  `9e9ab043410241c3f797f760d93a1cfd9126fdd2`
  (`Implement unified Jobs and Agent runtime checkpoint`).
- One canonical worktree only. Never inspect or touch `.agents/plans/ux/**`;
  another owner reserves it.
- The v34 shared-Session migration, Phases 1–5 runtime implementation, shared
  `OperationId`/`ExecutionRef` contracts, and AW-side D2 source-fenced Start
  authorization are committed in that checkpoint.
- AW-side Start authorization is qualified for replay-before-reauthorization,
  writer/membership/source revocation ordering, exact target-generation binding,
  and no replacement Start after changed/ambiguous intent.

### dotlink

- Repo: `/home/pvl/spaces/abird/src/dotlink`
- Branch: `main`
- Reviewed/pushed D1 checkpoint:
  `3f59bb16b0262f6157351fd28b13167cc1b3e5db`
  (`Cut over standalone Jobs to shared AW runtime`).
- dotlink pins `aw-tools`, `aw-tools-fs-unix`, and `aw-tools-fs-win` exactly to
  AW `9e9ab043410241c3f797f760d93a1cfd9126fdd2`.
- D1 standalone cutover and reviewed-shared-pin boundary are complete and qualified.
- D2 dotlink remote Start consume/claim/local-authority/dispatch is the exact next
  implementation boundary.
- Existing second worktree:
  `/home/pvl/spaces/abird/src/dotlink-016-fixes`, branch `release/0.16`.
  Do not touch it and do not create another worktree.
- `docs/CHATGPT_PLUGIN.md` remains a pre-existing unrelated dirty modification;
  preserve it byte-for-byte unless the user explicitly asks to include it.

### User process constraints

- Use **dotlink---l5 only**. Do not use or modify the A1 tunnel/machine.
- Work in the two canonical checkouts above; do not create another worktree.
- Limit one model/user work turn to roughly **30 minutes**.
- Aim for the next coherent checkpoint, record state, then pause.
- Preserve concurrent/unrelated dirty files.
- No push, version, tag, release, deploy, history rewrite, reset-hard or destructive
  action without separate explicit authorization.
- A request to implement is not authorization to commit or push.
- The 2026-11-14 AW/D1 implementation checkpoints were explicitly authorized and
  pushed; that does not authorize future commits/pushes.
- If asked to commit, commits are signed and only owned paths may be staged.

---

## 1. Exact reading order for the next agent

Do this before editing.

### Both repos

1. Read each repo's `AGENTS.md`.
2. Revalidate:
   - `git rev-parse HEAD`
   - `git status --short`
   - `git diff --cached --name-only`
   - `git worktree list --porcelain`
3. Read each repo's `.agents/plans/README.md` and `.agents/plans/TODO.md`.

### AW

4. Read **this file**.
5. Read:
   - `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/README.md`
   - **all** of `DESIGN.md`
   - `TODO.md`
   - `NEXT-AGENT.md`
6. Read the two predecessor AI-native plans as design lineage, not current authority:
   - `ai-native-operations-agents-jobs-transcript-2026-11-10/`
   - `ai-native-job-control-wake-2026-11-10/`
7. For older AW product/runtime history read, as needed:
   - `.agents/docs/HANDOFF.md` — retained full historical synthesis
   - `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md`
8. Read live source/tests for every owner you touch.

### dotlink

9. Read:
   - `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/README.md`
   - `TODO.md`
   - the companion current handoff in dotlink `.agents/docs/CURRENT_HANDOFF.md`
10. For the shared filesystem/Graph/Job lineage read:
   - `.agents/plans/dotlink-ai-native-filesystem-2026-10-31/README.md`
   - `TODO.md`
   - `FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md`
11. Read live dotlink source/tests before D2.

### Normative lower-level reference

12. Treat DL-150's reviewed filesystem/Graph/result/replay/ACK semantics as inherited
    unless the unified `DESIGN.md` explicitly supersedes them.

---

## 2. Historical lineage — why the current design looks like this

This section is a self-contained digest. The older handoffs retain full phase-by-phase
detail and source maps.

### 2.1 AW foundations, September–October 2026

The original AW program established these permanent decisions:

- one Rust Agent and one user-facing `aw`;
- one append-only/durable conversation/Turn path;
- one ToolRegistry/ToolPolicy path;
- one TurnJournal and one authoritative State/Session owner;
- Domain → Workspace → Session as the user model;
- native TUI, browser, headless/RPC and SDK as projections over the same engine;
- view/layout changes never retarget a running Agent or create execution authority;
- provider account, payer, credential custody, inference executor, tool executor,
  Session owner and participant are separate dimensions;
- journal intent before model/tool effect;
- preserve paid/factual results even if later branch attachment fails;
- ambiguous/possibly charged effects remain UNKNOWN and are never silently replayed;
- no silent provider/account fallback, credential disclosure, listener, tool grant,
  network permission or command retry;
- exact Actor/source/site/effect/writer/grant generations are authority, not names.

The historical program then built:

- durable SQLite/JSONL Session/Turn/journal ownership;
- native Workspace/TUI and real terminal/PTY ownership;
- Git/file scoped views;
- browser/local attach and personal-device paths;
- team/shared-draft/context-flow boundaries;
- remote Machine/Fleet/Domain authority;
- writer epochs/failover, signed handoff, device identity and SecretStore;
- portable/no_std Agent cuts (R0–R14);
- P6 runtime-management, retry/reconciliation, recovery and provider correctness.

Phases 64/65 (indexing/LSP) and E6 firmware were deliberately parked. Numbered
historical phases stop at 68. Do not invent a Phase69.

For the detailed chronology and earlier source map see:

- `.agents/docs/HANDOFF.md`
- `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md`

### 2.2 dotlink hardening and DL-150, October 2026

dotlink evolved as the concrete local/remote native executor with:

- schema-v10 profile/config authority;
- private OAuth/ngrok/stdio/http transport boundaries;
- Linux Bubblewrap sandbox by default; network off unless granted;
- descriptor-rooted native filesystem authority and protected-path precedence;
- exact launcher/interpreter/source identity;
- native Process ownership, bounded retained stdout/stderr, writer quiescence,
  cancellation and cleanup;
- replay/request-key dedupe;
- MCP Task result ownership;
- raw-byte retained output + explicit ACK release;
- one canonical Graph/Job runtime.

DL-150 sealed the AI-native filesystem/Graph language:

- compact flat filesystem tools;
- deterministic `ops` Graph;
- one JobManager;
- deterministic NodePlan/footprints;
- Process/shell Graph nodes;
- factual statuses/outcomes;
- pure read vs explicit ACK;
- no workflow-language drift.

The full 2026-11-09 shared-tools handoff records the filesystem/runtime convergence:
`.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md`.

### 2.3 Shared-tool extraction and crate boundary

The cross-repo convergence established:

- `aw-tools` = canonical shared filesystem/Graph/finite-Job semantics/runtime;
- `aw-tools-adapter` = AW-specific concrete execution/authority/process bridge;
- one canonical JobManager, not a second AW workflow engine;
- `aw-tools-fs-unix/win` only where platform implementation is genuinely shared;
- dotlink consumes reviewed `aw-tools` revisions rather than copying AW code.

The naming decision was explicitly:

- old `aw-tools-core` → **`aw-tools`**
- old AW-specific `aw-tools` → **`aw-tools-adapter`**

This is why no new `aw-tools-core` or `aw-actor` crate should be introduced.

### 2.4 November 10 — operation/Agent/Job/transcript plan

`ai-native-operations-agents-jobs-transcript-2026-11-10` established:

- one semantic operation language;
- flat atomic model tool names;
- Agent and Job identity separation;
- one Jobs control plane;
- Agent transcript/history projection;
- wake as source event → Agent input, not a second inference engine;
- dynamic policy without grant issuance;
- dotlink as a thin concrete executor.

### 2.5 November 10 — Job Control/wake plan

`ai-native-job-control-wake-2026-11-10` refined:

- first-class `Control` ToolEffect;
- `jobs.send` semantics;
- source-owned event registration/revision;
- receiver-owned wake/Input admission;
- exact-once wake request rather than fictional exactly-once inference;
- Agent Graph nodes and factual child settlement;
- no public restart.

### 2.6 November 11–13 — unified successor plan

The current successor is:
`.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/`.

Important design commits:

- `ae3e826` — finalize unified Jobs/Agent plans
- `4df93e2` — initial reusable policy/session/host crate foundations
- `53243b5` — unified policy/sessions/layered runtime design
- `3c27d20` — checkpointed implementation plan
- `406bfa5` — resolve Graph/replay/authority/integration design
- **`890e9a3`** — current authority; resolves shared Job replay/wake and
  transitive Graph ownership

`890e9a` materially changed acceptance. Do not use pre-`890e9a` assumptions for:

- replay of contextual refs such as `^`;
- `jobs_subscribe` and constructor `wake`;
- source-owned event generation/revision and receiver-owned Input;
- fast-completion constructor ordering;
- transitive Graph ownership through descendant Agents;
- nested-`batch` denial while Graph ownership is live.

---

## 3. Permanent architecture and decision record

These are the constraints the implementation must preserve.

### 3.1 Model-facing tool surface

Maximum model-facing core: **25 flat tools**.

Filesystem (13):
- the current DL-150 filesystem set.

Constructors:
- `bash`
- `pwsh`
- `powershell`
- `batch`
- `agent`

Jobs:
- `jobs_list`
- `jobs_get`
- `jobs_read`
- `jobs_ack`
- `jobs_send`
- `jobs_cancel`
- `jobs_subscribe`

Do not reintroduce:

- public `agents.*`;
- grouped `jobs({op})`;
- `subagent`, `team`, `plan`, `review_release`;
- `job_start`, `job_restart`;
- compatibility aliases.

Direct tools and model Graph use the same flat spellings such as `read`, `edit`.
SDK/internal semantic IDs use namespaces such as `fs.*` and `jobs.*`.

### 3.2 One owner per responsibility

There must remain exactly:

- one Agent/Actor/TurnMachine/TurnJournal runtime;
- one SessionStore authority;
- one JobManager/Graph runner;
- one Process/output/ACK owner;
- one replay owner for a given effect;
- one policy-admission owner;
- one approval/wait owner.

Never create a second:

- Agent engine;
- Graph scheduler;
- Session database;
- policy database;
- approval engine;
- collaboration queue;
- transcript store;
- event bus.

### 3.3 Identity and refs

Canonical hierarchy is typed `d/i/p/a/t/j/n`.

Rules:

- slash refs are fully resolved;
- non-slash refs are contextual overlays;
- structural `:0` defaults are fixed identities, never caller-relative aliases;
- canonical handles never retarget or reuse;
- Graph node `n:<coordinate>` is direct 1-based coordinate;
- Step has no public path segment;
- an Agent binds one Session+branch once for its lifetime;
- project `a:0` binds its primary executable Session once and is never
  sequentially rebound.

### 3.4 Jobs/data/control semantics

Natural reads:

- Process → stdout;
- Graph → result;
- Agent → latest committed result.

`jobs_read`:
- pure;
- never ACKs;
- raw offsets remain encoding-independent for retained bytes;
- Agent views/filter/history are AW-only disclosure projections.

`jobs_ack`:
- exact ref/view/raw offset;
- no revision;
- monotonic/idempotent;
- cannot release bytes never delivered/owned by that lease.

`jobs_get`:
- rev is conditional observation/change token.

`jobs_send/cancel/subscribe`:
- optional rev = compare-and-act;
- effectful replay claim is established before effect;
- under `890e9a`, original replay claim is looked up **before contextual ref
  re-resolution**;
- the first admission stores the resolved canonical target;
- retry uses that stored target/receipt even if parent/relation changed.

No restart:
- rerun means a fresh explicit constructor under current authority.

### 3.5 Event and wake semantics

Event/wake is not a notification broadcast service.

Source owns:

- subscription replacement;
- subscription generation;
- event revision;
- factual event commit.

Receiver owns:

- current Agent/Input authority;
- causal-work budget;
- idempotent Input acceptance;
- wake scheduling.

Rules:

- source registration and event commit serialize under the source owner;
- constructor `wake` is admitted before exposure/root start;
- fast completion cannot outrun requested wake registration;
- repeated result+settled at one factual checkpoint may coalesce when effective
  mode is the same;
- source event remains factual even if recipient later denies/revokes;
- recipient revocation suppresses new Input; it never rewrites source event;
- dotlink standalone/initial D2 bridge does **not** advertise remote wake.

### 3.6 Agent/Turn authority

Agent is a long-lived stable execution resource.

- one Agent identity ↔ one exact Session/branch;
- Turn is a generation under that Agent;
- root send queues future Input;
- Turn send steer/interrupt remains in the same active Turn;
- whole-Agent close and Turn cancel are distinct.

Every authenticated Input/control boundary creates immutable
`AuthoritySegment` evidence in TurnJournal.

Every causally following:

- model Step;
- tool-result continuation;
- delegated child;
- automatic continuation

remains under that segment until another authenticated boundary or Turn end.

Each paid Step records `ModelInvocationAdmission` binding:

- Agent;
- Turn;
- Step/ordinal;
- AuthoritySegment ID/fingerprint;
- actor/payer/audience/disclosure evidence;
- existing provider/account `InvocationIdentity`;
- host invocation ID.

Retries cannot cross authority segments.

If actor/disclosure changes:

- rebuild provider input from authorized public Session/Turn projection;
- discard previous participant's provider-private continuation/cache/state.

### 3.7 Shared Sessions

AW owns all collaboration semantics.

One shared Session/branch has:

- one Agent coordinator;
- one input queue;
- one Turn writer.

Participation predicates are independent:

- View
- Contribute
- Submit
- Control
- Manage

Never union participant privileges.

Full Session sharing:
- same owner/Agent/branch;
- authorized public history only.

Approved-context sharing:
- creates a **new sanitized Session** in the same Workspace;
- copies only explicitly approved bounded context;
- does not copy private Turns, provider-private state, credentials, tool outputs,
  or original system/provider continuation implicitly.

Writer fencing:
- `WriterFence = NotApplicable | Epoch(nonzero)`;
- durable/multi-writer sites require Epoch;
- stale writer/follower cannot create a second paid Turn/coordinator.

### 3.8 Policy and grants

`aw-policy` is pure:

- constraints;
- denies;
- routes;
- preferences;
- limits.

It **never issues grants**.

Positive authority comes only from authenticated current grant evidence:
`ToolPolicy`, `ToolBindingAuthority`, resource/Session stores, writer epochs,
provider/payer authority.

Guardian/model decisions are advisory evidence only.

Admission guardian:
- Allow
- Deny
- Ask
- Narrow

After Graph seal, Node policy cannot rewrite:

- OperationId;
- arguments;
- topology;
- `when`;
- semantic ceilings.

Node-time policy may allow/deny/ask or narrow route/resource/provider/payer/limits.

### 3.9 ExecutionStrategy

AW host owns:

- Local
- DelegateAgent

ExecutionStrategy is not:

- a model field;
- a ToolEffect;
- an OperationId;
- a Job kind.

DelegateAgent:

- ordinary Agent/Turn;
- receives a gated original semantic executor;
- factual success comes only from original executor receipt;
- cannot widen semantic side effects;
- disclosure authority is separate from execution authority;
- self/ancestor/wait cycles must reject before child admission;
- parent model idles while transparent delegate works;
- uses same finite causal/payer budget.

### 3.10 Graph determinism

- Graph topology/prepared args immutable after admission.
- Dynamic Node routing sees only dependency-closed `GraphActivationContext`.
- DL-150 implicit previous-success dependency counts as dependency.
- Unrelated parallel completions are invisible to route decisions.
- Preflight captures union of candidate route footprints.
- Conflict position is reserved before per-node router/Ask latency.
- Later conflicting node cannot overtake; non-conflicting work may continue.
- Model/SDK/host nested `batch` is rejected.
- AW may expose `agent` as a Graph leaf only with the real Agent adapter.
- Standalone dotlink never exposes Agent.
- Under `890e9a`, an Agent/descendant owned by a live Graph node retains that
  Graph ownership scope transitively; nested `batch` remains denied until the
  owned subtree settles/unknown and conflict footprint releases.

### 3.11 Crate/layer placement

`aw-agent-kernel`
- portable Actor/Envelope/capability invariants;
- tiny WriterFence.

`aw-agent-core`
- sole Agent/Turn engine;
- ToolRegistry/ToolPolicy;
- provider/account identity;
- AuthoritySegment/ModelInvocationAdmission.

`aw-policy`
- pure/no_std policy constraints/preferences compiler;
- no grant issuing/database.

`aw-session`
- backend-neutral Session/coordinator façade;
- no second Session DB.

`aw-tools`
- shared filesystem/Graph/finite-Job core;
- OperationId;
- ExecutionRef;
- Job event value types;
- coordination/footprint values.

`aw-tools-adapter`
- concrete AW native/Process/Job execution bridge;
- no competing Agent/Session/Policy owner.

`aw-host`
- thin composition/routing/policy host.

dotlink
- concrete remote/local native Process/Graph executor;
- no AW Agent/Session/Policy engine.

### 3.12 Performance discipline

Normal Local/single-user path must not gain mandatory:

- collaboration allocations/lookups;
- team network calls;
- guardian/router inference;
- extra Agent queue;
- generic plugin registry;
- unnecessary per-tool allocation.

Prefer static dispatch/borrowed values/existing owner callbacks. Share a primitive
only when two real owners need it.

### 3.13 Hard pre-1.0 cutover

No compatibility surface for removed model APIs.

Do not add:

- legacy route aliases;
- dual schema;
- legacy replay service;
- migration shim solely to keep old vNext model names alive.

Transient old dev/test state may be reset only under the explicit quiescence gate:
ingress closed, original runtime/supervised children stopped, no live/ambiguous
external effects, no outstanding replay/Task/output/ACK owners.

---

## 4. Current AW implementation reality

This section describes the **dirty source**, not merely HEAD.

### 4.1 Phase 1 foundation implemented in working tree

Implemented/shared pieces include:

- canonical `OperationId` + flat-name mapping;
- typed `ExecutionRef`;
- first-class `Control` effect;
- portable `WriterFence`;
- coordination ancestry/value types;
- `aw-policy`, `aw-session`, `aw-host` foundations;
- exact `ToolBindingAuthority` snapshots/generations;
- Local path preserved.

### 4.2 Phase 2 finite Jobs implemented

Implemented:

- one finite Job/Graph/Process runtime;
- heterogeneous exposure ordering;
- `jobs_list/get/read/ack/send/cancel`;
- Process stdout/stderr/result;
- Graph result/node reads;
- exact retention/ACK;
- interactive Process stdin current-authority checks;
- background shell/Graph;
- no restart;
- replay/lost-receipt fencing;
- runtime-generation/sessionless identity fencing;
- source-owned event rule/event revision primitives;
- constructor wake before roots;
- `jobs_subscribe`/wake receiver path in AW;
- replay-before-contextual-ref resolution;
- same-checkpoint result+settled coalescing.

### 4.3 Phase 3 Agent Job plane implemented

Implemented:

- stable Agent ref + exact Turn ref;
- immutable Agent↔Session/branch binding;
- project root `a:0` one-time binding behavior;
- mixed Agent + finite Job list exposure ordering;
- Agent `jobs_get/read/send/cancel/subscribe`;
- root queue, same-Turn steer/interrupt;
- long-lived Agent constructor;
- bounded Agent history/filter/cursors;
- AuthoritySegments;
- ModelInvocationAdmission;
- current provider/payer/audience context rebuild;
- causal continuation/delegation budgets;
- whole-Agent close vs Turn cancel;
- replay-before-`^` re-resolution;
- source/recipient event/wake ownership;
- active Graph ownership propagated through descendant Agents.

### 4.4 Phase 4 policy/delegation implemented

Implemented:

- authenticated grant evidence;
- pure policy resolution;
- durable policy admission claim;
- guardian Allow/Deny/Ask/Narrow;
- paid guardian replay/crash fencing;
- PendingApproval/multi-Ask;
- Local vs DelegateAgent operation router;
- original factual executor gating;
- whole-Graph delegate;
- dynamic per-node delegate;
- pre-seal Graph Narrow;
- node-time mutation rejection;
- conservative route-footprint union;
- conflict-position ordering under router/Ask latency;
- Agent Graph node using same JobManager;
- nested Agent/delegation under one causal/payer budget;
- Graph-owner descendant lease/unknown settlement.

Recorded one-time fixture measurement in TODO:
- Local: 0 auxiliary calls;
- delegated whole Graph: 2 delegate model Steps;
- reported 8 input / 3 output tokens;
- cached test wall time ~0.661 s, Local/delegate matrix ~0.631 s.

### 4.5 Phase 5 shared Sessions implemented

Implemented in working tree:

- migration v34 participation store;
- View/Contribute/Submit/Control/Manage predicates;
- legacy append remains distinct from Submit/Control;
- participant-origin immutable evidence;
- payer/provider/tool/disclosure projection refs;
- participant Submit under participant identity/payer;
- cross-participant Control with persistent segment ownership;
- Carol steering after Bob creates new segment;
- provider-private continuation discarded on actor/disclosure change;
- revocation after acceptance fences before paid dispatch;
- approved-context new sanitized Session;
- shared-session read projection avoids rebinding another user's live Agent;
- transcript/history/wake disclosure revalidation;
- stale writer/failover fencing;
- WriterFence carried across paid provider/tool boundaries.

### 4.6 Checkpoint status nuance

Current AW TODO shows:

- Checkpoint C: `[x]`
- Checkpoint D: `[x]`
- Checkpoint E: `[x]`

But **0/A/B remain formally unchecked**.

The TODO explicitly says checkpoint acceptance is ordered
`0 → A → B → C → D → E → F → G`. Therefore C/D/E checkmarks represent
reported implementation evidence, not a final product acceptance while predecessor
checkpoint formalization/shared-review boundaries remain open.

Do not erase the C/D/E evidence. Do not claim full A–E product qualification yet.

### 4.7 Key executed AW evidence

The working implementation has executed focused regressions for:

Checkpoint C / amended `890e9a`:
- stable Agent create → first Turn → idle → continuation → close;
- queue/steer/interrupt/cancel;
- exact AuthoritySegment persistence;
- contextless constructor denial;
- mixed Agent/finite listing;
- bounded history/disclosure;
- replay claim before parent `^` re-resolution;
- constructor wake before fast roots;
- same-checkpoint event coalescing;
- transitive Graph ownership / nested-batch denial.

Checkpoint D:
- Local zero auxiliary calls;
- single-op DelegateAgent factual executor;
- stale grant/authority denial;
- whole-Graph delegate one JobManager;
- node delegate same JobHost node;
- conflict-order wait;
- pre-seal Narrow;
- node-time mutation rejection;
- multi-Ask zero roots;
- approval revocation;
- guardian crash replay;
- Agent Graph nested Agent under same delegation root/budget.

Checkpoint E:
- Alice full Session share to Bob without owner/Agent/branch replacement;
- Bob Submit uses Bob provider/payer;
- Bob→Carol Control segments across multiple Steps;
- unsafe disclosure rejects before provider;
- revocation-after-acceptance prevents paid dispatch and preserves factual receipt;
- `a:0` stable binding;
- writer failover fence;
- approved-context isolation;
- revoked participant cannot receive later wake or transcript page;
- Local no-delegate path stays zero auxiliary model calls.

Do not rerun all of this mechanically. Re-run by ownership/dependency trigger.

---

## 5. Current dotlink D1 implementation reality

D1 standalone finite Jobs cutover is committed and pushed at
`3f59bb16b0262f6157351fd28b13167cc1b3e5db`.

Changed paths:

- `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/TODO.md`
- `docs/CHATGPT_PLUGIN.md` (**unrelated pre-existing change; preserve**)
- `src/jobs.rs`
- `src/mcp.rs`
- `src/mcp/tools/graph.rs`
- `src/mcp/tools/jobs.rs`
- `src/mcp/tools/shell.rs`
- `src/transports/openai.rs`

### 5.1 D1 public cutover

Standalone model surface is now:

- `jobs_list`
- `jobs_get`
- `jobs_read`
- `jobs_ack`
- `jobs_send`
- `jobs_cancel`

Constructors:
- shell with `background:true`;
- `batch(..., background:true)`.

Old model routes are unreachable:

- `job_start`
- `job_restart`
- old `job_*` names.

Standalone intentionally has **no**:

- Agent;
- Agent transcript/filter;
- `jobs_subscribe`;
- wake;
- nested `batch`.

### 5.2 D1 correctness closed

- Graph root `j:<id>` `jobs_read` returns canonical synthesized aggregate
  `result`.
- Graph root aggregate is intentionally non-ACKable.
- Exact node `j:<id>/n:<coord>` output remains ACKable.
- ACK cannot advance past bytes actually returned by `jobs_read`.
- Process natural read is stdout.
- Interactive stdin remains runner-owned and rev-fenced.
- Current launcher/sandbox/source authority is revalidated before stdin bytes.
- Revocation blocks before bytes.
- Runtime generation is fresh per startup.
- stale finite refs/cursors reject after restart;
- Job IDs are not reused across runtime generations;
- no live Process/Graph resurrection;
- cancellation/unknown settlement preserved;
- MCP Task/replay/stdio/protected-path ownership preserved.

### 5.3 D1 validation

The dotlink TODO records the grouped D1 checkpoint green on L5, including:

- flat catalog;
- shell sync/handoff/background;
- Graph whole/node reads;
- ACK ownership;
- authorized→revoked stdin;
- cancel/unknown;
- duplicate control replay;
- runtime restart stale ref/cursor rejection;
- protocol-clean stdio;
- representative protected-path/sandbox denial;
- owner-sensitive MCP Task/OpenAI replay carry-forward.

### 5.4 D1 shared dependency boundary closed

dotlink is pinned exactly to reviewed/pushed AW implementation revision:

`9e9ab043410241c3f797f760d93a1cfd9126fdd2`

That revision exports the reviewed shared finite Job/Graph runtime plus `OperationId`,
typed `ExecutionRef`, coordination/event primitives and the host-neutral remote-Start
contract. dotlink consumes those shared mechanics without depending on AW Agent,
Session, Policy or host implementation crates.

Standalone `j:<handle>[/n:<coordinate>]` parsing now uses shared `ExecutionRef` typed
suffixes, and shell names use shared `OperationId`. No sibling path dependency and no
copied AW source remain. D1's shared-pin prerequisite is complete.

---

## 6. The exact next boundary: Phase 6 / dotlink D2

Do **not** begin by refactoring D1 again. D1 and the shared-pin boundary are complete.

### 6.1 Shared pin resolved; AW Start owner available

AW implementation checkpoint `9e9ab043410241c3f797f760d93a1cfd9126fdd2`
contains the reviewed shared primitives and the AW-side durable source-fenced Start
authorization owner. dotlink D1 checkpoint
`3f59bb16b0262f6157351fd28b13167cc1b3e5db` consumes that exact revision.

The next work is exclusively the dotlink half of D2: authenticate/validate the AW Start,
claim the exact effect attempt through dotlink's existing replay owner before effect,
recheck current local launcher/sandbox/source/site/resource authority, and dispatch at
most once. Do not introduce a second replay database or upstream policy implementation.

### 6.2 D2 Start authorization contract

The D2 bridge accepts only **AW-resolved canonical intent**.

Each new remote effect must have one per-attempt source-fenced Start authorization that is
atomically issued by AW and bound to:

- original authenticated invocation/replay identity;
- exact effect-attempt ID;
- canonical operation/ref;
- exact arguments digest;
- target dotlink host/profile;
- target **live dotlink runtime generation**;
- actor/source/site/resource grant provenance;
- Session membership/audience where relevant;
- WriterFence/branch/workspace epochs where relevant;
- bounded expiry;
- no broader permission than the original semantic operation.

Linearization:

1. AW rechecks current authority and atomically commits Start authorization.
2. dotlink claims it through its **existing** replay owner before effect.
3. dotlink independently rechecks local launcher/sandbox/source/site/resource authority.
4. effect starts at most once.
5. factual receipt returns to AW.

Race rules:

- revoke wins before authorization → deny;
- Start authorization commits first → one bounded accepted/unknown attempt may dispatch;
- AW offline **before** authorization → no new Start;
- AW offline **after committed authorization** → same-generation bounded dispatch may finish;
- expiry must not outlive dedupe retention;
- old-generation proof never executes after dotlink runtime restart;
- ambiguous effect cannot get a replacement Start for the same original attempt;
- duplicate/lost Start response never double-launches.

### 6.3 D2 ref/control boundary

AW resolves:

- current Agent context;
- `^`;
- structural overlays;
- participant/Session semantics.

dotlink receives:

- accepted exact canonical finite target only;
- original attempt/replay identity;
- authenticated Start provenance.

dotlink does **not** resolve AW parent/Agent/Session hierarchy.

Remote Process/Graph control/read must preserve:

- one dotlink JobManager;
- current local sandbox/launcher policy;
- current Process stdin authority;
- writer/runtime generation fences;
- exact retained output/ACK ownership;
- no Agent transcript/state in dotlink.

### 6.4 D2 wake boundary

Initial D2 must **reject**:

- constructor wake on dotlink-owned remote Process/Graph sources;
- later `jobs_subscribe` on dotlink-owned sources.

Do not extend remote wake merely because AW now has a local source-event protocol.
The dotlink TODO explicitly requires a separately reviewed atomic remote source-owner
event protocol before remote wake is allowed.

### 6.5 D2 checkpoint F cases

Run together after D2 is implemented:

- AW→remote Process/Graph execution;
- exact remote control/read;
- stale writer/source/site denial;
- upstream allow + missing local sandbox grant → deny;
- revoked stdin → deny before bytes;
- duplicate request idempotence;
- no Agent state leakage;
- unsupported remote wake rejection;
- revoke-vs-Start race;
- duplicate proof / lost response;
- expiry vs dedupe retention;
- runtime-generation restart;
- AW offline before new authorization;
- AW offline after accepted Start;
- no old proof in new generation;
- no double launch;
- MCP Task/replay/ACK/stdout cleanliness.

---

## 7. D3 and final AW Phase 7

After D2 passes, do D3:

- remove any remaining old dotlink compatibility decoders/helpers/tests;
- update dotlink live docs/examples;
- record standalone provider schema bytes/tokens;
- verify advanced AW host capabilities did not grow standalone dotlink model Graph/tool surface;
- run dotlink checkpoint 3 only for affected ownership lanes.

Then return to AW Phase 7:

- hard source/API cleanup;
- remove obsolete `job_*`, grouped jobs/agents, separate Agents model route,
  subagent/team/plan/review_release model routes where no independent consumer remains;
- no dual-schema/compatibility service;
- update public docs/SDK examples;
- final grouped G qualification.

Do not call the product complete until formal ordered checkpoints
`0, A, B, C, D, E, F, G` are reconciled and required external/platform gates are either
passed or explicitly pending.

---

## 8. Validation discipline

Do not rerun everything after each edit.

During development:

- targeted package check/test for touched code;
- one coherent vertical slice at a time.

At a checkpoint:

- run the grouped user-visible flows/failure boundaries in the TODO;
- fix until the checkpoint passes as a whole;
- record concise commands/results in TODO;
- carry prior green lanes unless their owner/dependency changed.

Re-run earlier checkpoint only when later work changes:

- owning crate/API/schema;
- authority/replay/concurrency behavior it depends on;
- source event/wake semantics;
- shared Graph/Job primitive it relies on.

Do not create evidence-only commits or giant log archives.

dotlink repo final validation guidance remains in its `AGENTS.md`; sandbox-affecting work
may require `DOTLINK_TEST_BWRAP=1`.

---

## 9. Git and concurrent-work safety

### AW

- only canonical worktree exists;
- never inspect/touch `.agents/plans/ux/**`;
- unified runtime implementation is committed/pushed at `9e9ab043...`;
- live Git state remains authoritative over this handoff.

### dotlink

- D1 is committed/pushed at `3f59bb16...`;
- separate `release/0.16` worktree exists; do not touch;
- do not create a new worktree;
- `docs/CHATGPT_PLUGIN.md` is unrelated pre-existing dirty work and must remain
  outside unified-runtime commits unless explicitly requested.

### Both

- preserve signed history;
- do not disable signing;
- do not push/tag/release/deploy without explicit permission;
- before any requested commit inspect exact staged paths/stat;
- never include unrelated files in a commit.

---

## 10. Source map for the next implementation agent

### AW shared/runtime

- `crates/aw-tools/src/operation.rs`
- `crates/aw-tools/src/execution_ref.rs`
- `crates/aw-tools/src/job_event.rs`
- `crates/aw-tools/src/coordination.rs`
- `crates/aw-tools/src/filesystem/{batch,footprint,job_manager,schema}.rs`
- `crates/aw-tools-adapter/src/{job_host,job_runtime,job_tools,jobs_tools,agent_jobs,operation_router,process,shell}.rs`

### AW Agent/authority

- `crates/aw-agent-kernel/src/writer_fence.rs`
- `crates/aw-agent-core/src/{agent,control,journal,runtime,tool,accounts,conversation}.rs`
- `src/{actor_owner,agent_jobs,subagents,child_actors,policy_runtime}.rs`

### AW Session/store

- `crates/aw-session/src/lib.rs`
- `crates/aw-store-api/src/lib.rs`
- `crates/aw-store/src/{lib,journal,actor_admission,actor_control,actor_queue,actor_authority_steer,job_exposure,policy_admission,session_participation,subagents}.rs`
- `crates/aw-store/src/context/mod.rs`
- `crates/aw-store/migrations/034_shared_session_participation.sql`

### AW host composition

- `crates/aw-host/src/lib.rs`
- `crates/aw-policy/src/lib.rs`
- `src/{main_full,main_minimal,sessions,minimal_sessions}.rs`

### dotlink D1/D2

- `src/jobs.rs`
- `src/mcp.rs`
- `src/mcp/tools/{jobs,shell,graph}.rs`
- `src/transports/openai.rs`
- `docs/ARCHITECTURE.md`
- `docs/SECURITY.md`

---

## 11. Known decision boundaries / do not improvise

1. **Reviewed shared pin is fixed for D2.**
   Use AW `9e9ab043410241c3f797f760d93a1cfd9126fdd2`; do not move the pin,
   copy AW shared code, or use a sibling path dependency without a new review.
2. **Remote Start linearization.**
   Start proof must include target live generation and be claimed before effect.
3. **Remote wake is not in D2.**
   Reject it until separately designed/reviewed.
4. **No Agent engine in dotlink.**
5. **No nested batch anywhere.**
6. **No provider/model result may fabricate original native success.**
7. **No replay may re-resolve a contextual ref before finding the original claim.**
8. **No participant privilege union.**
9. **No Session view switch may rebind a live Agent identity.**
10. **No Graph child may release conflict ownership before all owned descendants settle/unknown.**

If live source proves a contradiction with `890e9a`, update the **owning design layer once**
and continue. Do not carry two semantic paths.

---

## 12. What not to do next

Do not:

- redo D1;
- clean/refactor unrelated dotlink code;
- migrate `docs/CHATGPT_PLUGIN.md`;
- copy `OperationId`/`ExecutionRef` into dotlink;
- add dotlink Agent/Session/Policy code;
- enable remote wake;
- broaden Graph model leaves;
- reintroduce restart;
- run giant primitive suites after every edit.

---

## 13. Next coherent work sequence

1. Revalidate both pushed checkpoints and this handoff.
2. Re-read `890e9a` D2/remote Start sections and dotlink D2 TODO.
3. Implement dotlink-side Start envelope validation and one-use claim through the existing
   replay/owner ledger, bound to host/profile/live runtime generation and expiry.
4. Recheck current local launcher/sandbox/source/site/resource authority before each effect,
   including Process stdin delivery.
5. Wire AW→dotlink remote finite Process/Graph Start/control/read while keeping
   Agent/Session/Policy/history AW-owned and rejecting remote wake/subscription.
6. Run dotlink Checkpoint 2 / AW Checkpoint F as one grouped matrix.
7. Pause and hand off before D3 if the turn approaches ~30 minutes.

---

## 14. Copy/paste prompt for the next agent

Use the prompt in
`.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/NEXT-AGENT.md`.

The prompt deliberately tells the agent to read this handoff and the normative design rather
than reproducing all details inline.

---

## 15. Archival/reference map

These documents remain valuable history but are not current semantic authority:

AW:
- `.agents/docs/HANDOFF.md`
- `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md`
- `.agents/plans/ai-native-operations-agents-jobs-transcript-2026-11-10/`
- `.agents/plans/ai-native-job-control-wake-2026-11-10/`

dotlink:
- `.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md`
- `.agents/plans/dotlink-ai-native-filesystem-2026-10-31/README.md`
- `.agents/plans/dotlink-ai-native-filesystem-2026-10-31/TODO.md`

Current semantic authority:
- AW `ai-native-unified-jobs-agent-runtime-2026-11-11/DESIGN.md` at `890e9a`
- AW checkpoint `TODO.md`
- dotlink companion README/TODO
- live source/tests/executed results

---

## 16. Accuracy rule for future handoffs

When this file is refreshed:

- never overwrite historical handoffs;
- update the observed HEADs/dirty state;
- distinguish committed design from dirty implementation;
- distinguish reported checkpoint evidence from formally ordered acceptance;
- preserve unrelated/concurrent work warnings;
- record only executed validation;
- keep pending decision boundaries explicit;
- if D2/D3 advances, update both AW and dotlink current handoffs together.
