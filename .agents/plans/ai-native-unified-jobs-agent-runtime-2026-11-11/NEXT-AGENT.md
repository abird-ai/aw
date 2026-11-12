# Next-agent handoff — unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **updated unified scoped-policy/execution and Session-sharing design + thin crate bootstrap 2026-11-12**. New crates `aw-policy`, `aw-session`, `aw-host` and existing AW local-entry composition were added; future API/runtime behavior remains unimplemented and must be gated by the plan tests.

## Read order

1. Read AW and dotlink AGENTS.md.
2. Read each repository's .agents/plans/README.md and current queue without editing coordinator-owned indexes.
3. Read this plan's README.md, DESIGN.md and TODO.md completely.
4. Read sealed DL-150 for filesystem/Graph/result/replay invariants.
5. Read the two 2026-11-10 successor plans only as historical rationale; where their proposed vNext API conflicts with this plan, this plan wins.
6. Inspect live source/tests and dirty work before edits. Source and executed tests outrank dated prose.
7. Do not inspect or touch AW .agents/plans/ux/.

## Starting prompt

Implement the new unified Jobs + Agent runtime plan in AW first, then integrate dotlink thinly.

Repositories:
- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink

New authoritative proposal:
- AW .agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/
- dotlink companion with the same directory name.

Crucial decisions:

- Model-facing core is exactly 25 flat tools at maximum:
  13 filesystem; bash/pwsh/powershell/batch/agent; jobs_list/get/read/ack/send/cancel/subscribe.
- SDK generic calls/policy/internal evidence use fs.* and jobs.* semantic IDs, but model-facing Graph tuples deliberately reuse the same flat leaf names as direct tools (read/edit/...); Graph admission maps them below the wire.
- No public Agents API. Agent is a long-lived Job kind with one stable immutable a:<handle> identity inside a d/i/p namespace; a:0 is the reserved project root/default Agent. **It may lazily bind one primary executable Session/branch only**, while additional independently active Sessions/branches need distinct a:<handle> identities. No per-user/per-team coordination Agent is created on share.
- Segment grammar is typed: d:/i:/p:/a:/t:/j:/n:. Allocators may use numbers/slugs/UUIDs/hashes/etc., but canonical handle spelling is one 1..64-byte delimiter-safe ASCII token, case-sensitive, with no escape/Unicode aliases.
- d:0/i:0/p:0/a:0 are fixed pre-provisioned canonical structural defaults, never caller-relative aliases; /d:<h>/i:0 has one identity for every caller and contextual omission/inheritance represents the current principal. Missing/unauthorized defaults fail closed. t/j handles remain owner-chosen; Graph n is its canonical 1-based coordinate (n:0 invalid).
- Slash refs are fully resolved canonical refs and must include full d/i/p/a. Non-slash refs are contextual overlays resolved from current Agent; partial slash forms such as /a:0 are invalid.
- Contextual overlay semantics: inherit ancestors above first explicit d/i/p/a level, set explicit levels, reset omitted descendants to :0; without current Agent use d:0/i:0/p:0/a:0 base. "." is self and "^" is coordination parent.
- Execution suffixes are owner-typed transitions: Agent->t, Graph Job->n, authoritative owner->j; invalid transitions fail before dispatch.
- Public Job lifecycle is fixed and separate from DL-150 outcomes: running/waiting/idle/blocked/cancelling/cancelled/closing/closed/settled.
- Stable Agent identity is separate from immutable origin and mutable authorized parent/coordinator. Creating an Agent allocates stable a:<handle> rather than nesting it under creator Turn.
- **Unified scoped Policy:** one host-side shape covers exact tool/source/site/resource grants and mandatory denies, `Allow/Deny/Ask/Narrow` guardian decisions, Local/DelegateAgent execution (`Graph`/`Node`), limits/payer/provider/site and disclosure. Resolve authenticated actor separately from canonical target d/i/p workspace owner and verified team memberships; compile host/domain/identity/active-team/workspace/session/Graph/Node scopes with **mandatory denies/ceiling intersection, valid positive grant paths and closest permitted explicit preferences** (Node > Graph > Session > Workspace > Team > Identity > Domain > Host). Actual ToolPolicy/Envelope/ToolRevisionPolicy remain the single exact grant/narrowing enforcers. Per-effect/current-data authority rechecks, epoch-fenced snapshots and one bounded audit/replay identity are mandatory. AW owns policy resolution; aw-tools only generic primitives and dotlink only concrete sandbox/resource enforcement.
- **Permanent crates, thin initial implementation:** `aw-policy` (pure no_std scoped candidate-route/deny/ceiling resolution, never issues grants); `aw-session` (backend-neutral Session access and coordinator-binding evidence over `aw-store-api`, no second persistence store); `aw-host` (thin local entrypoint and future Policy/Session/Agent/tools composition). `aw-agent-kernel` + `aw-agent-core` remain the one Actor/Turn runtime; `aw-tools` is already the core and `aw-tools-adapter` only handles concrete tools. **Do not add `aw-actor`, `aw-tools-core`, another PolicyStore or a SessionManager that merely forwards calls.** Root `src/lib.rs` now enters through `aw-host::configure_local`, and the former duplicate root execution-site/scheduler policy was consolidated there. **Pure `EffectivePolicy::narrow` is monotonic for one admitted route; mandatory outer strategy beats an inner soft preference; same-scope conflicting preferences fail closed.** `aw-session` uses generic backend dispatch and full coordinator-binding identity/revision comparison, with optional writer epochs for non-workspace local backends. Minimal Local remains the priority. Cargo package tests and root minimal check are defined in Phase 1C; future multi-user/delegation remains unimplemented and must fail closed.
- AW ExecutionStrategy is configurable for **every semantic operation**, including FS, Process, jobs.*, batch and agent constructors: **Local is AW's simple default, DelegateAgent is available under independent exact Agent/provider/payer/tool grants**. A delegated Agent always uses the **original prepared native/Process/Job/constructor executor and canonical factual receipt**, not surrogate model text. No operation-kind Local-only restriction or second runner. The core aw-tools lib supplies generic prepared-operation/Graph/Job/execution-scope primitives with route callbacks; AW adapter/host selects routes and user-advanced pipelines. An authorized auxiliary tool call is distinct from the original operation, with its own evidence.
- Turn t:<handle> is the durable addressable Agent generation. Step is journal/scheduling metadata only; no public s:<handle> ref. Turn rev fences observed Step/state.
- Durable Process/Graph/Wait work created during a Turn uses child j:<handle>; Graph nodes use directly derivable n:<1-based coordinate>. Ordinary bounded filesystem calls remain receipts.
- jobs.list is flat deterministic exposed-Job discovery in exact canonical d/i/p; live Agent roots reuse immutable SessionStore creation/commit ordering and finite Jobs use bounded live exposure ordering. No second Agent exposure DB/migration field. List cursor binds scope/runtime generation, authority epoch, position and projection version. Finite constructors reserve required recoverability capacity before effects; handoff receipts include ref+rev for immediate conditional jobs.get.
- jobs.send/cancel/subscribe rev is compare-and-act; jobs.get rev is conditional observation/change token. jobs.read/ack deliberately have no rev and use exact ref + cursor/offset/watermark like DL-150 retained paging/ACK.
- Common model schemas remain flat and typed: SendInput={text}|{eof:true}; ReadView=result|stdout|stderr|messages|tool_calls|tool_outputs|trace; Event=result|settled|attention. `jobs_read` also offers one **AW Agent-only optional filter**: omitted/"default" latest result, "none" all authorized public categories, or custom booleans/tools with `exclude:true`; `view` and `filter` cannot be combined. Standalone dotlink omits Agent filtering.
- jobs.send uses mode auto|queue|steer|interrupt. Agent root auto->queue; active t:<handle> auto->steer within that Turn; queue alone defers to future Turn. Process j:<handle> auto->stdin only after **current Control and independent `process.input` launcher/source/epoch/resource authority against its actual retained sandbox**, never just an open stdin handle. Graph n:<handle> delegates to node owner.
- jobs.cancel({ref,rev?}) always requires explicit ref. "." intentionally closes current Agent; t:<handle> cancels only that Turn; no public Step cancel.
- Queued Agent Input is durable custody, not inference authority. Every Input claim revalidates current writer/source/model/provider/account/payer/budget/ToolPolicy/resource ceilings before replay-stable Turn admission. **Model-issued self/peer sends, child delegation and subscription wakes all share one finite external-origin causal-work budget**, bounded queue/work reservations and payer/Turn caps; a model cannot mint a new root. SDK and bridge retry identities live in the host invocation envelope, not extra model args; ambiguous effects are not blindly retried.
- Waiting inside an Agent Turn consumes no model Step while blocked. A durable independently controllable wait/watch, if later justified, is a Job kind; Notify itself is only a factual event.
- jobs.read replaces jobs.output and all Agent transcript reads. Process defaults stdout, Graph/native defaults result, Agent defaults latest committed result. Historical cursors bind canonical ref, normalized **view or filter** (including tool selection/inversion), disclosure epoch, projection format and intra-record fragments with copyable `more cursor=...`. All authorized public categories are bounded, and `trace`/negative filtering never bypass current disclosure.
- jobs.subscribe requires explicit source; EventRule is a unique semantic set with canonical result,settled,attention order. **Subscription replace/clear and event commit linearize at the source Job owner**; event captures matching subscription generation/mode for delayed delivery, current recipient grants are rechecked at Input admission. Self-source and causal wake cycles are blocked and all model-origin work is budgeted. **AW bridge rejects constructor wake/subscribe against dotlink-owned live Process/Graph sources before effects**, until a reviewed source-owner-fenced atomic registration/event protocol actually exists; an AW recipient alone is not remote wake authority. AW Graph attention/handoff uses the same captured host predicate: completion-only default unless blocking parent-attention was actually delegated; direct Agent creation follows the same sole-parent handoff rule.
- jobs.ack applies only to ackable retained byte streams and always uses explicit ref. Durable Agent history is not ACK-consumed.
- Internal read values are typed; model renderer is compact deterministic text/native content; SDK gets structured typed objects and bytes.
- job_start is replaced by the typed constructor + background=true; job_restart removed in favor of a fresh explicit current-authority constructor. Pre-1.0 source/API cutover stays hard with no compatibility decoder/migration/drain engine. **Before any old transient state reset**, verify a disposable isolated dev/test runtime, closed ingress, stopped supervised processes and no active/unknown external effects, paid attempts, connected clients, Task/replay/ACK obligations. If not provable, abort reset or deploy a separately isolated new runtime; unrelated durable Session/Agent data is never discarded.
- No team/plan/review_release/subagent/grouped op/legacy compatibility surface.
- Internal crash/startup reconciliation remains required even though model-facing restart is removed.
- Preserve DL-150 Graph tuple/when language, atomic compact result/pagination rules, result ceilings, conflict determinism, replay and filesystem semantics.
- **Graph delegation choice:** Local (AW default); DelegateAgent(Graph) **default when delegation is chosen**, using **one Agent/Turn for the entire immutable Graph**; or advanced DelegateAgent(Node) with **dynamic route choice at each runnable node** (Local or Agent), skipped nodes not routed. Optional one-shot whole-Graph guardian/router remains independent and can feed either shape. Root-free Graph preflight captures original semantic grants and **union candidate route footprints**, then per-node runtime route may only narrow the envelope after fresh authority checks. One original JobManager/Graph runner enforces `when`, results, conflict and child settlement; coordinator cannot fake native outputs. Graph-node eligibility (including advanced nested `batch`/`jobs.*`) is a host/adapter-authorized primitive with bounded depth/owner safety, not a class blacklist in aw-tools. Graph one Agent Turn may use multiple model Steps if feedback is needed. Same-Job handoff for parent-blocking attention; no new public tools, unbounded paid routing or implicit effectful detach.
- **Shared Session design:** full sharing leaves personal Session owner/branch/coordinator a:<handle> **unchanged** and adds authenticated participants to the existing one-input-queue/Turn-writer path; selective private-context sharing creates a new sanitized Session and, only when executed, a new Agent. View/Contribute/Submit/Control/Manage are separate Session participation predicates; they never manufacture filesystem/Process/payer privileges. No synthetic team user or coordinator per participant; team is a verified Policy scope, and team-owned project a:0 coordinates its primary Session. Each Turn/effect retains initiating human, actual service/human execution grant, explicit payer, Session/member epoch, ToolPolicy/Envelope and original replay receipt. Revocation fences future inputs, data reads and effects without erasing factual work. Explicit ownership transfer/export/import into team d/i/p is separate from sharing, preserves old personal refs and approved disclosure.
- **Layered extension:** `aw-tools` **is the existing shared core** (no separate `aw-tools-core` crate): opaque host-verified original request/provenance, writer/admission fences, coordinator scopes, typed Job/Graph receipts and minimal callback traits where needed. `aw-tools-adapter` forwards them to original AW/native/Process/Job owners, but owns no Session membership or queue. AW SessionStore/Policy/Actor owns the current Session audience, one Agent input queue/writer, coordinator binding, shared drafts, approved history, user/payer authorization and revocation. dotlink remains concrete remote executor, not team membership or Agent coordinator.
- One original AW Agent/Session/Actor/TurnMachine/ToolPolicy/TurnJournal; no duplicate history/execution owner. Single-user Local default must remain one direct Session/Agent path without collaboration-specific allocations, team lookups or extra model inference.
- Standalone dotlink has Process/Graph Jobs only. Agent Jobs require an authenticated AW host/bridge.

Implementation must also add the new first-class Control effect exhaustively across kernel Effect, agent-core/shared ToolEffect, grants/Envelope, adapter mappings/ranks, journal/store/portable projection and read-wave scheduling; no compatibility encoding or migration layer is retained.

First implementation action must be read-only reconciliation of:
- current aw-tools JobManager/Graph interfaces;
- aw-tools-adapter JobHost/JobRuntime/job_tools/shell;
- Agent Turn/Step cancellation and Actor Input/Notify paths;
- SessionStore branch/history readers;
- ToolRecord/ToolOutput typed projections;
- current direct shell Job handoff/output ownership;
- dotlink Job/MCP Task/replay projection.

Preserve unrelated work. Do not commit, push, version, tag, release or deploy without separate explicit authorization.
