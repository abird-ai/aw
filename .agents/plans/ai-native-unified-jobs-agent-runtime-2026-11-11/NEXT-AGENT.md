# Next-agent handoff — implement unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **implementation-ready consolidated plan, 2026-11-12**.

## Goal

Implement the complete unified Jobs + Agent runtime in AW, then integrate dotlink thinly.
Do not redesign the architecture during implementation unless live source proves a real
contradiction. The detailed semantic authority is `DESIGN.md`; the execution order and
checkpoint gates are `TODO.md`.

Repositories:
- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`

Authoritative plan:
- AW `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/`
- dotlink companion with the same directory name

## Read first

1. AW and dotlink `AGENTS.md`.
2. Each repo's `.agents/plans/README.md` and current queue; do not edit coordinator-owned
   indexes unless explicitly authorized.
3. AW plan `README.md`, `DESIGN.md`, then the checkpointed `TODO.md`.
4. dotlink companion `README.md` + `TODO.md` before bridge work.
5. Sealed DL-150 reference for filesystem/Graph/result/replay invariants.
6. Live source/tests and current dirty state. Source/executed tests outrank prose.
7. Never inspect or touch AW `.agents/plans/ux/`.

## Implementation cadence

Follow AW `TODO.md` phases **0 → 7** in order.

Within a phase:
- make the coherent vertical change;
- use targeted compile/tests only as needed to develop touched code;
- do not repeatedly rerun unchanged primitive/native suites.

At the phase checkpoint:
- run the grouped user-visible flows + failure boundaries listed there;
- fix the slice until the checkpoint passes as a whole;
- record concise commands/results in the TODO;
- carry the passed checkpoint forward.

Re-run a previous checkpoint/lane only if later work changes:
- its owning code/crate/API/schema;
- a dependency boundary it relies on;
- authority/replay/concurrency semantics it relies on.

Do not create giant evidence bundles or a checklist entry per unit test.

After AW Checkpoint E, pin one reviewed AW/shared revision in dotlink and execute the
dotlink D1 → D3 checkpoints. Finish with AW Checkpoint G.

## Non-negotiable architecture

### Model/API surface
- Maximum model-facing core remains 25 flat tools:
  - 13 filesystem;
  - `bash`, `pwsh`, `powershell`, `batch`, `agent`;
  - `jobs_list/get/read/ack/send/cancel/subscribe`.
- Direct tools and model-facing Graph use the same flat names (`read`, `edit`, ...).
  SDK/internal semantic IDs use `fs.*` / `jobs.*`.
- Provider/model-facing `batch` stays the conservative DL-150 leaf surface.
  Advanced nested `batch`/`jobs.*` composition is typed SDK/host-only unless later
  deliberately promoted from measurements.
- No public `agents.*`, grouped op API, `subagent`, `team`, `plan`, `review_release`,
  `job_start`, `job_restart`, or compatibility aliases.

### One owner for every state
- One Agent/Actor/TurnMachine/TurnJournal runtime.
- One SessionStore authority.
- One JobManager/Graph runner.
- One Process/output/ACK owner.
- No second PolicyStore, Agent runner, Graph scheduler, collaboration queue, approval engine,
  replay engine or transcript database.

### Refs/identity
- Typed canonical `d/i/p/a/t/j/n`.
- Fully resolved slash refs; contextual non-slash overlays.
- Fixed canonical `:0` defaults; never caller-relative aliases.
- Handles never retarget/reuse.
- Graph `n:<coordinate>` is its direct 1-based coordinate.
- Agent binds one Session+branch **once for its identity lifetime**; `a:0` may bind its first
  primary executable Session once and is never sequentially rebound.

### Jobs/data/control
- Process natural read = stdout; Graph = result; Agent = latest committed result.
- `jobs.read` is pure and never ACKs; Agent durable history is never ACK-consumed.
- `jobs.ack` uses exact ref/view/raw offset and no rev.
- `jobs.send/cancel/subscribe` compare-and-act when rev supplied; `jobs.get` uses rev as
  conditional observation token.
- Effectful control is replay-safe before effect.
- No model-visible restart; rerun = fresh explicit constructor under current authority.

### Policy/authority
- `aw-policy` is pure constraints/denies/routes/preferences/limits. It **never issues grants**.
- Positive authority comes from authenticated `AuthenticatedGrantPath` evidence backed by
  existing ToolPolicy/ToolBindingAuthority/resource stores.
- `Control` is a first-class effect, neither Read nor Write.
- Guardian/model decisions are advisory evidence, never authority.
- Admission guardian may `Allow|Deny|Ask|Narrow`.
- After Graph seal, Node policy cannot change prepared args/topology/`when`/semantic ceilings;
  only Allow/Deny/Ask or route/resource/provider/payer/limit narrowing.
- `Ask` reuses existing owner wait state. Whole-Graph multiple Ask entries live in one bounded
  canonical `PendingApprovalSet` owned by the Graph Job; roots stay zero until all required
  approvals succeed.

### ExecutionStrategy
- AW host owns `Local | DelegateAgent`; it is not a model field, ToolEffect, Job kind or
  `aw-tools` Agent concept.
- Delegation always uses an ordinary Agent/Turn plus the **original trusted semantic executor**
  and factual receipt.
- Execution authority and disclosure authority are separate.
- Delegation cannot widen the original semantic side effects.
- Transparent delegated `jobs.*` control rejects self/ancestor/wait cycles mechanically.
- Parent model remains idle while transparent delegate works.
- Same causal-work/payer budget applies to delegated/model-origin autonomous work.

### Graph determinism
- Immutable topology/args after admission.
- `GraphActivationContext` contains only immutable metadata + authorized receipts in the
  target node's declared dependency closure, including DL-150 implicit previous-success edge.
- Unrelated parallel completions are invisible to dynamic routing.
- Preflight captures union candidate footprints before roots.
- Before activation-time router/Ask, node claims deterministic union-footprint conflict position;
  conflicting successors cannot overtake while decision waits.
- Router/guardian/approval latency must not change effect order.

### Shared Sessions / multi-user
- One existing Agent input queue and one Turn writer.
- No participant privilege union.
- Every authenticated Input/control boundary creates one immutable TurnJournal `AuthoritySegment`.
- Every causally following model/tool-receipt Step inherits that segment until another
  authenticated boundary or Turn end.
- Cross-participant steer starts a new controller segment and never later reverts to prior actor.
- Each paid Step has `ModelInvocationAdmission` binding Agent/Turn/Step +
  authority-segment/context fingerprint + existing provider/account `InvocationIdentity` +
  host invocation ID.
- Provider retries cannot cross authority segments.
- When actor/disclosure changes, rebuild provider input from authorized public projection and
  discard prior segment provider-private continuation/cache.
- Portable writer fence lives in `aw-agent-kernel`:
  `NotApplicable | Epoch(nonzero)`; durable/multi-writer sites require real Epoch.

### Layering
- `aw-agent-kernel`: portable Actor/Envelope invariants + tiny `WriterFence`.
- `aw-policy`: pure/no_std constraints/preferences compiler.
- `aw-session`: backend-neutral Session/coordinator façade over existing store contracts.
- `aw-tools`: existing host-neutral FS/Graph/Job core; borrowed `GraphActivationContext`,
  ancestry/pending-approval value types where genuinely shared.
- `aw-tools-adapter`: concrete AW/native/Process/Job executor bridge only.
- `aw-host`: thin composition/routing/policy/Session/Agent host.
- dotlink: concrete remote Process/Graph/native executor only; no AW Agent/Session/Policy engine.

## Performance discipline

Prefer static/borrowed/value types and existing owner callbacks.
Keep Local/single-user fast path free of:
- collaboration allocations;
- team/membership network lookup;
- guardian/router inference;
- extra Agent queue/mailbox;
- generic plugin registry;
- extra per-tool heap allocation where avoidable.

Do not introduce abstraction layers just to make the architecture look generalized.
Share a primitive only when two real owners consume the same invariant.

## Cutover

This is pre-1.0:
- hard source/API cutover;
- delete obsolete routes/decoders/helpers after replacement flows work;
- no migration, dual schema, compatibility runner or old replay interpreter;
- transient old dev/test state can be reset only through the DESIGN quiescence gate.

## Git / work hygiene

Preserve unrelated work and other owners' files.
Do not touch `.agents/plans/ux/`.
Use coherent checkpoint commits only when the current user authorization permits commits.
Do **not** push, version, tag, release or deploy unless separately authorized in that run.

## Starting instruction

Implement the plan end-to-end now. Start with Phase 0 reconciliation, then execute each
vertical phase and its checkpoint. Do not ask for confirmation between normal phases; make
the best safe implementation decision consistent with DESIGN and live source. If a checkpoint
finds a real design contradiction, stop that slice, record the exact contradiction, resolve it
at the owning layer, then continue. Do not broaden scope into unrelated refactors.
