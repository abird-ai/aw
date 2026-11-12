# AI-native unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **vNext design under review; checkpointed implementation may proceed on agreed foundations** (2026-11-12). `DESIGN.md` owns provisional semantics; `TODO.md` gates implementation. `aw-policy`, `aw-session`, `aw-host` have thin working local contracts. Parallel implementation does **not** freeze unresolved public interfaces or validate future scoped Policy, shared Sessions, delegation or Jobs cutover.

This is a new successor plan. It does not edit or rewrite the sealed DL-150 plan, the 2026-11-10 Jobs/wake plan, or the 2026-11-10 operations/Agents/Jobs/transcript plan. Those remain historical design records. This plan intentionally replaces their proposed vNext public/API choices where they conflict with the design below.

Normative design: [DESIGN.md](DESIGN.md)
Implementation phases and gates: [TODO.md](TODO.md)
Next-agent handoff: [NEXT-AGENT.md](NEXT-AGENT.md)

Dotlink has a thin integration-only companion at:
../dotlink/.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/

## Why this successor exists

The previous design still carried unnecessary distinctions and compatibility baggage:

- separate public Agent and Job APIs even though Agent execution needs the same wait/cancel/read/send/subscribe control plane as every other durable execution;
- grouped provider tools and op discriminators despite the model being better served by one simple flat tool per action;
- a separate Agent transcript API even though a long-lived Agent Job can own durable Turn history;
- historical job restart, team, planner and review-release surfaces that are not required primitives;
- SDK filesystem methods left flat even though code benefits from a clear fs namespace.

The new design keeps DL-150's model-first principles but simplifies the public language.

## Final model-facing core

The model sees only simple flat tools:

    ls find grep read write edit patch cp mv rm bread bwrite bedit
    bash pwsh powershell batch agent
    jobs_list jobs_get jobs_read jobs_ack jobs_send jobs_cancel jobs_subscribe

Maximum core catalog: 25 tools before capability/authority filtering.

There is no grouped agents tool, grouped jobs tool, model-visible generic call tool, team, plan, review_release, subagent, job_start, jobs_start, job_restart/jobs_restart, jobs_output, agents_read, agents_get/list/run, watch handle, or compatibility alias in the new contract.

## Canonical semantic language

SDK generic calls, policy and internal evidence use dotted semantic operation IDs:

    fs.ls fs.find fs.grep fs.read fs.write fs.edit fs.patch
    fs.cp fs.mv fs.rm fs.bread fs.bwrite fs.bedit

    bash pwsh powershell batch agent

    jobs.list jobs.get jobs.read jobs.ack
    jobs.send jobs.cancel jobs.subscribe

Rules:

- dot separates semantic resource namespace from operation;
- underscore separates ordinary words inside a field name;
- provider wire projection stays flat and model-efficient;
- provider read maps to semantic fs.read;
- provider jobs_get maps to semantic jobs.get;
- original provider ToolCall remains immutable journal evidence beside the resolved semantic operation.

## Execution strategy and configurable pipelines

**All semantic operations can execute `Local` or `DelegateAgent`.** This includes the 13 native filesystem tools, Process tools, `jobs.*` controls, `batch` and `agent` constructors. The reusable core exposes **generic prepared-operation, Graph, Job and execution-scope primitives**, not an operation-class blacklist or an AW Agent dependency. The AW host/adapter chooses the backend and policy while the **same original native/Process/Graph/Job executor** remains authoritative for effects and receipts.

**AW's ordinary default is `Local`**, without paid router inference. Advanced user/host configuration can choose `DelegateAgent` for any operation; delegation requires separately authorized inference, tool, resource, payer and causal-work grants. A coordinating Agent may perform auxiliary authorized work, but cannot claim native write/read/Process/`jobs.*`/constructor success from model prose: it must call the **original prepared semantic executor** under one replay identity, and the true owner provides the factual result, output and cancellation semantics. No extra public tool, substitute Job or duplicate runtime is introduced.

For a Graph, delegation offers two scopes:

- **`Graph` (default *when DelegateAgent is selected*):** one ordinary AW Agent/Turn coordinates the **entire immutable Graph**. The original Graph manager remains responsible for `when`, dependency ordering, native exact executors, conflicts and settlement. Multiple model Steps may still be necessary for dependencies; "one pass" means one coordinator Turn/scope, **not necessarily one provider inference**.
- **`Node` (advanced):** the existing Graph runner can choose `Local` or `DelegateAgent` **dynamically when each node becomes runnable**, after its `when` dependencies settle. Only activated nodes incur optional per-node routing/model cost; skipped branches need no delegate.
- **`Local`:** original Graph/Job executors and scheduler; an advanced mixed-Graph policy may also delegate selected nodes using the `Node` shape.

An optional **one-shot whole-Graph guardian/router** is independent of execution shape: it may check the entire Graph in one bounded invocation and return per-coordinate guardian decisions/route hints. A host can combine this with Graph delegation or Node runtime routing. **Local with no guardian is still the default**; no automatic N calls per node. A Graph-wide decision is **planning**, not execution of the Graph.

**Initial `batch` Graph leaves:** reviewed DL-150 native/shell leaves, **plus `agent` on AW model/SDK/host Graphs** with exact Agent/Turn ownership and still-owned child-effect settlement. Standalone dotlink remains native/shell-only. **Nested `batch` is rejected in all Graphs—model, SDK and host—in initial vNext.** An Agent node may create subordinate Agents via ordinary authorized `agent` calls, with one inherited bounded causal/payer/child-depth budget; no new Agent type or recursion grant. `jobs.*` Graph leaves remain explicitly qualified typed SDK/host-only when real owner/control/cycle adapters exist, never implicitly model-visible. The shared Graph runner keeps all-leaf preflight, immutable args/topology, union route-effect footprints and dependency-closed Node routing; a delegate cannot bypass `when`, invent receipts or launch another scheduler.

AW host/application settings can configure default strategy, operation/category overrides, guardian stages, Graph delegation scope (Graph or Node), when to decide routes (admission or activation), model/role selection and total cost/concurrency/retention budgets. All these are **host-side, narrowing-only choices**, not new model-facing `batch` arguments or an alternate Graph DSL. Dotlink remains a concrete Process/Graph executor and transport; AW owns any Agent strategy decisions. More complex pipelines compose these primitives and immutable Graph/Job scopes rather than requiring an operation-type prohibition.

The correctness rule remains strict: the original semantic effect, real permissions, replay identity, retained output/ACK, truthful outcome, current Process-input authorization and paid usage must be proven. **Flexibility changes who coordinates execution, not who can fabricate execution facts.**

## Unified policy and execution pipeline

One host-side **`Policy`** describes constraints/denies, decisions, execution preferences, limits and disclosure across **host → domain → authenticated identity/team role → actual workspace owner → session/Agent → Graph/Node/ToolCall**. Positive authority is **not minted by `aw-policy` or preference layers**: AW resolves authenticated direct/team-role `AuthenticatedGrantPath` evidence from existing ToolPolicy/ToolBindingAuthority/resource stores (issuer + issuer epoch + exact scope/grant ref), then intersects those proven grant paths with pure policy constraints. Actor identity is distinct from target owner, `i:0` never remaps to caller, and mandatory restrictions/denies always apply. Session/Graph/Node preferences can narrow/choose within authority, never issue it.

The single pipeline is **authenticate + prove grants/scopes → materialize exact ToolPolicy/Envelope/resource ceilings → claim original replay identity + reserve decision/paid/cost budget → optional journaled guardian (`Allow | Deny | Ask | Narrow`) → authorize revision/route/provider/site/payer → seal accepted decision/arguments → dispatch original executor → factual receipt + current disclosure**. A matching retry attaches to the same original claim *before* any paid stage; policy changes do not create a second paid admission. Ambiguous guardian attempts stay pending/unknown, never rerun automatically. ToolPolicy remains exact grant enforcement, Envelope a narrowing Actor ceiling, ToolRevisionPolicy validates admission-time argument narrowing. After a Graph is sealed, Node-time policy cannot rewrite args; it may only allow/deny/ask or narrow route/resource/provider/payer/limits. `Ask` is a replay-stable wait on the same ToolCall/Graph/node identity; no effect starts, approval/deny/expiry resolves that same wait, and foreground paths proactively hand off rather than deadlocking their only approver.

**Merge semantics:** verified positive grant paths are accepted only under all mandatory resource restrictions; explicit deny wins; ceilings use the tightest applicable bound. Configurable preferences select the closest explicit permissible value: **Node/ToolCall > Graph > Session > Workspace > active Team > Identity > Domain > Host**. For multiple teams, only explicitly selected team context contributes a team route preference; unrelated memberships cannot silently override it. A forbidden preferred provider/route/site may fall back to an allowed configured default (normally `Local`) *before admission*; explicitly required unavailable choices deny. One payer/causal root is charged authoritatively and never reset by scope changes.

**One configuration shape** is used across scopes for constraints/denies, decision, execution preferences, limits and disclosure. Positive grants are separately authenticated host/store grant-path evidence backed by ToolPolicy/ToolBindingAuthority, never preference-layer fields inside pure `aw-policy`. AW's default is Local/no guardian. A one-shot Graph guardian can decide all nodes even when runtime execution is Node-dynamic, and one Graph-scoped delegate can run without a guardian. This remains host/session configuration—not added arguments to the 25 model tools or a second Graph DSL.

An immutable **`EffectivePolicy`** is compiled for the exact authenticated subject, team membership, canonical target workspace, Session, tool/source/site epochs and policy generations. Graph preflight captures allowed route candidates and conservative union footprints before roots. At every later effect, queued Turn claim, Job control, Process stdin write or transcript page, **current authority is revalidated**; revocation may stop new work, but never rewrites a proven existing effect or reroutes an ambiguous one. Journal the final decision/provenance and usage without flooding model-facing responses. Shared `aw-tools` supplies host-neutral primitives, while AW resolves memberships, policy scopes, guardians, approvals and payer/model selection. Standalone dotlink enforces its own concrete launcher/sandbox authority and trusts no AW route suggestion as a new native grant. For bridged effects, AW first commits an **exact source-fenced Start authorization** under current grants/revocation/writer epochs; dotlink independently checks that single-use attempt and its current local sandbox. Revocation before the AW Start transaction denies; after it, already authorized/possibly started effects remain factual/unknown rather than falsely rolled back. If AW is unreachable or proof is stale, no new remote effect starts.

## Core execution model

Job is the universal controllable execution resource.

    Process Job     <- bash / pwsh / powershell
    Graph Job       <- batch
    Agent Job       <- agent
    future Wait Job <- only if a durable wait/watch primitive is later justified

Every identity segment is typed and carries one immutable canonical handle:

    d:<handle>   domain
    i:<handle>   principal / identity
    p:<handle>   project / workspace
    a:<handle>   Agent
    t:<handle>   Turn
    j:<handle>   durable child Job
    n:<handle>   Graph node

Handle allocation is intentionally implementation-defined: numeric, slug-like, UUID/ULID/hash-like, etc. Canonical spelling is still strict: one immutable 1..64-byte ASCII token, first byte alphanumeric, remaining bytes alphanumeric or . _ ~ -, byte-exact/case-sensitive, with no Unicode/percent/escape aliases.

Reserved structural defaults are canonical handles:

    d:0   local/default domain
    i:0   default principal
    p:0   default virtual project
    a:0   root/default Agent

0 is not implicitly special for t/j; Graph nodes use their deterministic 1-based DL-150 coordinate as `n:1`, `n:2`, etc. (`n:0` is invalid). This avoids a second node-handle map or discovery round.

A fully resolved Agent may therefore be:

    /d:abird/i:pvl/p:zg/a:0
    /d:abird/i:pvl/p:zg/a:2

Principal metadata distinguishes user/team/service-account identities; the execution API uses the same i:<handle> resource type for all of them.

"Subagent" is not another type/API. A created Agent receives its own stable a:<handle>; creator Turn/Step/ToolCall remain origin metadata only.

Turn t:<handle> is an addressable durable Agent generation. Step is not public; it remains journal/scheduling metadata for one model invocation plus its complete tool-receipt phase.

Durable Process/Graph work created during a Turn lives beneath that Turn:

    /d:abird/i:pvl/p:zg/a:2/t:5/j:1
    /d:abird/i:pvl/p:zg/a:2/t:5/j:2/n:4

## Canonical refs and contextual refs

The jobs.* family shares typed refs/revisions without forcing one identical argument shape: jobs.list has no ref, jobs.cancel requires ref, self-oriented non-destructive calls may default ref to ".", and rev semantics follow observation versus effect intent.

A slash-prefixed ref is already fully resolved canonical identity:

    /d:abird/i:pvl/p:zg/a:2
    /d:abird/i:pvl/p:zg/a:2/t:5/j:1

Canonical refs always include the full d/i/p/a structural tuple, including explicit :0 defaults. Partial forms such as /a:0 or /i:pvl are invalid.

A non-slash ref is contextual. It is resolved from the exact current Agent structural context.

Special contextual forms:

    omitted / "."   current Agent
    "^"             current authorized coordination-parent Agent

Structural contextual refs act as typed overlays. Given:

    current = /d:abird/i:pvl/p:zg/a:7

then:

    a:0
      -> /d:abird/i:pvl/p:zg/a:0

    a:2
      -> /d:abird/i:pvl/p:zg/a:2

    p:core
      -> /d:abird/i:pvl/p:core/a:0

    i:alice
      -> /d:abird/i:alice/p:0/a:0

    d:remote
      -> /d:remote/i:0/p:0/a:0

    p:core/a:2
      -> /d:abird/i:pvl/p:core/a:2

    t:5
      -> /d:abird/i:pvl/p:zg/a:7/t:5

    a:2/t:3
      -> /d:abird/i:pvl/p:zg/a:2/t:3

Resolution rule:
- ancestors above the first explicit structural segment inherit current context;
- the explicit segment(s) are applied;
- omitted descendants reset to their reserved :0 defaults unless explicitly supplied later;
- t/j/n suffixes attach after Agent resolution.

Without a current Agent context, d:0/i:0/p:0/a:0 is a **parsing base only** over fixed canonical default objects. Each :0 denotes one provisioned identity in its containing namespace; authentication never remaps i:0 to the current caller. Missing or unauthorized defaults fail closed, refs never create principals/projects/Agents, and contextual omission/inheritance—not i:0—represents the current principal. Standalone dotlink needs an explicit local live-session finite Job ref rather than implicitly gaining AW context.

There is no ".." traversal, wildcard/range grammar, or alternate alias syntax. Contextual refs are canonicalized once before authority/replay/effect, and journal evidence preserves both requested_ref and fully resolved canonical resolved_ref.

Stable identity and coordination are separate:
- origin is immutable provenance;
- parent/coordinator is a current authorized Agent relationship;
- "^" resolves current parent, not creator;
- changing parent never renames the Agent;
- path/parent relationships never grant authority by themselves.

## Universal Job control plane

The common reference vocabulary is ref + optional rev, but schemas stay intent-specific rather than forcing one unsafe default everywhere.

Public lifecycle is fixed and separate from DL-150 outcomes:

    running waiting idle blocked cancelling cancelled closing closed settled

The control plane:

- jobs.list({cursor?,limit?}): flat deterministic discovery of authorized exposed Job roots in the caller's exact authenticated canonical d/i/p namespace; no recursive or cross-principal discovery. One project-scoped AW listing/routing index assigns the **comparable monotonic exposure position** for Agent roots backed by SessionStore and finite Jobs backed by JobManager, without copying lifecycle state. Standalone dotlink uses its own finite-Job exposure order. The opaque cursor binds exact d/i/p (or standalone runtime generation), listing authority epoch, exposure position and projection version, so it cannot be replayed into another namespace/runtime.
- jobs.get({ref?,rev?,wait_ms?}): compact lifecycle/state/progress. rev is a conditional observation token for lost-wakeup-safe long polling; state advancement returns the newer snapshot rather than a stale error.
- jobs.read: universal typed data plane for results, streams and Agent history/tool evidence. It deliberately has no rev; exact ref + offset/cursor provide stable replayable paging while each page revalidates disclosure authority.
- jobs.ack: explicit ref/view/offset only; idempotently advance retention for an ackable byte owner. It deliberately has no rev so a lost ACK response remains retryable after cleanup state advances.
- jobs.send: typed {text}|{eof:true} input with mode auto|queue|steer|interrupt. Supplied rev is compare-and-act.
  - auto: resolved resource's natural behavior; default.
  - queue: preserve active execution and defer to the next normal input opportunity/future Turn.
  - steer: on an exact active Turn, settle the current Step, journal steering input into that same Turn, then redirect its next Step.
  - interrupt: on an exact active Turn, redirect at the earliest safe boundary after truthful settlement/cancellation of started effects, then continue that same Turn when safe.
- jobs.cancel({ref,rev?}): destructive target is always explicit; "." intentionally means current Agent self.
- jobs.subscribe({ref,rev?,events}): explicit source ref; events is a bounded unique semantic set with canonical result,settled,attention order. Source owner serializes rule replacement with event commit and captures the applicable rule generation on each event; delayed delivery never applies a different rule. Self-subscriptions and automatic wake cycles are rejected; the same causal/paid-work allowance also bounds **model-issued self/peer continuation**. AW Graphs project attention only when a captured host delegation permits blocking parent coordination; ordinary completion-only Agents do not force background handoff.

Read views are a small enum: result|stdout|stderr|messages|tool_calls|tool_outputs|trace. Initial events are result|settled|attention. Resolved kind capability checks reject invalid combinations.

Background/automatic handoff receipts expose shortest-safe ref + kind/state + current rev (and useful progress) without echoing arguments, so the next jobs.get can long-poll immediately. Agent creation commits its stable identity/Session/first Turn before paid inference; jobs.list reuses existing immutable SessionStore creation/commit ordering for durable Agent discovery, with no second exposure database or migration-only field. Agent-specific durable reads are jobs.read views, not a separate Agents API.

## Shared Sessions and one coordination model

**Sharing a Session does not promote it to a team, change its canonical owner or create a new master Agent.** The Session's existing coordinator and one branch writer remain authoritative. AW's existing SessionStore memberships/shared drafts and Agent Input/TurnJournal provide multiple authenticated participants with one ordered queue and factual request provenance.

- **Private local/domain Session:** one user, one lazy project `a:0` for the primary executable Session, ordinary Local tool execution. No synthetic team identity, network membership resolution, extra coordinator/queue or paid guardian.
- **Existing personal Session shared with Bob:** owner, branch, `a:<handle>` and history remain unchanged; Bob gets only expressly granted View/Contribute/Submit/Control/Manage capabilities. Submission queues a new Turn under Bob's own grants and payer authority; it does not inherit Alice's credentials or interrupt her active Turn.
- **Selected private context shared:** create a **new sanitized shared Session** from approved checkpoint/context, with independent audience and lazily allocated Agent if it executes. A branch inside a still-private Session is **not** a separate disclosure boundary.
- **Team-governed project:** team membership and workspace governance are Policy scopes, not a synthetic team user. One project `a:0` may coordinate its primary Session; additional independent Sessions/branches get their own `a:<handle>` only when needed. Real service/resource principal and unattended payer/provider are deliberately granted, never created on invite.

**Coordinator binding is per executable Session/branch and immutable for an Agent identity's lifetime.** `a:0` may lazily bind its first primary Session/branch once and is never sequentially rebound. Later executable Sessions/branches get fresh Agent identities. Portable `WriterFence = NotApplicable | Epoch(nonzero)` lives in the tiny kernel; durable/multi-writer sites require real epochs. Each authenticated Input/control boundary starts one immutable **AuthoritySegment** in the existing TurnJournal; every causally following model/tool-receipt Step references that segment until another authenticated boundary or Turn end. Cross-participant steer starts a controller segment and never reverts later Steps to the prior actor. Paid/model attempts are replay-bound to the exact segment/context fingerprint plus existing provider `InvocationIdentity`; actor/disclosure changes rebuild provider input from authorized public projection and discard prior segment private continuation/cache.

**Layering:** `aw-tools` (the existing host-neutral core; no separate `aw-tools-core` crate) exposes only necessary generic request-origin/epoch, coordinator/admission/receipt and Graph/Job callbacks; `aw-tools-adapter` forwards authorized prepared operations to their original FS/Process/Job executors; **AW SessionStore/Policy/Actor** owns real members, approved audience, shared drafts, Session coordinator binding, one writer/queue, provider/payer and disclosure. Standalone dotlink remains the concrete remote executor, not another collaboration runtime.

On every participant input, ToolCall and history page, intersect that human's current source-bound grants with Session membership/action, coordinator Policy, workspace/domain resource limits and payer authorization. **Never union participants' rights**, expose private history because a Session was shared, or use the last active human's account for unattended work. Revocation invalidates new work/current disclosure but retains immutable factual effects. Team ownership promotion requires a separate authorized context/resource transfer with new canonical target ownership—not an implicit share.

See `DESIGN.md` Section **2.5.2** and the implementation Phase **1B / 5A** and shared-session conformance matrix in `TODO.md`.

## Agent control and coordination

Normal self continuation is zero-ref:

    jobs_send({
      input:{text:"continue"}
    })

Agent root auto -> queue.

Parent communication:

    jobs_send({
      ref:"^",
      input:{text:"I found a cancellation race"}
    })

Peer/coordinated Agent communication:

    jobs_send({
      ref:"a:2",
      input:{text:"Please review this"}
    })

Cross-project/domain targeting uses the same contextual overlay when authorized:

    jobs_send({
      ref:"p:core/a:2",
      input:{text:"Please review this"}
    })

or a fully resolved canonical ref when the caller already has it.

Precise steering targets the active Turn; Turn rev fences the observed Step/state:

    jobs_get({ref:"t:5"})
      -> t:5 turn running rev=7 step=2

    jobs_send({
      ref:"t:5",
      rev:7,
      input:{text:"focus on cancellation"}
    })

Active Turn auto -> steer.

Fast redirection is explicit:

    jobs_send({
      ref:"t:5",
      rev:7,
      mode:"interrupt",
      input:{text:"stop waiting and use what we have"}
    })

Exact Turn cancellation:

    jobs_cancel({ref:"t:5", rev:7})

Whole current Agent close:

    jobs_cancel({ref:"."})

There is no public Step ref/cancel. If Step state changes before a rev-fenced steer/cancel arrives, Turn rev changes and the effectful request becomes stale.

Queued Agent input is custody, not future inference authority. Before any queued Input becomes a Turn, AW revalidates current writer/source lineage, model/provider/account, payer/budget, ToolPolicy/resource ceilings and closing/blocked/reconciliation state, then journals immutable Turn admission before paid provider effect. A revoked queue item cannot execute under stale authority.

**All model-origin autonomous continuation is bounded**, not only subscriptions: self-send, peer-send, child Agent/Graph delegation and wake retain an authenticated external causal root and its finite pending-Input/paid-Turn/depth budget. A model cannot reset the root, queue unbounded self-Turns or create a new payer budget. SDK/bridge controls use a stable host-envelope invocation identity for replay (not another model field); ambiguous Process stdin effects are never blindly replayed.

**Interactive Process input is current-authority gated:** an open stdin handle is custody, not a lasting shell grant. Each `jobs_send` to a Process must pass current `process.input` and launcher/source/epoch/resource authority against its *actual original sandbox*. Revocation or narrower current policy blocks new input; it cannot retroactively narrow the already-running OS sandbox. Existing effects remain factual.

## jobs.read

Default view is explicit by kind: Process/Process-backed node **stdout**; Graph/native **result**; Agent **latest committed Turn result**. Read never ACKs, and ACK never touches Agent history.

Process views:
- stdout
- stderr
- result

Graph:
- result
- n:<handle>-specific result/stdout/stderr when valid

Agent:
- result
- messages
- tool_calls
- tool_outputs
- trace

Exact Agent Turn selection uses t:<handle>. Step remains metadata in Turn/tool evidence. Root history uses an opaque advancing cursor, bound to the canonical Agent/Turn, selected **view or filter**, disclosure epoch and intra-record fragment position. Oversized records fragment explicitly rather than silently truncate; model continuation is exactly `more cursor=...`.

For normal reads, a single `view` (`result`, `messages`, `tool_calls`, `tool_outputs`, `trace`) is the shortest selector. An optional **Agent-only `filter`** is mutually exclusive with `view`: omitted or `"default"` selects the latest result; `"none"` selects all currently authorized sanitized public categories; a custom object selects category booleans and optional exact flat tool names, with `exclude:true` subtracting matches rather than including them. One combined read avoids multiple rounds or dumping a full trace. Standalone dotlink never implements or advertises Agent transcript filtering. All projections require current per-category/resource disclosure and never leak private reasoning, system context, credentials or revoked sensitive output.

The internal read result is typed. Model-facing adapters render the shortest safe contextual refs whenever possible; SDKs receive structured typed objects/bytes and may additionally expose canonical_ref without parsing model text.

## SDK

An AW SDK/host **Job-producing** call made without a current Agent Turn explicitly selects an existing authorized Agent owner (`a:0` if already bound and permitted, or another bound Agent). Parsing defaults never create a hidden Agent or grant. Without that owner, a Process/Graph/Agent constructor fails before effects; standalone dotlink uses its own live-session Job namespace.

Typed SDK:

    tools.fs.read(...)
    tools.fs.edit(...)
    tools.bash(...)
    tools.batch(...)
    tools.agent(...)

    tools.jobs.list(...)
    tools.jobs.get(...)
    tools.jobs.read(...)
    tools.jobs.ack(...)
    tools.jobs.send(...)
    tools.jobs.cancel(...)
    tools.jobs.subscribe(...)

Generic SDK:

    tools.call(["fs.read", {...}])
    tools.call(["agent", {...}])
    tools.call(["jobs.get", {...}])

Model-facing Graph keeps DL-150's flat leaf names:

    [
      ["read", {"path":"Cargo.toml"}],
      ["agent", {"task":"Review dependencies"}, "*"],
      ["bash", {"command":"cargo test"}, "1&2"]
    ]

Graph admission maps the same flat model name to the dotted semantic ID below the wire, exactly like a direct tool call. All routes then share PreparedOperation / ToolPolicy / Envelope / journal / executor.

## Deliberately removed

This vNext has no compatibility requirement. Preserve DL-150 correctness while simplifying obsolete Job forms:

- job_start(ops) becomes batch({ops,background:true}); shell job_start becomes the original shell constructor with background:true.
- job_output becomes jobs.read; the remaining job_* controls become jobs.*.
- job_restart/jobs.restart are removed rather than renamed. Re-run means a fresh explicit replay-protected constructor under current schema/authority; retained historical arguments are evidence, not executable authority. This is a pre-1.0 hard cutover **without a migration/drain/dual API**, but a reset is allowed only for an explicitly disposable isolated development/test runtime after verifying quiescence: no continuing external effects, connected clients, MCP Task/replay/ACK obligations or unknown paid attempts. Otherwise abort that reset or use a separately isolated new deployment; never discard unrelated durable Agent/Session state.
- grouped jobs({op:...}) / agents({op:...}), separate agents.*, subagent, team workflow tool, plan, review_release, mode aliases and compatibility-only argument aliases are removed.

Internal crash/startup/replay/private-artifact reconciliation is still required; removing model-facing Job restart does not remove recovery correctness.

## DL-150 inheritance

The canonical new [DESIGN.md](DESIGN.md) now includes a **DL-150 adoption/simplification ledger**: keep the 13 native operations, Graph grammar/status/modifiers, captured per-node ceilings, native platform safety, multi-owner output/ACK, protected transport replay and compact token budgets; replace `job_start` with `background:true`, Job controls with `jobs.*`, and explicitly remove `job_restart` in favor of a fresh authorized constructor. Do not rewrite the sealed original.

Keep the important DL-150 semantics:

- 13 small orthogonal filesystem operations and one-concept-per-tool model selection;
- bounded work, compact deterministic results, no redundant request echoes;
- exact path mutation and atomic mutation commit truthfulness;
- canonical primary outcomes plus orthogonal more/incomplete modifiers;
- directly copyable continuation arguments and atomic-record pagination;
- immutable Graph topology, compact tuple/when grammar and no workflow/dataflow language;
- captured per-node semantic result/work ceilings that outer transport fitting cannot change;
- deterministic native conflict ordering and explicit dependency semantics;
- raw retained-byte ownership/ACK and pure reads;
- replay-before-effect and fail-closed unknown effects;
- fixed public lifecycle vocabulary separate from Graph outcomes;
- exact schema bytes/tokenizer/first-valid-call/round/result-token acceptance gates;
- one shared low-level filesystem/Graph implementation.

New Agent Graph nodes capture a conservative footprint from the **complete delegated authority ceiling**, including native filesystem, shell/process, external/network and child delegation. Opaque effects conflict with potentially interacting Graph nodes automatically. **A Graph Agent node cannot become completely successful or release conflict protection while effectful children it still owns remain active or ambiguously settled**; the node waits for factual quiescence, or reports truthful unknown/incomplete without launching conflicting dependents. Initial vNext forbids implicit effectful detach/transfer inside Graph/delegated completion contracts. This keeps DL-150 bare `when` dependencies correct without another scheduler.

The host `attention_capable` predicate is false for completion-only children and true only when blocking parent request/response is actually delegated. It governs event projection and proactive handoff. Attention-capable children with a synchronously blocked sole parent hand off the same Graph Job (or direct Agent Job) at admission; ordinary child reviews can complete optimistically.

## Permanent crate topology and local bootstrap

The intended reusable crate layout is established **now** rather than splitting a monolithic manager later. `aw-policy` compiles pure scoped constraints/preferences (no tool grants, provider calls or store); `aw-session` defines backend-neutral Session/coordinator evidence atop the existing `aw-store-api`; and `aw-host` composes their decisions with the **one original** `aw-agent-core` Actor/Agent, `aw-tools` Graph/Job primitives and `aw-tools-adapter` concrete executors. The current local AW Agent construction enters through `aw-host::configure_local`, which now also owns **the stock local execution-site/parallelism policy**, eliminating the duplicate root setup. It only narrows a pre-granted embedding ToolPolicy, and repeated scoped policy application cannot re-expand an earlier ceiling. The adapter remains the exact native grant/resource checker. This path adds no guardian/model step, database, collaboration service or alternative tool runtime.

There is **no separate `aw-actor`** crate: `aw-agent-kernel` (portable Actor/Envelope/Turn) and `aw-agent-core` (hosted Agent/Actor lifecycle) already own the relevant invariants. There is **no `aw-tools-core`** crate: the existing `aw-tools` is the shared core. `aw-store-api` already owns reusable backend-neutral Session persistence contracts, while `aw-store` and `aw-store-jsonl` remain the authoritative backends. `aw-session` must not duplicate their durable data or grant checks.

Dependency direction: `aw-cli → aw-host → {aw-policy, aw-agent-core, aw-tools-adapter, optional aw-session}`; `aw-session → aw-store-api → aw-agent-core`, `aw-tools-adapter → aw-tools`. SQLite/JSONL backends are **chosen by the host**, not imported by the Session crate. `aw-policy` stays independent of host, provider, Agent, filesystem and storage. dotlink only consumes reviewed shared primitives; it does not import AW host/session/policy implementation.

Initial **userless, local** mode uses the current host-authenticated local principal, in-memory preference and original ToolPolicy/Envelope and tool adapter. All advanced delegation routes currently fail closed until implemented; no fake `DelegateAgent`, team identity, policy database, second queue or provider call. Full Session sharing, multi-domain policy and Graph/Node delegation grow inside the same crates behind explicit feature/capability checks. See `DESIGN.md` Section 21 and `TODO.md` Phase 1C.

## Implementation boundary

aw-tools remains a **host-neutral primitives layer** for filesystem, Graph/Job semantics, typed ExecutionRef/Revision and generic prepared-operation/route/footprint/policy-decision results. It must **not** interpret domain/team/workspace membership, hold provider/model accounts, maintain a parallel ToolPolicy grant store or decide user-visible execution preferences.

Do not move AW Session/Agent/provider custody into aw-tools.

AW remains authoritative for:
- authenticated principal/team membership and actual target workspace/domain policy scope resolution, including cross-domain checks and fixed canonical :0 defaults;
- one unified scoped Policy compiler producing effective exact ToolPolicy, narrowing Envelope, guardian `Allow/Deny/Ask/Narrow` decisions, execution site/strategy, payer/budget and disclosure ceilings;
- first-class Control authority/effect wiring across kernel/core/policy/Envelope scheduling;
- stable Agent identity, immutable origin and mutable parent/coordinator relation;
- Session/branch/Actor;
- Turn/Step/journal;
- provider/account/model custody;
- Agent Job durable history;
- Agent input/wake;
- transcript disclosure.

Dotlink remains thin around:
- transport;
- sandbox/process custody;
- MCP/OpenAI replay;
- standalone Process/Graph Jobs;
- shared Job control projection.

A real AW bridge may resolve/canonicalize Agent refs and expose Agent Jobs through the same Job control API. Standalone dotlink must not invent durable Agent Sessions. **Remote dotlink-owned Process/Graph Jobs are passive get/read/control sources in initial vNext**: their constructor `wake` and `jobs_subscribe` fail before effects unless a reviewed source-owner-fenced atomic event registration/delivery bridge exists. AW Agent authentication alone is not a remote wake guarantee.

## Non-goals

Do not add:
- a second Agent runtime;
- a second transcript store;
- a second Graph scheduler;
- a universal event database;
- Jobs for every tiny filesystem ToolCall;
- model-visible workflow macros;
- authority-bypassing peer/cross-principal Agent routing;
- hidden chain-of-thought access;
- cross-process live Job resurrection without a separate design;
- a compatibility layer for the superseded vNext drafts.

The implementation cadence is deliberately **checkpointed rather than micro-qualified**: develop a vertical phase with targeted checks, then validate its user-visible happy paths and failure boundaries together. Carry forward passed unchanged primitives and re-run them only when their owner/API/dependency/authority semantics change. The goal is a **single unified policy-and-execution pipeline** built from simple core primitives, not a broader framework surface or separate competing ToolPolicy/ExecutionPolicy engines. Model-efficiency gates compare deterministic Local, opt-in whole-Graph guardian/router, and bounded Node routing only at the relevant checkpoint.
