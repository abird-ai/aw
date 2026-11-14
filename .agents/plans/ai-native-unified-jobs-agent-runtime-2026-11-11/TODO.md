# AI-native unified Jobs + Agent runtime — implementation plan

Design authority: [DESIGN.md](DESIGN.md)
Overview: [README.md](README.md)
Handoff: [NEXT-AGENT.md](NEXT-AGENT.md)

This TODO is the **provisional execution order**, not a second copy of the design. `DESIGN.md`
is normative for proposed semantics, invariants and acceptance details. Live source/tests
outrank dated prose for what exists. Parallel implementation is a head start, **not approval
or freezing of still-reviewed public/API contracts**. Reconcile changes in DESIGN before
accepting their checkpoint.

## Implementation discipline

- Implement **vertical slices**, not one checkbox/test per invariant.
- During a phase, run only the compile/test command needed to develop the code being
  changed. Do not repeatedly requalify unchanged filesystem, Graph, replay, native
  backend or platform primitives.
- At each **checkpoint**, run the grouped user-visible flows and failure boundaries
  below once. A checkpoint passes only when the whole slice is coherent.
- Carry a passed unchanged checkpoint forward. Re-run a prior lane only when later
  work changes its owning crate/API/schema, a dependency it relies on, or its
  authority/replay/concurrency semantics.
- Prefer one coherent implementation commit per passed checkpoint or tightly related
  vertical slice. Do not create evidence-only commits.
- Record concise executed commands/results in this TODO when a checkpoint is completed.
  No giant evidence bundles, duplicated logs or per-test prose.
- Native OS/hardware gates run when their implementation changed or at the final gate
  on a capable host; otherwise preserve the last accepted result and mark the gate
  unchanged/pending honestly.
- Keep the normal Local/single-user path cheap: no mandatory Session/team lookup,
  guardian/model call, collaboration allocation, extra queue or generic registry.
- **Checkpoint acceptance is ordered: 0 → A → B → C → D → E → F → G.** A checked
  validation lane is an implementation agent's **reported progress**, not acceptance
  while predecessors or amended design gates remain open. The current C/D `[x]` entries
  are provisional until 0/A/B pass and their new affected cases are checked. Preserve
  the reported evidence; requalify only the newly affected invariants at the next gate.
  Record a concise accepted checkpoint + commands/results, not a second test ledger.

## Phase 0 — reconcile live state and freeze the implementation map

Goal: start from the actual current architecture without redesigning it during implementation.

- [x] Re-read live AW/dotlink source, current plan index, affected tests and dirty work.
      Map the current `aw-tools` JobManager/Graph/PreparedOperation owners,
      `aw-tools-adapter`, Agent/Actor/TurnJournal, SessionStore, Process/output
      retention/replay/MCP Task paths, and the thin `aw-policy`/`aw-session`/`aw-host`
      skeleton.
- [x] Produce one short implementation map in the commit/working notes: exact crates/files
      owning OperationId/ToolEffect, refs, Job control, Agent Turn control, policy,
      Session coordination and dotlink bridge. If live ownership materially differs
      from DESIGN, fix the plan once before implementation rather than carrying two paths.
- [x] Identify obsolete pre-vNext routes/helpers to delete during cutover. No migration,
      compatibility decoder, second Job/Agent engine or parallel policy/session store.
- [x] Reconcile the approved initial Graph eligibility: native DL-150 Graph leaves + AW `agent`,
      **no nested `batch` on any surface**; prove the paid-guardian replay ordering, mixed-owner
      Job-list ordering, explicit SDK Agent owner, and remote Start linearization in the design
      before freezing dependent public interfaces. Implementation code does not decide these.

### Checkpoint 0 — baseline / ownership
- [x] Current targeted crates build in the configurations needed to start implementation;
      existing unrelated failures are identified before edits. No broad qualification run
      is required here.

## Phase 1 — shared language, authority primitives and Local host seam

Goal: establish the reusable low-level contract while keeping Local execution behavior
and DL-150 semantics unchanged.

Implement together:
- canonical semantic OperationIds and flat provider names; one mechanical model-name →
  semantic-ID resolver used by direct tools and model-facing Graph;
- first-class `Control` effect through kernel/core/policy/Envelope/adapter/journal
  projections and scheduler classification;
- typed immutable handles/ExecutionRef/Revision and directly derivable Graph
  `n:<1-based coordinate>`;
- portable `WriterFence = NotApplicable | Epoch(nonzero)` in `aw-agent-kernel`/
  authoritative Agent/Session owners; `aw-tools` receives only opaque host-verified owner
  generation/admission tokens or callbacks, **not an Agent-kernel dependency**;
- host-neutral borrowed `GraphActivationContext`, wait/control-ancestry query input and
  bounded PendingApproval value shapes in shared layers only where multiple owners use them;
- pure `aw-policy` constraints/preferences compiler, `aw-session` coordinator evidence,
  and thin `aw-host` composition with default Local/no-guardian execution;
- preserve no_std/minimal/stateless boundaries and avoid new allocation/lookup cost on the
  direct filesystem hot path.

Do **not** implement multi-user policy, Agent delegation or dotlink bridge behavior yet.
Unsupported advanced paths must fail closed.

### Checkpoint A — shared foundation + unchanged Local behavior
- [x] Validate as one group:
  1. provider catalog remains flat and bounded; model Graph uses the same flat leaf spelling;
  2. representative direct `read/edit` and compact native `batch` still produce DL-150
     outcomes/`more`/`incomplete` and deterministic node coordinates; **nested `batch`
     is rejected on every Graph entrypoint**. The AW `agent` leaf is only projected when
     its exact Agent owner adapter exists (Phase 3/4), not exposed by an unfinished skeleton;
     standalone dotlink never advertises Agent leaves;
  3. Control is denied under Off/ReadOnly without an explicit grant and does not join Read waves;
  4. minimal/no-default/no_std-capable crate boundaries still compile where supported,
     and standalone `aw-tools` has no `aw-agent-kernel`/Session/provider dependency;
  5. Local host path performs no guardian/provider/session/collaboration work beyond the
     pre-existing Agent invocation.
- Do not rerun full native backend suites here unless Phase 1 changed their implementation.

## Phase 2 — finite Process/Graph Jobs and the new jobs.* data/control plane

Goal: land the complete finite-Job API before layering Agent Jobs on top.

Implement together:
- Process/Graph Job refs, exposure/listing and lifecycle/revision snapshots via **one
  host-neutral bounded exposure-order contract**: durable project/SessionStore-backed index
  when available, runtime-generation-scoped in-memory index for sessionless Local AW.
  Reserve finite exposure/recoverability capacity before effects; publish the same owner
  exactly once or preserve unknown. Sessionless cursors cannot survive restart and `j:`
  handles never collide across runtime generations. Agent-root publication joins in Phase 3;
  neither mode copies lifecycle history or adds a manager/mandatory DB to local tools;
- `jobs_list/get/read/ack/send/cancel` for finite Jobs;
- shared source-owner **event registration/commit** primitive for AW-hosted finite Jobs
  using existing Job revisions: atomic constructor `wake` admission before first effect,
  bounded source-captured subscription generation/event revision and idempotent callback
  receipts. Do not advertise AW wake until Phase 3 supplies its real recipient Agent;
  non-serializing/remote dotlink sources reject wake before any effect;
- Process natural stdout, Graph natural result, node result/stdout semantics;
- raw-byte retention/offset/ACK ownership, exact EOF/truncation, lost-receipt promotion,
  same-Job shell handoff and idempotent control replay;
- interactive Process stdin with **both** current Control and independent current
  `process.input` authority;
- shell/`batch` `background:true` replacing old `job_start`; no restart capability;
- AW contextless SDK/host Process/Graph constructors require an explicitly selected **existing
  authorized Agent owner**; never auto-create/bind `a:0` from parser defaults; standalone
  dotlink finite Jobs keep their own authenticated live-session namespace;
- until the actual Agent/Turn Graph owner is wired in Phase 3/4, provider-facing Graph
  stays on reviewed DL-150 native/shell leaves. Thereafter AW may advertise its explicit
  `agent` leaf; typed SDK/host-only `jobs.*` leaves need separate owner qualifications,
  and **nested `batch` remains rejected everywhere**.

### Checkpoint B — finite Job user flows
- [x] Validate these flows together:
  1. shell completes synchronously **or** hands off to the same Job; lost response never reruns it;
  2. background shell/Graph returns compact ref+rev and `jobs_get(wait_ms)` observes settlement;
  3. Process stdout/stderr/result and Graph whole/node reads page exactly; read never ACKs;
  4. ACK is monotonic/idempotent and another replay/Task owner prevents premature cleanup;
  5. interactive stdin succeeds when currently authorized, then fails before bytes after revoke;
  6. cancel/timeout/unknown preserves writer quiescence and factual uncertain effects;
  7. duplicate effectful control returns its original receipt; changed duplicate fails closed;
  8. finite-Job exposure/list ordering and lost-receipt promotion use one bounded
     monotonic index (durable Session host or ephemeral Local), no missing/duplicated refs;
     sessionless restart rejects stale cursors and cannot reuse old `j:` identity;
     Agent/finite mixed pages are tested at C;
  9. source-owned finite event rule registration versus fast terminal event commits one
     captured generation/revision or rejects unsupported owner before effects; no
     receiver wake is advertised before its Agent owner exists.
- Re-run DL-150 native filesystem/Graph conformance only if shared Graph/filesystem semantics
  changed during Phase 2.

## Phase 3 — Agent as a Job, Turns, authority segments and Agent history

Goal: make Agent lifecycle/control use the same Job plane without a second Agents API.

Implement together:
- stable Agent `a:<handle>`, exact Turn `t:<handle>`, heterogeneous routing to authoritative
  Session/Actor/TurnJournal owner, and one-time Agent↔Session/branch binding;
- publish Agent roots into the **existing project Job-list index**, reusing the same
  comparable exposure sequence as Phase 2 finite Jobs; commit creation/list publication
  atomically and keep SessionStore as the sole Agent lifecycle owner;
- require an explicitly selected existing authorized Agent owner for contextless AW SDK
  Job-producing calls; parsing defaults never fabricate a new root Agent;
- `jobs_list/get/read/send/cancel/subscribe` for Agent/Turn refs; queue/steer/interrupt;
  AW-owned `jobs.subscribe` and constructor `wake` only for exact sources with serialized
  event commits and one currently authorized Agent recipient. Source owns registration,
  receiver owns idempotent Input, no new event database. Per-Turn `result`, Agent `settled`,
  supported Graph-child `attention` project one source-committed event revision;
- immutable TurnJournal `AuthoritySegment` per authenticated Input/control boundary;
  all causally following Steps inherit the current segment until another authenticated boundary;
- lightweight `ModelInvocationAdmission` binding each paid Step to exact authority-segment
  fingerprint + existing provider/account InvocationIdentity + host invocation ID;
- causal-work/payer budget across model-origin self/peer sends, children and continuations;
- Agent `jobs_read` natural latest result plus bounded views/filter/history cursors;
- cancellation/close truthfulness and exact replay/unknown-effect behavior;
- unresolved active Turn uses public `waiting`/`cancelling` while its **Agent** is `blocked`;
  an irreconcilable Turn can settle with factual `unknown`/`mixed`, never a nonexistent
  public `Turn blocked` lifecycle state.

### Checkpoint C — Agent lifecycle + replay
- [x] Validate these flows together:
  1. create Agent → first Turn → idle → continue via root send with stable Agent identity;
  2. queue while busy creates future input; steer/interrupt stays in the exact active Turn;
  3. a steering AuthoritySegment persists across tool-result-driven Steps and never reverts;
  4. replay/lost initial Agent response recovers the same Agent/Turn with no duplicate paid effect;
  5. provider retry under a changed authority fingerprint is a new admission and cannot reuse
     previous paid/private continuation state;
  6. Turn cancel vs whole-Agent close has exact scope and preserves unknown effects;
  7. default Agent read is compact; filtered/history reads remain bounded and disclosure-safe;
  8. unresolved paid/external attempt blocks the Agent without emitting `blocked` for a
     Turn, losing the accepted Input, or dispatching a new Step;
  9. interleaved Agent creation + finite Job exposure across several `jobs.list` pages
     remains one immutable comparable order, replay-stable without duplicate lifecycle state;
  10. contextless AW SDK without an existing authorized Agent owner denies Job constructors
      before effects; explicitly bound owner succeeds;
  11. retry `jobs.send/cancel/subscribe(ref:"^")` after parent rebind uses the original
      claimed resolved target/receipt **before** contextual re-resolution; changed wire
      request under same ID rejects, and a fresh invocation resolves the new parent;
  12. AW-owned constructor `wake` versus immediate completion and source subscription
      replace/clear versus event commit capture the correct generation. Repeated result/
      settled delivery coalesces, recipient admission is idempotent and currently authorized,
      revocation suppresses new Input, and finite causal-work limits block wake cycles.

## Phase 4 — scoped Policy, Ask and ExecutionStrategy delegation

Goal: add advanced host policy/delegation without changing semantic operation identity,
model surface or Local hot-path behavior.

Implement as **three coherent owner-local lanes, then one combined Checkpoint D** (no test-per-field bureaucracy):
1. Proven grants and replay/approval admission: authenticate positive `AuthenticatedGrantPath`
   evidence from existing authority stores and intersect with pure `aw-policy`
   constraints/denies/preferences/limits;
   - coherent PolicyContext/effective-policy revisions, current-authority revalidation,
     deny precedence, exact payer/provider/site authorization;
   - **claim the original replay identity and bounded paid/approval/child budget before
     any guardian/router invocation**. Persist per-attempt evidence; unknown paid attempt
     cannot be automatically rerun after crash; admitted argument `Narrow`/route/policy
     are immutable evidence *inside* the original replay claim;
   - admission guardian `Allow|Deny|Ask|Narrow`; after Graph seal, Node decisions can only
     Allow/Deny/Ask or narrow route/resource/provider/payer/limits;
   - replay-stable `PendingApproval`, with whole-Graph multi-Ask as one bounded set in
     the **existing** Graph Job;
2. Single-operation execution route: AW-host `Local | DelegateAgent` for original
   semantic operations; an ordinary Agent/Turn must invoke the original trusted executor
   for factual receipts. Intersect original and delegate tool/provider/payer/disclosure
   authority and reject self/ancestor/wait-cycle controls before paid admission;
3. Graph routing: one Graph coordinator or activation-time Node routing, deterministic
   dependency-closed `GraphActivationContext`, captured union footprints and conflict
   positions claimed **before** per-node router/Ask. Immutable prepared args/`when`/topology,
   existing JobManager and exact owned-child settlement remain authoritative.

Use targeted tests while building each lane; run **one** integrated Checkpoint D after all three.

### Checkpoint D — policy/delegation failure boundaries
- [x] Validate as one integrated matrix:
  1. Local default performs zero auxiliary model calls and remains semantically identical;
  2. DelegateAgent returns the original factual operation receipt; free-form child prose cannot
     fabricate native/Process/Job-control success;
  3. missing original grant, delegate grant, payer, provider or disclosure permission denies;
  4. delegated self/ancestor/wait-cycle controls reject before child admission;
  5. whole-Graph delegation keeps one JobManager; Node routing is identical under different
     incidental parallel completion orders;
  6. router/Ask wait cannot let a later conflicting node overtake;
  7. admission-time Narrow can revise before seal; Node-time argument mutation rejects;
  8. Graph multi-Ask starts zero roots, accepts approvals independently/idempotently,
     resumes only when all required approvals pass, and replay never reruns guardian;
  9. crash between paid guardian acceptance and its durable decision does **not**
     duplicate inference on replay (same original request/claim, unknown/pending attempt);
     a policy-generation change also cannot turn original redelivery into new paid work;
  10. policy revocation blocks new effects without retargeting/rerunning already-started work;
  11. nested `batch` rejects before roots on model/SDK/host surfaces **and** via an
      Agent/child still owned by a live Graph node. AW Agent Graph node → nested Agent →
      Process/queued work holds the original conflict footprint through transitive
      settlement/unknown; conflicting successors cannot overtake. A later independently
      admitted Turn revalidates fresh ToolPolicy/causal/payer/footprint authority without
      inheriting the completed Graph's grant. Sole-parent attention cannot deadlock.
- [x] One-time fixture measurement: Local = 0 auxiliary calls; delegated whole-Graph = 2 delegate model Steps with reported usage 8 input / 3 output tokens; cached test wall time 0.661 s (Local/delegate matrix 0.631 s). Do not remeasure unchanged Local primitives.

## Phase 5 — shared Sessions and multi-user coordination

Goal: add collaboration by extending the existing Session/Agent owner, not creating another
coordination runtime.

Implement together:
- Session participation predicates View/Contribute/Submit/Control/Manage over existing stores;
- immutable Agent↔Session/branch binding; `a:0` binds once and is never sequentially rebound;
- portable WriterFence enforcement for branch + Workspace owners;
- full-Session sharing vs approved-context sanitized Session;
- one existing Agent input queue and one Turn writer for all participants;
- participant-origin/payer/audience evidence and current-authority checks at input claim/effects;
- cross-participant control starts a new AuthoritySegment; provider input is rebuilt from the
  authorized public projection when actor/disclosure changes; previous actor private continuation
  is never reused;
- membership/audience revocation, explicit service-principal unattended work and curated
  ownership transfer semantics.

### Checkpoint E — multi-user user-visible flows
- [x] Validate together:
  1. Alice shares an existing personal Session with Bob without changing owner/Agent/branch;
  2. Bob Submit queues under Bob's grants/payer; Alice-only provider/tool/resource rights remain absent;
  3. Bob Control steer creates a Bob authority segment that persists across multiple tool/model
     Steps; Carol steering later creates a Carol segment;
  4. unsafe disclosure context rejects cross-user steer before provider dispatch;
  5. revoking Bob after queue acceptance blocks future claims/effects/pages but keeps factual receipts;
  6. `a:0` cannot bind a second Session concurrently or sequentially;
  7. stale WriterFence/failover cannot create a second paid Turn or writer;
  8. approved-context Session excludes private history/provider state by default;
  9. revocation of a shared participant denies later wake Input admission/Turn claim and
     transcript disclosure without rewriting already committed source events.
- Single-user Local path must remain unchanged in allocation/model-call/membership-lookup behavior.

## Phase 6 — dotlink thin integration

Goal: consume one reviewed AW/shared revision and project only concrete remote execution,
never AW policy/Session/Agent ownership.

- [x] **D1 may begin after reviewed AW A/B shared refs and finite Job contracts**, independent
      of Agent/Session-sharing checkpoints; consume one exact reviewed shared revision (no
      sibling path/copy). Land standalone dotlink flat jobs/ref/read/ack/control with original
      sandbox/launcher/protected-path checks and no Agent leaf or nested `batch`.
- [x] **D2 waits for AW E** and the necessary Policy/Start contracts. Bridge only resolved
      AW canonical intent plus authenticated actor/source/site/epoch/writer/grant evidence.
      Each remote effect requires an AW-owner **source-fenced Start authorization** bound to
      original attempt/ref/args, exact target host/profile and **dotlink live generation**;
      dotlink claims it through its existing replay ledger **before** effects. Old-generation,
      expired or already ambiguous proofs cannot restart effects or mint a new Start.
      Revocation before AW authorization denies; an already committed proof may dispatch
      during bounded lifetime even after AW disconnects. AW offline blocks **new** Start
      grants, not accepted in-flight dispatch. Dotlink still checks local authority.
- [x] Keep standalone Agent/filter/subscribe/wake absent. Initial bridge rejects remote
      dotlink-source wake/subscription until a reviewed source-owner-fenced protocol exists.
- [x] Keep nested `batch` disabled **even in SDK/host Graphs**. AW model Graph adds
      `agent` to DL-150 eligible leaves; standalone dotlink Graph stays native/shell-only.
      Advanced `jobs.*` Graph controls stay explicitly qualified typed SDK/host-only.

### Checkpoint F — standalone + bridge
- [x] Validate together:
  1. standalone Process/Graph jobs_list/get/read/ack/send/cancel;
  2. standalone schema has no Agent/filter/subscribe/wake or advanced model Graph growth;
  3. AW bridge controls the exact remote Job with stable replay identity and writer fences;
  4. upstream AW grant **and** dotlink local sandbox/launcher grant are both required;
  5. stale actor/team/source/site/writer evidence and revoked Process stdin deny before effect;
     race AW revocation against remote **Start authorization**: revoke-first denies, Start-first
     has one bounded accepted/unknown attempt. Duplicate/lost Start response, proof expiry,
     dotlink restart/new generation, AW offline before grant and offline **after committed
     Start** must not launch twice, revive old Process/Graph or reinterpret old authority;
  6. no Agent/Session/transcript state appears in dotlink and no duplicate remote Job/Graph owner;
  7. MCP Task/replay/ACK ownership and stdout protocol cleanliness remain intact.
  - 2026-11-14 L5 grouped Checkpoint F is green. Two real cross-process OAuth/TCP lanes use the
    production `aw-mcp::RemoteBridge` against the current dotlink binary: explicit no-sandbox
    Process → `jobs_send` duplicate replay → `jobs_read`, and narrow-RW filesystem mutation →
    duplicate replay → Graph → node/read. The narrow server also proves a valid upstream Start
    cannot escape the filesystem grant or mint absent local shell authority.
  - AW authority/race gates are green: shared remote-Start 5/5; Store remote-Start 3/3 including
    writer/membership revoke-before-Start and replay-before-current-reauthorization; exact
    ToolBinding revoke serialization; exact replay survives later revoke while a fresh Start
    denies; execution-site retarget denial; policy-operation-router regression; `aw-mcp` unit
    suite and explicit bearer-HTTP execution-site/proof serialization; all-feature CLI compile.
  - dotlink grouped regression is green: 649 passed / 1 reviewed conformance fixture ignored plus
    all integration suites (child env, exit semantics, live OAuth rotation, private generations,
    stdio tools), strict all-target/all-feature Clippy, remote-Start cluster including runtime
    restart/new-generation rejection, revoked Process stdin before bytes, no wake/subscribe/Agent
    standalone surface, Tasks/replay/ACK ownership and stdout cleanliness. `cargo tree` confirms
    no AW Agent/Store/Host/Policy/Session crate dependency. Same-generation committed proofs are
    self-contained at dotlink; new authorization still requires the live AW lease/commit path.

## Phase 7 — hard cutover, cleanup and final qualification

Goal: remove obsolete code only after the replacement flows are working, then qualify the
consolidated product once.

- [x] Remove old `job_*`, `job_start`, `job_restart`, grouped jobs/agents, separate Agents API,
      subagent/team/plan/review_release model routes and compatibility-only helpers/tests that no
      longer have an independent product consumer.
- [x] Keep the pre-1.0 cutover hard: no migration/dual-schema/legacy replay service. Reset old
      transient dev/test state only through the DESIGN quiescence gate.
- [x] Remove dead duplicate policy/session/host wiring exposed by the new canonical path.
      - 2026-11-14 L5: removed the production-dead AW `PolicyGraphRouter`/narrow/guardian/
        approval owner (1,361 lines / ~57 KiB) plus its four router-only tests/providers. The
        generic `GraphPolicyRouterSlot` remains in `aw-tools-adapter` because JobHost/JobRuntime
        still own and test that shared extension seam; AW no longer layers a second Session/policy
        owner over it. Surviving canonical `subagents::tests` are 12/12 green and the full binary
        test target compiles.
      - Earlier model/API cutover gate: `aw-agent-core` 87 passed / 1 ignored; canonical
        Agent/delegation-focused binary group 17/17; all-feature AW compile green. The cutover also
        fixed stable-Agent hidden `jobs_subscribe` wake authority by giving non-advertised host
        capabilities the same source/site/revoke-generation fencing as visible Tool bindings.
      - Strict all-target/all-feature AW Clippy is still a Checkpoint-G gate: after this cleanup
        the touched Agent-jobs `manual_clamp` lint is gone. Remaining blockers are outside this
        bounded cleanup: `actor_owner` (`large_enum_variant`, one 10-argument helper, one
        `unused_mut`, one test-only cloned-ref lint), `policy_runtime::run_admission_guardian`
        (`too_many_arguments`), and two `web/dock` boolean simplifications. These remain explicit
        next-gate work rather than being silently suppressed or mixed into this checkpoint.
- [x] Update live public docs/examples/SDK references to the new model/SDK names.
- [x] Capture final provider catalog/schema bytes + representative tokenizer estimate and compare
      model/tool rounds for the core user-visible workflows.
      - 2026-11-14 L5 schema snapshot: effective Unix local catalog = 22 tools / 12,524 serialized
        `ToolSpec` bytes (~3,131 at a deliberately rough 4-bytes/token estimate); canonical `agent`
        = 531 bytes (~133), for an effective root catalog of 23 tools / 13,055 bytes (~3,264).
        Cross-platform design ceiling remains 25 when both PowerShell spellings are available.
      - Current 13-tool canonical filesystem subset = 6,295 bytes versus the recorded DL-150
        6,177-byte baseline: +118 bytes (~1.9%).
      - 2026-11-14 L5 deterministic model-first execution snapshot now drives the real Agent/tool
        loop and durable Session authority with canonical schemas: direct Read first-selects
        `read` and completes in 2 parent provider rounds / 1 tool call with 11 returned tool-result
        bytes (~3 tokens at the same rough 4-byte heuristic); a 3-node read-only Graph first-selects
        `batch` and completes in 2 / 1 with 20 bytes (~5); Bash background handoff first-selects
        `bash`, then factually follows `jobs_get -> jobs_read(stdout,limit=4) -> jobs_ack(offset=4)`
        in 5 parent provider rounds / 4 tool calls with 404 aggregate returned tool-result bytes
        (~101); canonical `agent` first-selects `agent`, uses 2 parent + 1 child provider round,
        one parent tool call and 215 returned bytes (~54). All four scripted workflows produced a
        schema-valid canonical first call with zero recovery/alias rounds; the Bash/Jobs chain's
        subsequent three controls were also valid on first attempt.
      - Live representative-model selection is now measured on `pvl-x2` against its managed ROCm
        Ollama `qwen3.8:27b` (27.3B Q4_K_M, 262k context, llama.cpp runner), using the exact current
        L5-generated 23-tool AW catalog transferred verbatim to x2. Five prompt variants each for
        direct Read, 3-node Graph, background Bash and Agent creation were 20/20 correct tool
        selections, 20/20 schema-valid and 20/20 task-valid on the first call. Five additional
        read-only Job observation prompts selected `jobs_get` 5/5 and five cancellation prompts
        selected `jobs_cancel` 5/5, for **30/30 live first-call disambiguation** across six classes.
        Full-catalog first-call averages: Read 3,408 prompt / 25 output tokens / 2.39 s; Graph
        3,415 / 58 / 3.43 s; Bash 3,420 / 49 / 3.18 s; Agent 3,411 / 67 / 4.18 s; jobs_get
        3,404 / 29 / 2.71 s; jobs_cancel 3,398 / 29 / 2.87 s. The same Read prompt without tools
        is 69 prompt tokens, so Qwen/Ollama measures the complete current provider tool envelope at
        about **3,340 prompt tokens**, validating the earlier ~3.26k rough estimate.
      - Live continuation probes using factual synthetic receipts isolate model routing from effects:
        Bash followed `bash -> jobs_get -> jobs_read -> jobs_ack -> final` exactly 3/3, with the model
        carrying `j:bench`, `view=stdout`, `limit=4` and the returned `offset=4`; Agent followed
        `agent -> final` 3/3 and never issued a redundant `jobs_read`. The L5 deterministic fixtures
        above separately execute the real effects/Session/Job paths, so selection evidence is not
        being substituted for execution correctness. No numerical historical DL-150 live-model
        baseline exists, therefore no unsupported relative improvement percentage is claimed.

### Checkpoint G — final product gate
- [x] Run one final grouped qualification:
  - workspace/package build + strict Clippy for crates changed by this implementation;
    - 2026-11-14 L5 strict-clean slice is green with no lint suppressions: `aw-agent-core`,
      `aw-tools`, `aw-tools-adapter`, `aw-store`, and `aw-mcp` all-target Clippy pass with
      `-D warnings`; AW lib, `aw` binary and test targets pass separately with `--no-deps
      -D warnings` (split only to avoid the all-target peak that was killed by the L5 job runner).
      Affected behavior gates are green: `aw-agent-core` all targets; `aw-tools-adapter` 227/227
      with pinned `AW_BASH`; Actor-owner 45/45; policy-runtime 4/4; dock 24/24 with pinned
      `AW_GIT`; canonical subagents 12/12; `aw-mcp` 15/15. The Store library remained 208/208;
      its one stale design-only manifest test still hardcoded schema v33 and was corrected to the
      current v34 remote-Start schema, then passed directly. Unchanged long composite fault lanes
      were not rerun again per the ownership-trigger rule below.
  - minimal/no-default/no_std/alloc lanes whose dependency boundaries changed;
    - 2026-11-14 L5 reduced-boundary slice is green. `aw-tools`: no-default compile,
      read-only 99/99, RW 149/149. Stateless AW: 27 lib / 110 binary / 6 integration;
      JSONL-minimal: 27 / 113 / 6. `aw-agent-kernel`: 33/33 native plus alloc-only
      `wasm32v1-none`; `aw-embedded`: alloc-only native + `wasm32v1-none` compile;
      `aw-embedded-ffi`: 3/3 under both default and alloc-only cuts, alloc-only native +
      `wasm32v1-none`, strict alloc-only Clippy, plus the independent C11 `-Werror`
      consumer: `E5_C_ABI_NATIVE_OK checks=710 model_calls=2 tool_calls=1 events=10`.
      The gate found and fixed two genuine cut regressions: minimal now reuses/re-exports
      the canonical immutable `AgentExecutionBinding` and preserves `switch_session -> ()`;
      portable C ABI gained additive effect code `Control=5` while preserving Unknown=4
      and all prior V1 numeric values. Stateless/minimal strict Clippy also pass.
  - AW user flows from Checkpoints B–E and dotlink bridge flow from F;
  - DL-150 filesystem/Graph conformance **once** if shared primitive code changed since its last
    accepted checkpoint;
    - 2026-11-14 L5 fresh shared pass after the `aw-tools` changes: `aw-tools --all-features --lib`
      303/303 green, and the actual AW adapter `aw_direct_adapter_accepts_every_shared_canonical_request_vector`
      passes with the shared conformance feature. This is the one ownership-triggered DL-150 rerun;
      unchanged secured filesystem/platform primitives were not requalified again.
  - replay/unknown-effect/retention/ACK and policy/authority failure boundaries touched since
    their last accepted checkpoint;
  - platform-native Bubblewrap/Windows/macOS gates only when affected or on a capable final host;
  - provider schema/token snapshot and first-valid-call/round-count comparison.
    - Deterministic real-Agent execution/round evidence is recorded above, and the representative
      live-model selection gate is now green via `dotlink---x2` / ROCm Ollama `qwen3.8:27b`: 30/30
      first-call disambiguation across Read/Graph/Bash/Agent/jobs_get/jobs_cancel plus 3/3 exact Bash
      Jobs continuation and 3/3 Agent synchronous-result continuation. This uses current L5-generated
      schemas; x2 is inference-only and is not an implementation/source authority.
    - Dotlink checkpoint 3 is separately green after the final replay-first review: 651 active
      tests + 1 intentional ignore, all five integrations, strict Clippy, generation-fenced Start,
      stdin revoke-before-bytes and canonical
      standalone surface. The real OAuth/TCP bridge evidence from Checkpoint F remains current because
      neither bridge owner nor remote-Start semantics changed afterward.
- [x] Do not rerun unchanged primitive suites solely because the final phase removed old routes.
      Re-run by ownership/dependency trigger, not habit.
- [x] Leave any unavailable external/platform gate explicitly pending; do not call a partial run full.
      Native Windows/macOS remain separate release/platform gates and are not claimed from L5;
      Bubblewrap/filesystem sandbox primitives were unchanged by this cutover and were not rerun.

### Final exact cross-repo pin — blocked only on commit/push authorization
- [ ] Commit and push the reviewed AW implementation first; then repin dotlink's three shared AW
      dependencies from `9e9ab043...` to that exact new SHA, replace dotlink's temporary local
      remote-Start argument-digest mirror with `aw_tools::remote_start::arguments_sha256`, regenerate
      dotlink `Cargo.lock`, and rerun one locked dotlink package/integration + strict-Clippy gate.
      Never repin to dirty/unpushed AW or use a sibling path. Runtime Checkpoint G is green; this is
      the remaining repository-finalization dependency.

## Completion

The implementation is complete only when:
- prerequisite **Checkpoint 0** and execution **Checkpoints A–G (seven)** all pass; an
  unavailable external/platform gate remains explicitly **pending** and is not a passed
  product qualification or a substitute for a required internal behavior;
- no old model/API route remains reachable;
- one canonical Agent/Session/Actor/Turn/Graph/Job ownership model remains;
- default Local/single-user path retains its lightweight behavior;
- AW and dotlink docs match the implemented surface;
- no unchecked TODO above represents remaining product behavior rather than an external gate.

## Execution guard

This plan authorizes **implementation sequencing only**. It does not itself authorize push,
version, tag, release or deployment. Preserve unrelated work and reserved plan ownership.
