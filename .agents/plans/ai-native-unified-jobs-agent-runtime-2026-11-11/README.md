# AI-native unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **FINAL DESIGN SEALED / next implementation plan** (2026-11-12). Source implementation has not been performed; unchecked TODO/test gates remain implementation work.

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

## Host execution strategy

AW may execute one admitted semantic operation either locally or through a different ordinary Agent:

    Local
    DelegateAgent

ExecutionStrategy is host-owned and invisible to the model. It does not change ToolCall identity, canonical OperationId, ToolEffect, admitted args, result ceilings or the required factual result/effect contract.

A delegated Agent has its own Agent/Turn/inference identity and may use a different authorized model/provider/tool set. The parent model stays idle while the child runs; when the child settles, AW returns the authorized semantic result as the original parent ToolCall receipt. Delegation provenance remains journaled but is omitted from the normal model result unless useful.

Delegation requires independent Agent/inference/payer/tool/resource authority and consumes the same causal-work budget as explicit child Agent/Graph work. A Read grant never silently buys inference, and delegation never widens the operation's user-visible/persistent semantic side effects; execution mechanisms may be broader only where the trusted implementation contract explicitly permits them. Strategy is frozen before scheduling/replay; direct scheduling and Graph conflict planning use the captured execution footprint rather than semantic ToolEffect alone. Graph performs whole-graph semantic preflight first, then freezes all strategies/footprints, then starts roots.

Transparent delegation is completion-only by default. It never calls back into the synchronously blocked parent model. If request/response attention is required and there is no independent controller, the transparent route is ineligible; use another completion-capable strategy or the explicit Agent/Job handoff path.

This routing/guardian layer belongs to AW, not aw-tools or standalone dotlink.

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

- jobs.list({cursor?,limit?}): flat deterministic discovery of authorized exposed Job roots in the caller's exact authenticated canonical d/i/p namespace; no recursive or cross-principal discovery. Live Agent roots come from authoritative SessionStore creation sequence; finite Jobs use their bounded exposure sequence. The opaque cursor binds exact d/i/p (or standalone runtime generation), listing authority epoch, exposure position and projection version, so it cannot be replayed into another namespace/runtime.
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

New Agent Graph nodes capture a conservative footprint from the **complete delegated authority ceiling**, including native filesystem, shell/process, external/network and child delegation. Opaque effects conflict with potentially interacting Graph nodes automatically, preserving DL-150 deterministic scheduling without claiming transactions for outside services. The captured host `attention_capable` predicate is **false for completion-only children** and true only when blocking parent request/response is actually delegated; the same predicate governs event projection and deadlock prevention. Attention-capable children with a synchronously blocked sole parent proactively hand off the same Graph Job (or direct Agent Job) at admission; ordinary child reviews can complete optimistically without an unnecessary polling round.

## Implementation boundary

aw-tools remains the reusable filesystem + Graph/Job semantics layer. It should gain only genuinely host-neutral hierarchical ExecutionRef/ResolvedExecutionRef, Revision and Job control/read types/traits needed to expose heterogeneous owners.

Do not move AW Session/Agent/provider custody into aw-tools.

AW remains authoritative for:
- principal/project/Agent namespace resolution and fixed canonical :0 defaults;
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

A real AW bridge may resolve/canonicalize Agent refs and expose Agent Jobs through the same Job control API. Standalone dotlink must not invent durable Agent Sessions.

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

The goal is a smaller language with stronger execution semantics, not a broader framework surface. Model-efficiency gates must cover default single-view reads versus filtered combined Agent history, foreground completion-only Agent nodes versus attention handoff, transparent DelegateAgent execution with the parent model idle, self/peer/delegated causal-budget enforcement, and the original DL-150 schema/result benchmarks.
