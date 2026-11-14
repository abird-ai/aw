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
  `890e9a383fe2edc3e0bd0aa34a2b217f2724475c`.
- Final shared/runtime implementation commit:
  `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
  (`Complete unified Jobs and Agent runtime`), signed and pushed.
- Runtime Checkpoints 0/A/B/C/D/E/F/G are reconciled and green.
- Final AW qualification includes strict changed-crate Clippy, stateless/minimal/
  no_std+alloc boundaries, wasm32v1-none, DL-150 shared conformance, real
  Process/Graph/Agent/Jobs flows, hard-cutover cleanup and live Qwen selection.
- One canonical AW worktree only. Never inspect or touch `.agents/plans/ux/**`;
  another owner reserves it.

### dotlink

- Repo: `/home/pvl/spaces/abird/src/dotlink`
- Branch: `main`
- Final unified bridge commit:
  `c90a6603ccef0bbf626ce10d95bedf59877747b8`
  (`Finalize dotlink unified Jobs bridge`), signed and pushed.
- dotlink pins `aw-tools`, `aw-tools-fs-unix`, and `aw-tools-fs-win`
  exactly to AW `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`.
- The temporary local remote-Start argument-digest copy is removed; dotlink uses
  `aw_tools::remote_start::arguments_sha256` from that exact pin.
- Final locked qualification against the pin:
  - all-target/all-feature check green;
  - **651 passed / 0 failed / 1 intentional external-fixture ignore**;
  - integrations **1/1, 8/8, 1/1, 12/12, 4/4**;
  - strict all-target/all-feature Clippy green;
  - rustfmt/diff green.
- Final replay-first review is regression-locked for remote Graph, shell and
  cancel; exact redelivery reaches replay claim before mutable
  topology/policy/revision checks.
- Existing second worktree:
  `/home/pvl/spaces/abird/src/dotlink-016-fixes`, branch `release/0.16`.
  Do not touch it and do not create another worktree.
- `docs/CHATGPT_PLUGIN.md` remains unrelated dirty work and was not included
  in the final bridge commit.

### Completion state

The unified Jobs + Agent runtime implementation is **finalized across both repos**.
There is no remaining implementation or exact-pin TODO.

Future work should begin from live Git state and a new scoped request. Release,
tag, deploy, or native platform certification remain separate actions and require
their own authorization.

### User process constraints

- Use **dotlink---l5 only** for implementation/source work. Do not use A1.
- x2 may be used only as an inference host; it is not source authority.
- Work in the canonical checkouts above; do not create another worktree.
- Preserve concurrent/unrelated dirty files.
- No version/tag/release/deploy/history rewrite or destructive action without
  separate explicit authorization.

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

D1 originally closed against reviewed/pushed AW implementation revision:

`9e9ab043410241c3f797f760d93a1cfd9126fdd2`

The final unified bridge supersedes that historical D1 pin with exact AW
`0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`; see Section 6.

That revision exports the reviewed shared finite Job/Graph runtime plus `OperationId`,
typed `ExecutionRef`, coordination/event primitives and the host-neutral remote-Start
contract. dotlink consumes those shared mechanics without depending on AW Agent,
Session, Policy or host implementation crates.

Standalone `j:<handle>[/n:<coordinate>]` parsing now uses shared `ExecutionRef` typed
suffixes, and shell names use shared `OperationId`. No sibling path dependency and no
copied AW source remain. D1's shared-pin prerequisite is complete.

---

## 6. Final exact cross-repo pin — complete

D1, D2, D3, AW Phase 7, Checkpoint F, runtime Checkpoint G and repository
finalization are complete.

Final shared AW implementation revision:

`0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`

Final dotlink bridge revision:

`c90a6603ccef0bbf626ce10d95bedf59877747b8`

dotlink consumes the exact AW revision through normal Git dependencies for:

- `aw-tools`
- `aw-tools-fs-unix`
- `aw-tools-fs-win`

`Cargo.lock` changes only those three Git source revisions. No sibling path,
`file://`, vendored copy or unpushed SHA is used.

The temporary dotlink argument-digest mirror is gone; the bridge now uses
`aw_tools::remote_start::arguments_sha256` directly.

### 6.1 Final remote-Start replay contract

The final implementation regression-locks replay-first ordering:

- AW redelivery looks up immutable Start attempt identity before current
  ToolBinding/ref resolution and rejects changed semantic request digest.
- dotlink validates target/op/args/generation and then the concrete effect owner
  claims the Start before mutable local checks/effects.
- remote Graph claims before Graph preparation/topology revalidation.
- remote shell claims before current shell-policy revalidation.
- remote cancel claims before current revision precondition.
- `jobs_ack`, `jobs_send`, and shared-native FS are claim-first.
- exact Ready receipt replays after later mutable-state/policy/revision changes.
- Claimed/Unknown never becomes a fresh effect.
- genuinely new attempts still recheck current launcher/sandbox/tool/resource authority.

### 6.2 Final qualification against the exact pin

dotlink, locked to AW `0a6c6a0...`:

- `cargo check --offline --locked --all-targets --all-features` — green;
- `AW_BASH=... cargo test --offline --locked --all-features --quiet` —
  651 active / 1 intentional ignored fixture plus all five integrations green;
- strict all-target/all-feature Clippy — green;
- rustfmt and diff checks — green.

No further unified-runtime implementation step remains.

---

## 7. Runtime completion and final qualification

### 7.1 Hard cutover

Completed:

- old `job_*`, `job_start`, `job_restart`;
- grouped jobs/agents compatibility routes;
- old `subagent`, `team`, `plan`, `review_release` model routes;
- route-specific dead CLI/module plumbing;
- production-dead AW `PolicyGraphRouter` / guardian / narrow / approval owner
  (~1,361 lines / ~57 KiB) and its router-only tests/providers.

Retained deliberately:

- one canonical AW Agent/Session/Actor/Turn/Job runtime;
- one shared `aw-tools::JobManager`;
- generic `GraphPolicyRouterSlot` as the independently tested shared JobHost seam;
- dotlink finite/native Process/Graph ownership only.

### 7.2 AW qualification

Green evidence includes:

- strict Clippy across all changed crates/targets with no lint suppressions;
- `aw-tools-adapter` 227/227 at the strict-clean checkpoint;
- Actor-owner 45/45;
- policy-runtime 4/4;
- dock 24/24;
- canonical subagents 14/14 after final model-first fixtures;
- `aw-mcp` 15/15;
- Store library 208/208;
- stateless 27/110/6 and JSONL-minimal 27/113/6;
- no_std+alloc kernel/embedded/FFI + wasm32v1-none;
- independent C11 FFI consumer: 710 checks;
- `aw-tools --all-features --lib`: 303/303;
- AW direct-adapter DL-150 conformance vectors green.

### 7.3 Model-first qualification

Deterministic real Agent/effect fixtures on L5:

- direct Read: 2 parent provider rounds / 1 tool;
- 3-node Graph: 2 / 1;
- Bash background -> `jobs_get` -> `jobs_read` -> `jobs_ack`: 5 / 4;
- Agent: 2 parent + 1 child provider round / 1 parent tool.

Live model-selection evidence on **pvl-x2 inference only** used managed ROCm
Ollama `qwen3.8:27b` with the exact current L5-generated 23-tool AW catalog:

- Read 5/5 -> `read`;
- Graph 5/5 -> `batch`;
- Bash 5/5 -> `bash` with `background=true`;
- Agent 5/5 -> `agent`;
- read-only Job observation 5/5 -> `jobs_get`;
- cancellation 5/5 -> `jobs_cancel`.

Total first-call selection/schema/task validity: **30/30**.

Continuation probes:

- `bash -> jobs_get -> jobs_read -> jobs_ack -> final`: 3/3;
- `agent -> final` with no redundant `jobs_read`: 3/3.

The same representative Qwen Read prompt is 69 prompt tokens without tools and
3,409 with the full catalog, so the actual provider tool-envelope overhead is
about **3,340 Qwen/Ollama prompt tokens**.

### 7.4 dotlink qualification

Fresh after the final replay-first review:

- main suite **651 passed / 0 failed / 1 intentional ignore**;
- all five integrations green;
- strict all-target/all-feature Clippy green;
- rustfmt/diff green;
- old-generation Start rejection green;
- Process stdin revoke-before-bytes green;
- standalone no-Agent/no-wake/no-legacy surface green;
- final Unix standalone catalog: 21 tools / 13,832 serialized MCP bytes.

Checkpoint F's real OAuth/TCP Process and FS/Graph bridge evidence remains
current; later cleanup did not change that transport owner.

### 7.5 What remains

Only the exact final shared Git pin sequence in Section 6. Runtime behavior and
qualification are complete; repository finalization is not complete until the
new AW SHA is reachable and dotlink is repinned/relocated off the temporary digest
mirror.

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

1. Revalidate both canonical L5 worktrees, staged files, worktrees and unrelated dirty paths.
2. Confirm the user has explicitly authorized commit/push. If not, stop and report the exact-pin
   finalization blocker; do not manufacture a local-only pin.
3. If authorized, final AW diff/hygiene review, then signed AW commit + push.
4. Capture the exact reachable AW SHA.
5. Repin dotlink's three AW Git dependencies to that SHA.
6. Replace dotlink's temporary remote-Start argument-digest mirror with the shared AW helper.
7. Regenerate dotlink `Cargo.lock`.
8. Run one locked dotlink all-features/integration + strict Clippy/fmt/diff qualification.
9. Commit/push dotlink only if that action is explicitly authorized.
10. Pause at that exact cross-repo pin checkpoint; do not expand into release/tag/deploy work.

---

## 14. Copy/paste prompt for the next agent

Use the current prompt in:

`.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/NEXT-AGENT.md`

It now describes **final exact-pin / commit preparation**, not D2 implementation.
The dotlink companion prompt says the same thing.

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
