# Design — AI-native unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **vNext DESIGN UNDER REVIEW + THIN CRATE SKELETON**, 2026-11-12. `aw-policy`, `aw-session`, `aw-host` and the local composition seam exist; full scoped Policy, shared Sessions, delegation and universal Jobs remain proposed. This design is authoritative for design decisions, **not frozen by parallel implementation**. Reconcile interface conflicts here before accepting their implementation checkpoints; do not claim planned APIs are shipped.

The unified policy/execution direction and its invariants are specified at the design level. Implementation and acceptance tests are still required; any discovered ambiguity or change affecting permissions, policy precedence, delegation, tool schemas, reference grammar, lifecycle/replay, Graph semantics or DL-150 inheritance must be reconciled in this canonical plan rather than silently diverging in code.

## 0. Authority and relationship to earlier plans

Live source, tests and executed results remain authoritative for current implementation state.

This document is the newest proposal for the future public/API design. It intentionally supersedes conflicting proposed vNext choices from:
- ai-native-job-control-wake-2026-11-10;
- ai-native-operations-agents-jobs-transcript-2026-11-10.

It does not rewrite those files. Sealed DL-150 remains historical authority for the filesystem/Graph/result/replay invariants explicitly inherited here.

There is no compatibility requirement for the new public API. Implementation may delete old public tool names, aliases and orchestration tools rather than carrying migration complexity.

## 1. Design goals

The surface is primarily for models, while SDK/Graph forms should be pleasant for code.

Priorities, in order:

1. Correct authority, replay, cancellation and effect semantics.
2. One obvious model tool for one model intent.
3. Minimal model schema entropy and result-token cost.
4. One execution/control model for long-running work.
5. Typed structured SDK values; no SDK text parsing.
6. Immutable evidence and fail-closed uncertainty.
7. Shared low-level implementation without moving product-specific custody into core.
8. No workflow macros when ordinary Agents can compose the workflow.
9. Zero compatibility baggage while the product is still iterating.
10. Performance by simple reusable designs rather than additional engines.

## 2. Final public language

### 2.1 Flat model tools

Maximum core model catalog:

    filesystem
      ls
      find
      grep
      read
      write
      edit
      patch
      cp
      mv
      rm
      bread
      bwrite
      bedit

    executable constructors
      bash
      pwsh
      powershell
      batch
      agent

    Job control / observation
      jobs_list
      jobs_get
      jobs_read
      jobs_ack
      jobs_send
      jobs_cancel
      jobs_subscribe

Total: 25.

Only currently available and authorized tools are advertised. For example, a platform without powershell does not advertise powershell.

There is no grouped provider tool with an op discriminator.

### 2.2 Canonical semantic operation IDs

Canonical semantic operation IDs used by SDK generic calls, policy, journaling and internal preparation:

    fs.ls
    fs.find
    fs.grep
    fs.read
    fs.write
    fs.edit
    fs.patch
    fs.cp
    fs.mv
    fs.rm
    fs.bread
    fs.bwrite
    fs.bedit

    bash
    pwsh
    powershell
    batch
    agent

    jobs.list
    jobs.get
    jobs.read
    jobs.ack
    jobs.send
    jobs.cancel
    jobs.subscribe

Naming rule:
- dot separates a resource namespace from its operation;
- underscore separates words inside one field/property;
- atomic concepts need no artificial namespace.

Provider projection is mechanical but intentionally model-flat:

    read       -> fs.read
    edit       -> fs.edit
    agent      -> agent
    jobs_get   -> jobs.get
    jobs_read  -> jobs.read

The provider's original ToolCall name/arguments/call ID remain immutable evidence. Resolved semantic identity is recorded separately.

### 2.3 SDK

Typed SDK:

    tools.fs.ls(...)
    tools.fs.find(...)
    tools.fs.grep(...)
    tools.fs.read(...)
    tools.fs.write(...)
    tools.fs.edit(...)
    tools.fs.patch(...)
    tools.fs.cp(...)
    tools.fs.mv(...)
    tools.fs.rm(...)
    tools.fs.bread(...)
    tools.fs.bwrite(...)
    tools.fs.bedit(...)

    tools.bash(...)
    tools.pwsh(...)
    tools.powershell(...)
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

The generic call is SDK-only, not another model tool.

### 2.4 Graph

The model-facing Graph node remains DL-150's shortest tuple:

    [tool, args, when?]

Graph uses the same flat model leaf name as direct invocation, so the model learns one spelling per action:

    [
      ["read", {"path":"Cargo.toml"}],
      ["agent", {"task":"Review dependency changes"}, "*"],
      ["bash", {"command":"cargo test"}, "1&2"]
    ]

Here `agent` is an intentionally enabled AW model-Graph leaf. The example does **not** permit a `batch` node.

The Graph admission resolver immediately maps the flat leaf name through the same mechanical mapping as direct provider calls (read -> fs.read, etc.) before policy/preparation/journaling. SDK/internal code may continue to use canonical semantic IDs such as fs.read.

Keep DL-150's **compact coordinate, implicit sequencing and `when` grammar**. Immutable Graph topology and deterministic native semantics stay the normal model-facing behavior; no natural-language Graph DSL, uncontrolled dynamic node insertion, unbounded recursion or implicit dataflow.

**Initial provider/model-facing `batch` leaves:** reviewed DL-150 Graph-eligible filesystem/shell operations, **plus `agent`** when AW has the exact Agent/Turn/child-effect owner; standalone dotlink has no Agent leaf and `bread` remains direct-only. **No nested `batch`**, including typed SDK/host Graphs. The shared Graph compiler rejects `batch` as a node during all-leaf preflight. An Agent or its descendant still **owned by an active Graph node** also cannot start a separate `batch` by a direct tool call: the admitted Graph-ownership scope propagates through child Agents/delegates and rejects that constructor before effects. An independently admitted Agent Turn **after** the original Graph scope releases may use normal top-level `batch` authority. No implicit recursion through tool registries or Agent indirection.

`agent` Graph nodes create an ordinary long-lived Agent with one initial Turn and one factual node receipt; the node is complete only after that Turn and its still-owned effectful descendants reach the Section 8 settlement barrier. An Agent may itself create subordinate Agents through authorized `agent` calls (including while running as a Graph node), within the **same shared causal-work, cost, child-depth and ownership/cycle limits**. This is nested **Agents**, not nested Graphs; no second scheduler, unbounded recursive permission inheritance or new model-visible subagent tool. Parent/child relations never grant rights.

Advanced registered leaves such as `jobs.*` remain **explicit typed SDK/host-only opt-ins** where their exact owner/result, Control, replay and wait/control-cycle contracts are implemented and qualified; they never appear in model-facing `batch` automatically. Graph eligibility differs from execution strategy: an operation can still be Local or authorized DelegateAgent even if it is not permitted as a Graph child. Prefer one flat model vocabulary to a general workflow DSL.

### 2.5 ExecutionStrategy as composable primitives — AW host policy, not core restrictions

Semantic identity and execution placement are independent. **Every admitted operation** — the 13 native filesystem tools, bash/pwsh/powershell, `batch`, `agent` and all `jobs.*` controls — supports the same two conceptual execution strategies:

    Local            # execute by the existing exact owner/executor
    DelegateAgent    # coordinate through an ordinary authorized AW Agent/Turn

`Local` is the ordinary AW default: no auxiliary routing/model cost and no change to the direct tool/SDK/Graph wire language. **No operation class is intrinsically Local-only in the reusable primitives.** `DelegateAgent` is a real strategy for any registered semantic operation or whole Graph, including constructors and controls, whenever selected and permitted by the effective host policy. The user's advanced execution configuration may select it globally, per tool/category, per Graph or conditionally. There is no need to register a separate high-level model operation merely to make an existing tool delegatable. Conversely, a tool's normal grant does **not** implicitly authorize paid Agent inference; both the semantic operation and its delegated execution require current exact grants.

**Separation of layers:**
- **`aw-tools` / shared primitive library:** host-neutral immutable Graph/Job semantics, prepared semantic operation/receipt interfaces, execution scopes, bounded route candidates and conservative footprints, dependency/`when` evaluation, factual settlement and replay correlation. The shared scheduler may dispatch work through an injected executor/route callback for any node; it does **not** own an AW Agent, provider, guardian, policy, model, payer or Session. No hard-coded list of operations forbidden from delegated execution.
- **`aw-tools-adapter`:** implements Local dispatch and the bounded host-executor/delegated-dispatch bridge for each registered operation, using the **same** semantic validation, exact result/commit receipt and Job/Graph owner. Reuse one generic prepared-operation dispatch seam rather than 13 hand-written delegation engines. The adapter supplies availability, scopes, concrete execution/custody, resource conflict bounds and truthful failure mapping.
- **AW host/runtime:** selects Local or DelegateAgent, authenticates Actor/Session/Turn/provider/payer/tool authority, implements guardians/routers and user-selected pipeline policy, allocates one ordinary delegate Agent/Turn, records model costs, applies dynamic routing and controls parent/child ownership. No second Agent runner, Graph scheduler, hidden control plane or exposed strategy tool.

`ExecutionStrategy` is **not** a model tool, extra OperationId, ToolEffect, Job kind, intrinsic access grant or alternate result format. User/app configuration is host-owned, validated and narrowing within the actual static ceilings; the model cannot mint route authority, hidden tools or a larger payer allowance.

#### Unified exact semantic contract for both strategies

The common path is:

    immutable original ToolCall / typed SDK / Graph node
      -> authenticated PolicyContext + effective scoped Policy generations
      -> resolve semantic OperationId + schema + original arguments
      -> exact ToolPolicy / Envelope / source/resource/site/payer preflight
      -> reserve original replay claim + guardian/cost budget **before any paid inference**
      -> optional PolicyDecision guardian (Allow/Deny/Ask/Narrow), bound to that claim
      -> trusted authorization + revalidation of any Narrow
      -> permitted Local / DelegateAgent, site/model/payer preference
      -> seal admitted arguments/routes/footprint/decision on original claim
      -> original semantic executor / Graph/Process/Agent/Job owner
      -> immutable factual result + effects/children/ACK/Task evidence
      -> current disclosure and compact projection + parent Step receipt

The original provider ToolCall name/ID/proposed arguments and its resolved semantic operation are separately journaled. The **same exact typed operation and result contract** governs Local and DelegateAgent. A delegated Agent's prose, invented output, or description of calling a tool is never proof of native read/write/mutation, Process exit/stdout, Graph `when` result, `agent` creation or a Job control effect. For any of those, its requested execution is routed through the **existing original trusted prepared-operation executor** with exact arguments/effective authorized revision and one replay identity; only that owner can commit `ok/unchanged/fail/mixed/unknown/skip`, output bytes, ACK or effects. The Agent can coordinate, interpret permitted results and propose authorized next work; it cannot synthesize the semantic effect. The parent receives the canonical original ToolCall receipt, not a surrogate Agent success.

In particular:
- Filesystem `read/write/edit/patch/cp/mv/rm/bread/bwrite/bedit/ls/find/grep`: original native path, atomicity and result adapter; delegated additional calls are **separate** ToolCalls with their own grants/receipts, never silently fused into the requested operation.
- Process `bash/pwsh/powershell`: the original launcher owns the unique Process Job, interpreter identity, sandbox, stdin, stdout/stderr, timeout, cancellation, replay and ACK. A delegate may coordinate it but cannot reproduce its outcome from text.
- `jobs.list/get/read/ack/send/cancel/subscribe`: original resolved ref/owner and disclosure/Control/replay policy remain authoritative; an Agent cannot alter target identity, bypass current Process input permissions, publish a fabricated event or release another owner's output. Transparent DelegateAgent is **ineligible** when the resolved control target is the parent ToolCall's own active Turn/Agent, any unresolved ownership/wait ancestor, or any resource whose requested control cannot settle until the waiting parent Step/Graph node itself settles. This wait/control-cycle check is mechanical over canonical refs plus current ownership/wait edges and runs before delegate admission. Use Local when allowed or an independently controlled/handoff Agent/Job instead; never create a transparent self/ancestor control cycle.
- `agent`: the original constructor allocates **one canonical intended Agent Job/first Turn** under its own replay claim. An optional coordinating delegate is an independently identified helper, not a replacement second Agent creation; its presence cannot duplicate or retarget the constructor.
- `batch`: the original Graph Job/JobManager owns coordinates, `when`, conflicts, native node receipts, retention, cancellation and settlement even when a delegate coordinates the complete Graph. There is no second Agent-managed copy of Graph execution.

A delegated Agent's effective operation authority is the **intersection of the caller's original effective actor/resource/source/site/payer grant and the delegate's current tool/provider/payer/Session bounds**. Its otherwise broader personal/team/workspace grants cannot substitute for a missing original operation grant. An Agent may additionally perform explicitly caller-approved auxiliary operations only under a **separate bounded, independently authorized** delegation/tool envelope, each with its own factual receipt, cost, conflict and causal accounting. Such work cannot claim a different original semantic effect was committed.

**Execution authority and disclosure authority are separate gates.** Delegate context is a bounded, host-authored projection containing only fields/results that the selected delegate Agent **and provider/model** are currently permitted to observe. Exact prepared arguments, commands, secret-derived values, prior node outputs, private transcript/provider context, credentials and unrelated branch state remain sealed inside the trusted executor unless disclosure policy explicitly permits that exact field/category. A delegate may be authorized to request execution of an opaque prepared operation without seeing its secret-bearing payload. If a guardian/router needs protected content to decide, that route/model requires explicit disclosure authority or is unavailable; execution permission never implies model disclosure.

This permits advanced multi-step pipelines without silent privilege or disclosure expansion.

#### Graph delegation shape: whole Graph or dynamic per node

These are **two execution granularities**, not new Graph syntax or Job kinds:

    GraphExecution =
      Local
      DelegateAgent(Graph)    # default when Graph delegation is selected
      DelegateAgent(Node)       # advanced, independently routed nodes

- **`Graph` (delegated default):** one ordinary Agent/Turn receives one bounded **delegate-visible projection** of the fully admitted immutable Graph and coordinates its execution **as one delegated Graph scope**. Exact prepared node arguments/results remain sealed in the trusted Graph owner unless disclosure policy explicitly permits them; the coordinator can operate on coordinates, operation/category metadata, readiness/status and sanitized permitted fields and ask the owner to execute opaque prepared nodes. It may use multiple model Steps *inside that one Turn* when real tool receipts/dependencies require feedback; "one shot" means one Graph-scoped delegate, not an impossible guarantee of one provider inference for sequential dynamic results. It uses the shared Graph manager to dispatch only original prepared nodes, honor `when`, handle results and enforce deterministic conflicts. No new planned nodes/branches/loops, model-controlled bypass of dependencies, reinterpretation of skipped nodes or fake completions. It may perform a graph-wide guardian/routing decision in one separate optional bounded invocation, but **one-pass decision and whole-Graph delegated execution are distinct options**.
- **`Node` (advanced):** the shared Graph runner schedules the immutable nodes normally; each activated eligible node may execute Local or via its own authorized Agent/Turn. A node's route may be chosen dynamically only after its `when` condition becomes true. The route/guardian input is a deterministic **GraphActivationContext**: immutable Graph metadata plus canonical coordinate-ordered public receipts from that node's dependency closure, including explicit `when` references **and DL-150's implicit previous-success sequencing edge when `when` is omitted**. Results from unrelated nodes—even if already completed—are invisible. Skipped nodes require no routing inference/child Agent. The same Graph Job owns all statuses/outputs. The shared representation should be a borrowed/slice view over existing Graph receipts where practical, not a copied per-node transcript.
- **`Local`:** the original shared Graph scheduler dispatches local executors. An advanced host policy may enable a **mixed** Graph with selected nodes delegated even if the overall coordinator is Local; this is the `Node` execution shape. One full Graph does not acquire a new user-visible identity or duplicate scheduler simply because routing varies.

Host-side policy may pick the execution shape and stages (deterministic decision, optional whole-Graph guardian, whole-Graph delegate, activation-time per-node guardian/delegate, or a bounded composition of these). Default for AW is **Local**; when the host/user chooses **Graph DelegateAgent**, default granularity is **Graph**. Per-node dynamic is explicitly available, including the ability to choose Local for particular nodes. Do not hard-code a class-based restriction in shared code or advertise this policy as a new model-facing argument.

A **single Graph-wide guardian/router pass** remains available independently of delegation shape. It can inspect every prepared node once and return per-coordinate guardian verdicts and routing recommendations. Route recommendations are untrusted and may not bypass operation-specific exact grants; one-pass *planning* does not forbid later runtime per-node routing. If runtime routing is configured, the graph-wide result can be a **bounded initial hint/allowed-route envelope**, not an irrevocable prediction for each `when` branch. Decisions made later at activation are separately authorized, replay-correlated and bounded; neither the router nor delegate owns the Graph topology.

#### Preflight, route envelopes and dynamic conflict safety

Graph admission remains **all-node, two-stage and root-free**:

1. Static, side-effect-free preflight of **every** original node: schema and Graph eligibility (including rejecting nested `batch`), immutable coordinates/dependencies/`when`, proposed arguments, original executor and bounded semantic/effect ceilings. Resolve current original-source grants before optional inference.
2. **Before any paid guardian/router or external effect:** atomically reserve the original replay claim, recoverability/exposure slot and bounded guardian/Graph/cost budgets at the existing authoritative owner. Optionally run **one** whole-Graph guardian/router as a journaled child attempt of that claim; capture permitted candidate routes and conservative union footprints for all nodes. Revalidate admitted `Narrow` arguments/authority, seal the Graph and commit decision/approval state before starting roots. Replay of the same original intent reattaches to the claim, never creates a second paid guardian attempt or new Graph.

The shared scheduler plans conflicts using each node's **union of permitted candidate effects/resources**. An unknown delegated Process/network/Agent footprint is opaque and conservatively conflicts with potentially interacting work. At node activation, a configured `Node` dynamic policy may choose **only from that captured candidate set**, revalidate current grants/delegate/owner/payer/writer and atomic replay identity, and narrow the actual footprint; it can never widen an already admitted route envelope or retroactively invalidate earlier scheduling decisions. If an unknown route would defeat a safe conflict plan, fail or conservatively serialize at admission, not after effects. For `Graph`, the single delegate's full captured execution envelope covers all Graph nodes and auxiliary authorized work, and the original shared scheduler still enforces dependencies and owned-effect settlement.

Native exact-operation result authority, existing Job custody and DL-150 Graph outcomes are **independent of route**. In all shapes, complete success of a node/Graph waits for still-owned effectful descendants to factually settle/quiesce; ambiguous external effects never release conflicts or produce a false `ok`. A direct Graph constructor keeps one canonical Graph Job/replay identity; Graph-to-Graph nesting is not admitted. Independently authorized Agents created inside an Agent Graph node have their own stable Agent identities and factual owned-child settlement.

**Whole-Graph decision validation:** one host-private, bounded, typed map keyed by the original 1-based coordinates with `allow | deny | ask | narrow` and optional routing hints/allowed-candidate subsets. Trusted policy validates completeness of required guardian verdicts, provenance, exact per-node authority, narrowing and results; malformed, duplicate, missing required, or unauthorized entries fail closed. Ineligible *actual permissions* fail, but **no operation is ineligible merely due to its kind**. Node omissions from optional route recommendations retain the host's configured default route. Any authorized narrowing is semantically revalidated on the same fixed topology. Guardian decisions do not become node outcomes; paid preparation is factual model usage, not a Graph effect.

**Dynamic per-node admission:** no advance paid routing on branches that skip when `Node` is selected; routing at activation may use a deterministic host policy, a bounded per-node guardian/model, or a shared previously committed whole-Graph suggestion. The shared Graph owner constructs the canonical GraphActivationContext from the node's declared dependency/when closure; the host cannot add incidental scheduler-visible results. This preserves deterministic routing input independent of completion timing for unrelated parallel nodes.

Before any activation-time guardian/router/Ask runs, the shared scheduler claims the node's deterministic conflict-admission position using the preflight **union footprint**. While that decision is pending, conflicting later nodes that would follow it in DL-150's deterministic conflict order cannot leapfrog; unrelated non-conflicting work may continue. If the decision denies/expires, release the reservation. If it selects a narrower actual route, safely release only conflicts proven unnecessary by that admitted route before execution. Router/model/approval latency therefore cannot change conflicting effect order.

Each admitted route decision is journaled exactly once under (Graph Job identity, node coordinate, attempt identity, policy/registry epoch); duplicate dispatch/replay never selects a new delegate or re-executes an already started effect. Any changed/revoked current authority denies before node start; after ambiguous start no silent fallback or double execution. Host caps per-node and aggregate router invocations/tokens/latency and model/child budgets; a policy can select Local without paying for inference.

**Graph delegation:** create/reserve the one coordinating Agent/Turn before any delegated model step or external tool effect; bind it to the existing Graph Job and replay claim, not a second Graph runtime. Child tool proposals are checked as exact immutable prepared node submissions to the original Graph owner (or independent separately authorized auxiliary ToolCalls); the manager decides `when` eligibility and returns the factual node receipt. A coordinator may not submit the same node twice, execute a future node early, or substitute its response for tool effects. If its coordination stops/fails, outstanding work settles truthfully and the original Graph may remain blocked/unknown instead of inventing completion.

#### Routing/guardian invocation, authority and cost

The normal AW `Local` policy is a no-router, no-child hot path. Host/user advanced configuration may enable `DelegateAgent` for any original operation, `Graph` or `Node` for Graphs, and optional deterministic/model guardian stages; these choices must be explicit, bounded, and made under current static ToolPolicy, Agent inference, payer/resource and causal-work grants. A route is not a capability token. When a guarded route requires a model, **the original replay claim and cost reservation exist first**; each auxiliary inference has one persisted attempt identity (distinct from its provider account `InvocationIdentity`), purpose, payer/usage authority and factual/unknown receipt. Matching replay reuses a committed decision or remains waiting/unknown for an ambiguous attempt; it never silently pays for another. A rejected proposal grants nothing.

One whole-Graph guardian/router invocation is preferred over N speculative per-node calls when the host wants a graph-wide review. Runtime per-node inference is an **optional** cost-bearing mode and runs only for activated nodes. The full Graph preflight, model/token/byte caps and outer time limits still apply; compare all-Local, one delegated Graph Turn, and dynamic per-node costs **including all model Steps**, rather than claiming whole-Graph coordination needs exactly one provider call.

**Configuration and policy resolution are unified in Section 2.5.1.** There is one `Policy` schema across trusted scopes, with distinct authority, decision, execution, limits and disclosure fields—not a separate `ExecutionPolicy` config database. AW default: `Local` with no model guardian. When Graph delegation is selected, default scope is `Graph` (one coordinator), while `Node` offers dynamic activation-time routing; the configured decision scope (`Graph` or `Node`) is independent. Effective policy compiles once under exact principal/team/workspace/session/source/site epochs, and every later decision stays inside its captured authority/route envelope. User preferences may select advanced pipelines within host/domain/team/workspace limits but cannot issue grants.

### 2.5.1 One unified Policy → Decision → Execution pipeline

**Normative:** authority, guardian decisions, execution routing, resource placement, payer limits and disclosure belong to **one resolved host policy** and one admission pipeline. Do **not** build a separate unrelated ToolPolicy engine, GuardianPolicy engine, ExecutionPolicy engine, GraphPolicy engine and session policy merge layer. The earlier ToolPolicy and Envelope remain the **actual exact enforcement mechanisms**, while the unified host policy resolves their inputs and the permitted execution options. Different stages may have distinct trusted adapters, never separate sources of truth.

The public provider tools/Graph tuples remain unchanged. User/application advanced settings use one optional host `Policy` shape; pure `aw-tools` only needs generic typed policy context/constraints/decision/route result primitives and injected host callbacks, not domains, teams, provider models, sessions or a policy interpreter.

#### Scope resolution: context, subjects and resources

One authenticated `PolicyContext` begins with trusted subject/resource ownership **before** semantic admission, then binds the exact canonical OperationId/source/effect/site after semantic resolution and before policy authorization. It supplies immutable identities and resource owners:

    PolicyContext {
      actor: authenticated principal i:<handle>,   # who requests
      domain: d:<handle>,                         # execution/resource domain
      team?: verified team membership/context,    # separate relation, not a ref segment
      workspace: canonical d/i/p resource owner,   # where target project lives
      session?: exact Session/branch/Agent ref,    # which ongoing authority
      graph?: original Graph Job identity,
      node?: immutable 1-based coordinate,
      operation: canonical OperationId + source/effect/site/epoch,
      invocation: authenticated source/turn/call/replay identity
    }

The actor identity and the **target resource owner** are not interchangeable: an authenticated caller does not rewrite `i:0` or acquire a workspace merely by naming its canonical ref. `team` is an authenticated membership/role relation, **not** another `t:` path segment (reserved for Turn). Workspaces/projects use existing canonical `d/i/p` ownership; team-owned or shared workspaces still have explicit owner identity, membership grants and resource policy. The context includes both the executing/source domain and any separately addressed target domain; **cross-domain work must satisfy both** source and target authority, never an ambient inherited default.

Policy configuration is attached to these real, auditable scopes:
- **Host/runtime:** absolute provider/launcher/sandbox/resource ceilings and installed source bindings.
- **Domain:** domain-wide grants, explicit denies, execution sites, model/provider allowlists, privacy/data and resource/payer ceilings.
- **Identity and verified team membership:** identity-specific or role-based positive grant paths, team grant constraints, budgets and route preferences. A team membership **never becomes a grant** without an authorized issuer/role and applicable scope; losing membership invalidates the path.
- **Workspace/project:** resource-owner boundaries, exact filesystem/network/extension permissions, optional team-sharing rules, allowed execution sites, provider/payer limits and defaults.
- **Session/Agent:** bounded selected tools, allowed delegate/guardian options, model/payer preferences, interactive wake/Control grants, result-disclosure preferences.
- **Graph/Node/ToolCall:** optional one-use narrowing, guardian/route overrides and work/result/child limits under all parent ceilings. Original provider calls remain immutable evidence.

The above are **intersecting policy dimensions**, not a false tree. A user may belong to several teams, and a workspace may be team-owned. **Positive grants** are allowed when at least one authenticated identity/direct or currently valid team-role grant path authorizes the *exact* operation/source/effect/site/resource; a team path carries its own membership, workspace and role restrictions. Positive paths may union where an issuer explicitly authorized them, but **all applicable mandatory host/domain/resource/workspace constraints and explicit denies still apply**. A second team cannot launder a denied resource or bypass the selected team context. When a team context is explicitly selected, only that team path and any independently valid direct-identity grants participate; no accidental implicit rights from unrelated memberships. Ambiguous team/owner membership or stale revocation evidence fails closed.

#### One Policy shape; authority and preferences have different merge rules

Conceptual **one host-side policy configuration**, not a new mandatory wire schema:

    Policy {
      tools?: { deny?, allowed_sources?, effects?, sites?, resources? },
      # positive grants are NOT ordinary Policy fields at runtime; see GrantPath below
      decision?: { guardian?: None | Deterministic | Model,
                   scope?: Graph | Node,
                   rules?, approval? },           # host rules, not a fabricated Allow
      execution?: { default?: Local | DelegateAgent,
                    delegate?: Graph | Node,      # default Graph when delegated
                    timing?: Admission | Activation,
                    required?: boolean,           # unavailable => deny if true
                    fallback?: Local | DelegateAgent, # permitted before effects
                    operation_overrides?: {...},  # preferred route/model/site/role
                    provider?, model?, site?, payer? },
      limits?: { model?, tokens?, calls?, children?, depth?,
                 concurrency?, graph?, result?, time?, cost? },
      disclosure?: { allowed_views?, redaction?, retention? }
    }

    AuthenticatedGrantPath {                  # host/store evidence, not user preference
      issuer: PrincipalRef,
      issuer_authority_epoch: Revision,
      grant_scope: canonical scope/resource,
      tool_binding_or_grant_ref: opaque stable proof,
      restrictions: bounded exact source/effect/site/resource ceilings
    }

All fields are optional at each scope. Defaults are **no new grants**, no model guardian, `Local` execution, and bounded original DL-150 resources. The same field semantics work at host/domain/identity/team/workspace/session/Graph/Node: no different "workspace policy language" or "session policy language". Implement a minimal typed interface rather than a generic arbitrary-expression/JSON policy DSL; only add concrete fields backed by real enforcement.

**Policy publication authority:** the host controls root ceilings and source registrations; an authorized domain administrator may issue scoped domain/team-role/workspace grants **only within host ceilings**; a team administrator may issue role grants only if explicit domain authority delegates issuance; a workspace owner may issue access/resource grants **only for that workspace** within domain boundaries; an identity may set its own permissible preferences or narrow itself, not self-issue a new workspace/Process grant; Session/Agent/Graph/Node callers can choose/narrow effective preferences within current authority, never mint grants.

Publication of a positive grant is a separate authenticated host/store operation that emits an `AuthenticatedGrantPath` (or equivalent sealed host evidence) tied to issuer, issuer-authority epoch, resource scope and the existing ToolPolicy/ToolBindingAuthority grant. **The pure `aw-policy` crate does not create, authenticate or infer positive grants from `PolicyLayer` data.** It composes constraints/preferences/routes/limits only. At admission, AW resolves currently valid authenticated grant paths from authoritative stores, then intersects them with pure policy constraints and materializes the exact ToolPolicy/Envelope. A preference/Session/Graph/Node policy object can therefore never be deserialized or constructed as a grant merely because it contains similarly named fields.

Every policy/grant update authenticates both policy resource and issuing principal and advances the relevant generation; identity/team membership and workspace ownership changes invalidate affected future grant paths. Reuse existing host configuration and authoritative Session/resource stores for persistence, with no independent PolicyStore/database or new model tool.

**Authority resolution (mandatory, never last-writer-wins):**
- Source-bound tool permissions, static `Envelope` and ToolPolicy grants, exact principal/site/epoch, filesystem/process/network/OS sandbox, data disclosure and payer/provider authority are mandatory **conjunctions** of applicable ceilings and verified grant paths. **Any explicit deny from any applicable host/domain/identity/active-team/workspace/Session/Graph/Node scope wins**; a missing source grant is not `Allow`. Empty intersection is denied.
- Upper bounds (time, bytes, tool/model rounds, fanout, child depth, Graph work, tokens, payer/cost, concurrency, retention) use the **tightest applicable ceiling**, and multi-resource budgets are **atomically reserved/debited** at the authoritative owner; never reset by creating a Session, Agent, Graph or team context. Limits cannot be made larger by a lower-level preference.
- Distinct resource policies restrict only their actual scoped resources; irrelevant team/workspace policies cannot accidentally disable an unrelated operation. Source and target domain constraints both apply to cross-domain work.
- Delegation/guardian are **additional capabilities**, not substitutes for an original tool grant: the original operation still needs exact tool/source/effect/site/resource authorization; the chosen delegate/guardian additionally needs independent Agent/provider/model/payer/tool/context rights. `Control` is separate from `Read` or filesystem `Write`, as already sealed.

**Preference resolution (configurable within the above):** choose the closest **explicit** permissible preference in this stable order:

    Node / ToolCall > Graph > Session > Workspace >
    active Team > Identity > Domain > Host default

For a request without an active team context, team membership can supply authorized grant paths but **does not supply an ambiguous implicit team preference**. Preference conflict does not union incompatible values. If the preferred route/provider/site is forbidden by the effective authority, select the first **explicitly allowed configured fallback** (normally `Local`) before execution; an explicitly **required** strategy/site instead fails closed. Never silently reroute after an effect or paid model attempt is admitted/ambiguous. An operation override is keyed by trusted semantic OperationId/source, not untrusted descriptions. Graph `Graph`/`Node` scopes are distinct from the current Agent execution identity.

#### Canonical PolicyDecision, not parallel policy engines

The policy compiler yields one immutable/epoch-fenced `EffectivePolicy` for an exact invocation:

    EffectivePolicy {
      context: exact source/actor/domain/team/workspace/session/ref,
      authority: exact allowed ToolPolicy + Envelope/source/site/resource ceilings,
      candidate_routes: bounded permitted Local/DelegateAgent (+ Graph or Node),
      decision_stage: configured guardian and timing,
      limits: tight resource and shared payer/causal reservations,
      disclosure: current projection restrictions,
      revision: all relevant host/domain/identity/team/workspace/session/
                tool-source/registry generations
    }

The materialized `ToolPolicy` remains the source-bound exact grant predicate in `aw-agent-core`, and the portable `Envelope` remains its narrowing-only Actor ceiling. AW's current `ToolRevisionPolicy` is the trusted semantic-authority check for **revised** ToolCall arguments; it is not replaced with permissive "remove JSON fields = narrower" inference. `LocalToolOptions`/filesystem access and sandbox settings contribute actual root/site/resource ceilings rather than independent authority. Do not add a second registry of grants, policy database, separate policy evaluator in dotlink or duplicated Session execution state.

One `PolicyDecision` is evaluated for the original prepared operation (or one bounded coordinate-keyed result set for Graph admission):

    Allow | Deny | Ask | Narrow
    + permitted execution route / target / site / payer preference

A guardian/decision model is **advisory evidence**. `Allow` means the decision stage passed, **not** that a missing grant was created. `Deny` blocks execution. `Ask` creates one replay-stable **admission wait** bound to exact invocation/Graph/node identity, policy revision, approval scope, authorized approver set and expiry; it never executes on a guessed approval or starts a second policy loop. `Narrow` for direct calls or admission-time Graph decisions produces one explicit revised candidate that re-runs semantic/schema/revision/ToolPolicy/Envelope/resource checks and is admitted only if the trusted host proves semantic narrowing; it cannot mutate the original provider ToolCall or grow ceilings.

For an already admitted/running Graph, activation-time Node decisions are stricter: `Allow | Deny | Ask` plus route/resource/provider/payer/limit narrowing only. **Node-time `Narrow` cannot revise prepared node arguments or semantic identity.** In a Graph-wide admission pass, a required deny/ask blocks roots; in a Node activation pass, deny/ask blocks that node and dependents without falsifying already committed independent results.

**`Ask` liveness and settlement:** approval waiting reuses the existing owner/Job/Turn wait machinery; it is not a new Job kind, event bus or policy engine. The authoritative owner records immutable pending-decision evidence under the original admission/replay identity.

For a single direct/Node decision:

    PendingApproval {
      approval_id,                         # deterministic from owner/admission/target
      target: Direct | Node(n),
      policy_decision_revision,
      approval_scope,
      authorized_approver_set,
      expiry,
      state: Pending | Approved | Denied | Expired
    }

A whole-Graph guardian may return **multiple coordinate-level Ask verdicts**. The existing Graph Job owns one bounded canonical set, not another approval service:

    PendingApprovalSet {
      graph_ref,
      admission_identity,
      guardian_decision_revision,
      entries: [PendingApproval ...]       # canonical target/coordinate order
    }

Rules:
- one entry per target/coordinate; duplicates are invalid;
- each entry has an immutable deterministic approval_id and resolves idempotently exactly once;
- matching replay returns the same pending set and current entry states; it never reruns the guardian or allocates another set;
- approvals may arrive independently/in any order, but Graph admission resumes only when **every required Ask** is Approved;
- any required Denied/Expired entry, invalid current authority, or stale approval scope makes the Graph admission fail without roots;
- the set is bounded by the existing Graph node limit and retained in the Graph owner's admission state, not a new database.

Direct ToolCall/SDK Ask: no effect starts; owning ToolCall/Step waits on its single approval entry. If synchronous response disposition cannot service the approver without blocking the sole controller, hand off/expose the same owner first.

Graph admission Ask: reserve/expose the same Graph Job/replay identity, mark it waiting, start **zero roots**, and retain the canonical PendingApprovalSet. When all required approvals are satisfied, resume the same root-free preflight under current authority.

Activation-time Node Ask: only that exact node attempt waits on one approval entry; already committed independent results remain factual, dependents cannot start, and approval resumes the same attempt/route envelope.

Current authority is revalidated at approval consumption. Approval is not a bearer grant, cannot outlive scope/expiry/revision, cannot widen arguments/routes/resources, and cannot revive revoked base authority. If servicing Ask would require the synchronously blocked requester/model itself to run another Step, proactively hand off the same owner or reject foreground rather than deadlock.

Tiny host-neutral pending/wait value types may be shared where Process/Graph/Agent owners all need them; approver identity, membership and authorization remain AW-host policy concerns.

There is **one policy pipeline** for provider, typed SDK, Agent internal calls, host actions, direct tools, Graph admission and Graph node activation. **On redelivery**, authenticate the original invocation identity and check its claim **before** contextual ref resolution or fresh policy/effect admission; return the accepted/pending/unknown receipt under current disclosure. **Only for new invocations** follow:

    authenticate principal + membership + exact source/target resource
      -> resolve applicable scoped Policy layers / compile EffectivePolicy
      -> resolve original semantic OperationId + ToolCall/typed args
      -> static ToolPolicy + Envelope + resource/site/payer preflight
      -> **reserve original replay claim + bounded approval/model/cost/recoverability budget**
      -> configured guardian decision(s), if any, journaled under that claim
      -> authorize Narrow/Ask/Deny and any changed arguments
      -> choose permitted Local / DelegateAgent, execution site, model/payer
      -> capture Graph union route footprint or exact direct execution footprint
      -> seal final accepted arguments/route/decision on the original claim
      -> dispatch via original prepared semantic executor / Graph Job owner
      -> factual receipt, output/ACK, usage and policy provenance journal
      -> publish sanitized result under **current** disclosure policy

**Default hot path** has zero guardian/model routing and no repeated policy walks: compile a bounded immutable view and make one source-bound authorization/dispatch decision. The policy/guardian output is small typed data; no policy reasoning is copied into model tool schemas or normal result output.

#### Graph and dynamic pipeline application

- At Graph admission, authorize **all** immutable nodes' semantic/effect ceiling using the same effective scoped policy; prepare an envelope of **every allowed candidate route** and conservative **union of execution footprints**, even if Node routing will be delayed. The host may perform **one optional whole-Graph guardian decision** for every node. A proposal cannot add a grant or a Graph node, change original `when` or increase result ceilings.
- `DelegateAgent(Graph)` (default when Graph delegation is chosen) uses **one coordinating Agent/Turn** for the Graph; its provider/role/site/payer remain bounded by the captured effective policy. `DelegateAgent(Node)` chooses Local or Agent at node activation after `when` and current-policy recheck; skipped nodes pay no routing inference. The one-shot guardian choice (`Graph`) is independent of execution choice (`Graph` or `Node`).
- Dynamic Node routing/guardian may use only the canonical GraphActivationContext: authorized public receipts in the node's declared dependency/when closure plus current policy. It can choose only within **captured permitted candidate routes** and cannot widen preflight's union conflict footprint/limits. **Prepared Graph node arguments are immutable after admission.** Admission-time Graph guardian may `Narrow` arguments before the Graph is sealed and before any root starts; activation-time Node policy may only Allow/Deny/Ask or narrow route/resource/provider/payer/limit ceilings. It may not revise node arguments, operation identity, dependencies, `when`, topology or semantic work/result ceilings. A policy change requiring any such mutation fails rather than replanning during effects.
- A Graph coordinator or Node delegate cannot approve its own denied tool, mint a new payer or bypass an existing `Ask`. It may submit eligible node/tool requests only through the original trusted prepared executor and exact JobManager receipts.
- Routing the operation through an AW host to dotlink changes the **execution site** only when explicitly granted. `Local` means direct execution by the selected original owner, **not necessarily the same physical machine**; `DelegateAgent` means model coordination, **not** an automatic site transfer. Domain/workspace permissions decide which sites are allowed; dotlink's launcher/sandbox must recheck its concrete target resource authority.

#### Live changes, replay, audit and lifecycle

At policy publication, update one **immutable policy generation/version vector** of all scopes that influenced authority (including team membership, resource owner, source epoch, registry, session/Agent writer and site). Repeated reads of unrelated static settings can be cached by that exact key, but a cache **cannot become an authority** after revocation; where current membership/source/resource validity cannot be proved, fail closed. Assemble a coherent snapshot (or validate its scope versions before admission) rather than mixing a stale team grant with a fresh workspace policy. Do not require a new global transaction manager: the authoritative AW binding/Session/owner remains the commit point for its own effects and can atomically compare applicable revision fences.

**Captured ceilings persist; current authority may only narrow:** immediately before every effectful node, Agent Turn claim, Process stdin write, delegated child, `jobs.send`/`cancel`/`subscribe`, and current transcript read, recheck actual authoritative grants/epoch and owner resources. New preferences may affect **new** work, never silently change an admitted operation's model, site, delegate or replay target. Revocation prevents new starts and factually cancels/blocks where possible; an in-flight external effect with unknown stop remains `unknown` until reconciled.

**Cross-host Start linearization (AW → dotlink):** an upstream policy envelope is **not** a reusable execution grant. Before each new remote effect or Process stdin delivery, the original AW authority owner atomically rechecks actor/grant/source/site/resource/membership/policy/writer epochs and records **one** replay-bound Start authorization. The proof binds `(AW origin + original invocation, exact effect-attempt ID, resolved operation/ref/args digest, intended dotlink host/profile, authenticated dotlink live-runtime generation, current source/site/writer epochs, bounded expiry)`. It is not a transferable or reusable privilege. Dotlink atomically claims that exact attempt through its **existing** native replay/owner ledger before external effect, then rechecks local launcher/sandbox/source restrictions. Duplicate delivery in the **same** live generation returns the same factual/pending/unknown receipt, never a second effect; replay after generation change rejects the old proof even if the external effect was ambiguous. AW must not issue a replacement Start for the same uncertain original attempt, whether dotlink restarted or the reply was lost. Query/reconcile the original receipt or preserve `unknown`; a deliberate fresh invocation is a new independently authorized intent. Proof expiry must not outlive the remote dedupe claim; finite claim/tombstone capacity is reserved before Start. Old live Jobs are not resurrected across dotlink restarts.

The AW Start commit is the **authorization** linearization point: revocation committed before it denies, while a Start accepted before revocation may physically launch later under its same-generation, unexpired proof. Stronger no-launch-after-revocation semantics require fencing/draining accepted Starts before claiming quiescence. **AW disconnection blocks new Start authorization**, but does not by itself invalidate a proof already committed and delivered to the same live remote owner; that attempt may start or return its existing receipt within its bounded lifetime. On missing/stale/mismatched proof, remote generation change, ambiguous prior Start or unavailable local authority, do not launch **new** work. Already started/uncertain effects remain factual. No shadow AW membership database, new event service or extra standalone Local-path network hop.

**Replay lookup precedes contextual ref resolution on redelivery.** Authenticate the original transport/Turn/ToolCall or SDK request origin and stable invocation ID, then look up the owner claim by that **context-independent identity**. If found, compare the original wire request (including literal `requested_ref`, operation and canonical argument fingerprint) with the stored request; reject mismatches, and use the **stored `resolved_ref` and accepted revision/arguments/route**, never re-resolve `^`, `.`, structural overlays or defaults. Revalidate **current disclosure** before returning its original accepted/pending/unknown receipt; never reauthorize or repeat its effects. This also covers parent/membership/namespace changes after admission.

For a **new** invocation only, resolve the requested ref once against an authenticated coherent context, authorize it and atomically claim `(origin, invocation ID, original request fingerprint, requested_ref, resolved_ref)` **before paid guardian/Start/native effects**. Store policy/registry generations, revisions, `Narrow` arguments, route, payer and factual external attempts as evidence **inside** that claim—not new lookup keys. The first claim and its target resolution share the owner's admission boundary. Retired/ambiguous old identities whose replay safety cannot be established fail closed rather than being reinterpreted as fresh effects; new intent requires a fresh invocation ID. Use the same ordering for `jobs.send/cancel/subscribe`, constructors and SDK/provider/bridge control. No second replay engine.

**Team/workspace example:** Domain `d:acme` restricts provider/site and budget; identity `i:alice` holds a verified `team:engineering` role, granting workspace-read on owner `/d:acme/i:service/p:repo`; workspace policy denies remote network and caps Graph concurrency; session chooses `DelegateAgent(Node)` and a smaller cost cap. Effective decision: read through an authorized delegate is possible only if *both* the team-role grant path and workspace/domain/Session bounds permit the exact tool/source/site/model/payer. A write not granted to the actor remains denied even if the delegate has broader personal rights. The session's Node preference does not widen any grant, bypass workspace denies or change the canonical resource owner.


### 2.5.2 Session sharing and multi-user coordination — one Agent runtime, layered primitives

**Normative:** Session sharing changes **authorized audience, input attribution and participation rights**, not the Session's owner, the coordinator Agent's immutable identity or the execution runtime. **Never automatically create a team user/service principal, new master Agent or second coordination engine on the first invitation.** Team is an authenticated membership/Policy scope, not a synthetic person or Agent kind. AW SessionStore, the existing one-writer Agent/Actor/TurnJournal, and shared Graph/Job semantics remain authoritative.

This design covers private local/domain sessions, personal sessions shared with other identities, independently isolated shared contexts and genuinely team-owned projects. The number of participants is **not** the unit of Agent allocation.

#### Separate ownership, participation, execution and coordination

Four independent identities/relationships must not be conflated:

- **Resource owner:** existing canonical d/i/p workspace and Session ownership; controls history, resource policies, membership administration and disclosure. Owner ≠ current human caller.
- **Participant/initiator:** authenticated human `PrincipalId` who submits an input or requests Control. Each input/tool proposal retains that initiator and its original request/epoch.
- **Coordinator:** one **authoritative Agent binding for each executable Session/branch** with one current Turn writer. Multiple participants submit to **this same Agent Input queue**, not independent Agent copies.
- **Coordination parent:** optional authorized Agent parent (`^`), distinct from owner, creator and actor; a relation never grants permission or cancellation authority.

The reserved `a:0` is **one stable root/default Agent identity per d/i/p project**, not one per participant, Session, team or domain. Section 7.1 binds each Agent to **exactly one existing Session + branch + writer for that Agent identity's entire lifetime**. The first/main executable Session may lazily bind `a:0` **once**. That binding is never reused or rebound, even sequentially after the Session/branch closes or is retired. Closing/retiring that primary Session closes/retires `a:0`; any later independently executable Session/branch gets a fresh stable `a:<handle>` and may optionally use the still-historical `a:0` only as provenance, never as a live coordinator. If the product later needs a permanent project object independent of Session lifetime, add a distinct project resource rather than overloading Agent identity. A viewer, inactive branch, unused project or domain policy requires no running/paid coordinator.

| Case | Owner | Coordinator | Additional identity/Agent on share |
| --- | --- | --- | --- |
| One user locally or in a domain | Existing personal d/i/p and Session | Existing `a:0` in primary Session (other Session Agents only when used) | **None** |
| Personal Session fully shared with Bob | **Unchanged** personal owner/Session/branch | Existing bound Agent and Turn writer | **None** |
| Only selected personal context should be shared | Existing personal project, **new approved-context Session** with distinct audience | A new `a:<handle>` only when that new Session executes | No team user; extra Session/Agent exists for **privacy isolation**, not invitation |
| Team-owned project / Session | Existing team-governed workspace canonical resource owner | Its project `a:0` in primary Session; other Agents for independent Session branches | No per-member coordinator or fake team user |

A domain and team are **policy/resource scopes**, not always-on master Agents. For a team-owned workspace, an explicitly provisioned domain-managed **resource/service principal** may own shared resources or unattended execution, but only by deliberate authorized creation; it is never a stand-in for a human or automatically minted by a share. Sharing and changing resource ownership are separate operations.

#### Session coordinator binding and input admission

Conceptual host/store evidence (not new model tools, and do not mandate these exact Rust struct names):

    WriterFence =                       # tiny portable aw-agent-kernel primitive
      NotApplicable
      | Epoch(NonZeroRevision)

    SessionCoordinatorBinding {
      session: SessionId,
      branch: BranchId,
      agent: AgentRef,                   # immutable a:<handle>
      owner: CanonicalResourceOwner,
      agent_writer_fence: WriterFence,   # exact Session/branch Actor/Turn writer binding
      workspace_writer_fence: WriterFence, # independent Workspace writer fence
      binding_revision: NonZeroRevision
    }

It is **an extension of existing SessionStore/Agent binding evidence**, not a second Agent identity/Session state database. `NotApplicable` means the authoritative backend genuinely has no such multi-writer fence; it never means "required fence missing/unknown." Durable or multi-writer backends must require `Epoch`, while a single-owner local backend may explicitly use `NotApplicable`. Keep Agent/branch and Workspace writer fences distinct; they cannot substitute for each other or collapse into one stale integer. Bind the exact Agent/branch and all required fences **before any paid inference**. The Agent↔Session/branch binding itself is immutable once established; changing coordinator means allocating/binding a **new Agent identity**, never rebinding the old one. Share membership changes leave existing canonical binding identity unchanged.

Every ordinary shared submission is admitted into the **existing single Agent Input queue**, retaining durable order and exactly one active Turn/branch writer. Submitting while another human's Turn runs defaults to **queue**, not implicit steer/interrupt. `steer`, `interrupt` and `cancel` require independent current Control authority, exact Turn ref/rev and an authorized requestor; merely being able to submit does not give one participant control over another's running work. Existing Graph/child Jobs may run independently within their exact granted authority and conflict plan, never as another concurrent model writer on the same branch.

**Every model Step belongs to exactly one immutable authority segment.** An authority segment begins at one authenticated external/host Input or accepted explicit Control Input and persists across every causally following model Step in that Turn—including Steps triggered by tool receipts—until another authenticated Input/control event establishes a new segment or the Turn terminates.

The TurnJournal stores one compact segment record and Steps reference it rather than copying a full policy object repeatedly:

    AuthoritySegment {
      segment_id: AuthoritySegmentId,        # internal stable id / request-derived identity
      origin_input: InvocationId,
      actor: PrincipalRef,
      payer: AuthorizedPayerRef,
      causal_root: InvocationId,
      membership_epoch: Revision,
      audience_epoch: Revision,
      provider_model_ceiling_ref: opaque stable policy/evidence ref,
      tool_envelope_ref: opaque stable ToolPolicy/Envelope evidence ref,
      disclosure_projection_ref: opaque stable projection/evidence ref,
      context_fingerprint: Digest
    }

    StepAuthorityContext = reference to one AuthoritySegment
                          + current narrowed revisions/ceilings required for this Step

All ToolCalls proposed by a Step are authorized from that Step's segment/current StepAuthorityContext, never from Session owner, an older Step actor or participant union. A queued Input starts a new Turn with a new segment. A same-Turn `steer`/`interrupt` starts a **new authority segment** from the controller Input; every subsequent Step/tool-receipt continuation remains in that segment until another authenticated control/input boundary replaces it. It never silently reverts to the Turn's original actor.

Before a new segment begins, AW proves that the continuation context can be safely projected for its actor. If actor/disclosure changes, provider input is rebuilt from the authorized public Session/Turn projection; provider-private continuation/cache/state from the previous segment is never reused across that boundary. If required context is not disclosable, reject steer/interrupt and require queued/new Turn or approved-context Session. Control redirects execution; it never borrows another participant's secrets, payer or tool grants.

Provider/account `InvocationIdentity` remains the existing provider credential/account identity. **Do not overload it with Session/Turn authorization.** Instead each paid/model attempt has a lightweight immutable admission record:

    ModelInvocationAdmission {
      agent_ref,
      turn_ref,
      step_ordinal,
      authority_segment_id,
      authority_context_fingerprint,
      invocation_identity,                 # existing provider/account/model identity
      host_invocation_id
    }

Replay/retry may reuse a paid/provider attempt only when this complete admission identity matches. A different authority segment/fingerprint is a different admission even if Turn, model, text or provider account are otherwise identical.

    ParticipantInput {                    # host-owned immutable provenance
      actor: PrincipalRef,                # authenticated human / requesting service
      session: SessionId,
      branch: BranchId,
      request_id: InvocationId,
      membership_epoch: Revision,
      audience_epoch: Revision,          # shared history/CRDT disclosure generation
      expected_branch_revision?: Revision,
      expected_workspace_writer_epoch?: Revision,
      mode: Queue | ExplicitControl,
      causal_root: InvocationId,
      payer: AuthorizedPayerRef
    }

This is a **factual admission receipt, not an executable bearer grant**. Session membership/Action, branch writer/revision, Agent state, original participant ToolPolicy/Envelope, workspace/domain/team policy, provider/payer and causal budgets are checked at input acceptance, again when queued Input is claimed for a Turn, and before subsequent effects. An already accepted queue item cannot use subsequently revoked authority. The same request ID on replay returns the original accepted receipt; identical text under a new ID is a new submission. No collaboration-specific input queue or second Actor runtime is needed.

#### Share permissions and history audience

Sharing uses independent Session participation **capability predicates**, built from existing SessionStore membership, draft grants and unified scoped Policy (conceptual names, not new public tools):

    View         read the authorized shared Session history/results
    Contribute   edit/suggest an authorized shared draft/artifact
    Submit       submit an authenticated new Agent input
    Control      explicitly steer/interrupt/cancel/Job-control when granted
    Manage       manage participants, audience and coordinator configuration

These are **not** filesystem/Process/model/payer grants. Read, Contribute or Submit cannot imply Control or free paid inference; a user can be invited to view without running an Agent. Existing `members.can_append` continues to govern its legacy append APIs; `session_draft_grants.can_submit` and sealed draft submissions are **not** generic Session-append/tool authority. The unified Policy resolves each submitted action with its real operation/effect, source/site, caller and target branch.

Sharing scope is explicit:
- **Full Session:** the owner elects to expose the **currently authorized** existing Session history and required context dependencies to the invited audience. Keep its Session, owner, branch and Agent unchanged. Preflight and report the audience/history/dependency scope; page results only under current reader-specific disclosure.
- **Approved context only:** create a **new shared Session** within the still-personal workspace from a vetted sanitized checkpoint/authorized export, with its own audience, and optionally bind a new Agent when execution begins. Do **not** assume a new branch of the private Session provides privacy while membership/history is Session-wide. Never carry private prompts, hidden provider context, personal memory, credentials or unauthorized tool outputs into the new Session by default.

An existing bound Agent can continue only if the shared context and prior history it might use are safe for that audience. A participant's Session access does **not** give the Agent a combined union of participants' secrets. Model context for a multi-user Turn is an **authorized shared projection**, not the owner's unrestricted private history or the last participant's private context. If that projection cannot be proven, decline the full share and offer approved-context isolation instead.

#### Multi-user authority, payer and unattended execution

For each participant-initiated Turn and subsequent ToolCall, effective authority intersects:
- the **initiating caller's** current direct-identity or verified team-role grant path and membership epoch;
- the current Session participation capability, audience scope, branch writer and coordinator binding;
- the coordinator's own configured ToolPolicy/Envelope, provider/model/route/site limits;
- target workspace/domain/resource/ToolSource grants and active denies;
- the explicitly authorized payer/account, provider and finite causal-work budget.

**Never union all participants' rights.** Bob's input to Alice's shared Session cannot use Alice's private filesystem grants or personal provider account merely because Alice owns the Session or its Agent. Likewise, a team coordinator cannot execute with the combined permissions of all team members. The original operator and its current grant path are carried through delegated Agents, Process tools, Graph nodes and remote hosts; the delegate's separate permissions cannot enlarge the requesting operation's grant.

For every paid/effectful attempt, journal **requested_by**, **executed_under** (actual authenticated human or explicitly authorized service principal), **charged_to**, exact resource owner/branch, tool source/site and invocation/replay identity. These are attribution facts, **not new owner IDs**, and cannot be inferred from a chat message or a model's claimed author. A Session share never silently changes payer, provider credentials, execution Site or background automation scope.

**Unattended/team automation** is an explicit separate admission through a *real*, domain-managed service/resource principal with its own current workspace/tool/provider/payer grant and renewal/revocation epochs. It does not impersonate a member, and neither sharing nor leaving the UI starts background work under the last human's credentials. If a human-origin queue item loses current authority before execution, do not switch it to a stronger member or service principal without a fresh independent authorized request.

#### Membership, privacy, writer and ownership lifecycle

Session audience/member grants, team-role membership, resource ownership, coordinator binding, Session branch/Workspace writer epochs and inherited Policy generations are authoritative host/store facts. Updating a Session audience must atomically:
- authenticate the issuer's exact **Manage** right and target Session/branch;
- commit the membership change with a bounded monotonic audience/membership revision and audit event;
- rotate/invalidate any active shared-draft/CRDT epoch that could expose the old audience, consistent with existing AW multiplayer behavior;
- invalidate affected input claims, disclosure cursors, notifications/paid wake admission and future ToolCalls, while keeping original historical receipts immutable.

Removing Bob immediately prevents new View/Contribute/Submit/Control/Manage under his revoked scope. Queued Bob Inputs remain factual accepted evidence but cannot claim a paid Turn without reauthorization; active work checks current authority before further effects, and possibly started external work remains `unknown` until reconciled rather than being falsely undone. A revoked member cannot rely on cached Session history pages, old branch handles, previous coordinator parent relationships or a previously accepted `jobs.send` receipt as future authority.

**One authoritative Workspace writer epoch** protects cross-node Session/branch commits. Other AW machines may display permitted snapshots or submit original-idempotent requests to the active writer; they may not run a second coordinator using a stale owner/epoch or resurrect a live Process/Graph Job. Coordinator replacement/failover is separately fenced on current Session/branch binding and active owned children; an unknown old writer blocks new effectful starts until reconciled. This extends existing workspace writer and Agent Turn writer fences, not a new distributed scheduler.

**Share ≠ transfer:** converting a personally owned project/Session into team-governed ownership is a distinct explicit authorized export/transfer/curated import into the team workspace. Keep the original `/d/i/p/a` and Session/Agent history immutable and private; allocate the destination workspace/project root `a:0` separately and bind its own primary Session/branch if/when needed. Never silently rename an old personal Agent or leak a private history into the new project because a participant was invited.

#### Minimal layered extension points

The actual current crate boundary is **`aw-tools` (shared/core primitives)** and **`aw-tools-adapter` (AW-specific concrete adapter)**. There is **no separate `aw-tools-core` crate** in this tree; “aw tools core” refers to the host-neutral portion of `aw-tools`, not a second library or duplicate Job engine.

| Layer | Minimal responsibility | Must not own |
| --- | --- | --- |
| **`aw-tools` core** | Generic typed **request origin / context token, writer-generation fence, coordination scope, accepted-input/ToolCall/Job receipt** contracts; bounded queue/admission/owner callback extension points when concretely needed; existing immutable Graph, Job, replay, output, cancellation and conservative route/footprint primitives | Identities, teams, Session ACL/branch history, invitations, Agent loop, provider/payer, independent persistent queue, policy engine or collaboration scheduler |
| **`aw-tools-adapter`** | Bridge the same `PreparedOperation`, exact native/Process/Graph/Job executor and factual result to trusted AW host callbacks; pass authenticated scope/epochs/author and revalidate owner/admission before side effects | A second grant/Session/membership database, model/provider choice, participant inbox, coordinator Agent or Session writer |
| **AW host + SessionStore/Policy/Actor** | Authenticate participants, verify team membership, own Session/branch and resource audience, the real Agent coordinator binding, Input queue and one writer, shared drafts/approved context, provider/payer policy, attribution, current disclosure and revocation | Duplicate Agent/Graph runner, a synthetic team-user on share, privilege union or transcript shadow store |
| **dotlink** | Concrete remote Process/Graph/fs owner, sandbox/launcher/scope/retained bytes, exact bridge replay/native authority checks | AW Session sharing, teams, private Agent history, coordination parent or per-participant grant store |

These are **extension responsibilities**, not instructions to create five new public traits or a mandatory extra allocation in every `read` call. Prefer extending the existing shared Graph/Job **owner interfaces and admission callbacks** (or adding the smallest generic host hook only where genuinely missing), with typed receipt/epoch fields; do not assume a new public `JobOwner` trait already exists. Conceptual examples:

    ExecutionOrigin { request_id, actor_ref, resource_owner_ref, policy_epoch }
    CoordinatorScope { session_owner_ref, agent_writer_epoch, workspace_writer_epoch, binding_revision }
    AdmissionFence { expected_agent_writer_epoch?, expected_workspace_writer_epoch?, expected_branch_revision? }
    InputReceipt { original_request_id, accepted_generation, accepted_or_denied }
    HostCoordinationHooks { authorize_and_claim, enqueue, snapshot, cancel } # conceptual

Implement only the host-neutral parts actually consumed by multiple callers, through **borrowed/compact types and sealed host-authenticated callbacks where practical**. The core treats actor/Session/team data as opaque evidence: it never interprets `team` membership or lets a model string mint authority. An owner token is **not** a capability to execute effects. The existing AW Actor Input queue, SessionStore transaction/commit log and ToolBindingAuthority decide durable membership, rights and Turn claims. No new collaboration event database, Actor mailbox, provider runner or TeamAgent resource.

**Fast path:** for one authenticated owner with an ordinary personal Session, the existing primary `a:0` binds lazily, one Agent/branch writer processes input through `Local`/ToolPolicy/Envelope, and there is **no team lookup when no team context is used**, no audience-sharing transform, no extra coordinator/queue/model call and no remote coordination hop. Sharing adds membership and audience checks **only at the affected Session boundary**, not a new middle layer to every single-user filesystem operation.

#### Required shared-session conformance tests

1. One local/domain user: lazy project `a:0` only when first primary Session executes, one Session/branch writer, no collaboration objects/model inference overhead.
2. Invite Bob to Alice's personal Session **full history**: canonical owner, Session, branch and existing Agent stay identical; Bob's View/Submit (if granted) is attributable to Bob, not a new team principal or extra master.
3. Concurrent Alice/Bob requests: one idempotent per-request admission, one ordered Agent queue and active Turn/branch writer; default Submit queues instead of steering other user's active Turn.
4. View/Contribute are not paid inference, generic append, Job Control, provider credentials, filesystem/shell grants or new Agent authority; sealed shared draft submission is distinctly fenced.
5. A full share with unauthorized private context/dependencies fails; approved-context share creates a sanitized separate Session and only allocates a new `a:<handle>` on executable activation. Session branch fork alone is never claimed to isolate private history.
6. Bob's submitted tool attempts intersect Bob's current grant with Session/coordinator and workspace/domain denies. Alice's higher permissions or team colleagues' combined grants are unavailable; explicit service principal uses only its own authorized unattended policy.
7. Membership/revocation during queued/active Input, unknown Process effects and history pagination: future starts/ToolCalls/pages fail under current epochs; committed effects remain truthful, no blind retry or erased audit.
8. Multiple authorized team participants in a team-owned workspace: one project root Agent for the primary Session, no per-person coordinator, one writer per active Session branch; distinct authorship/payer.
9. Multiple Sessions/branches in one project: `a:0` binds its first primary executable Session/branch once; other active or later branches have distinct Agent refs. Never reuse `a:0` concurrently **or sequentially** for another Session/branch after retirement.
10. Explicit team ownership transfer preserves original private Session/ref identities; destination project has its own root and selectively imported context, never implicit promotion on share.
11. Cross-host follower/stale Workspace writer epoch cannot commit a second Agent Turn, change audience or double-pay provider; original owner can recover or truthfully report blocked/unknown.
12. Implementation reuse test: one original `aw-tools` Graph/Job runtime, one AW SessionStore/Actor queue, one ToolPolicy/Envelope pipeline, no new collaboration-specific public tool or duplicate team Agent engine. Same-user fast path must retain default no extra queue hop, policy network call or paid model Step.

### 2.6 Normative new input schemas

The 13 filesystem argument schemas retain DL-150 semantics and limits; only their canonical semantic IDs become fs.* below the provider wire.

Direct-constructor presentation fields are capability-projected, not blindly shared:

    title?: string            # 1..160 printable UTF-8 bytes
    background?: boolean      # direct response disposition only
    wake?: EventRule[]        # only when the host has a current receiving Agent + wake authority

Unsupported fields are omitted from the advertised provider schema rather than accepted and rejected later. In particular, standalone dotlink does not advertise wake because it has no current receiving Agent.

Initial EventRule is a tiny typed tuple:

    [event, mode?]

    event = result | settled | attention
    mode  = auto | queue | steer | interrupt

Every EventRule[] field, including constructor wake and jobs.subscribe events, is one bounded unique semantic set: duplicate event names reject and admitted/replay-hashed order canonicalizes as result, settled, attention. Omitted mode means auto. The resolved Job kind validates which events/modes it supports. Process Jobs support result/settled. Agent Jobs additionally support attention where authorized. An AW Graph supports attention iff its admitted immutable topology contains at least one **attention-capable Agent node**; standalone dotlink Graphs never do. Initial vNext treats an Agent Graph node as **completion-only unless its captured host delegation permits blocking parent-attention/request-response**; a nonblocking parent message alone does not make it attention-capable. This host predicate is not a model argument and the same captured predicate drives attention projection, proactive parent-controller handoff and runtime enforcement. Without that authority, the child cannot wait on its blocked invoking parent. A future event name requires an intentional public-schema revision rather than accepting arbitrary strings.

When wake is supplied on a constructor, registration of the initial rule is atomic with Job admission and happens before the first external effect, so a fast completion cannot race past its wake subscription.

Shell constructors:

    bash / pwsh / powershell {
      command: string,
      dir?: string,
      stdin?: string,
      timeout_secs?: positive integer under host ceiling,
      max_output_bytes?: positive integer under host ceiling,
      writes?: string[] | null,
      interactive?: boolean,
      title?: string,
      background?: boolean,
      wake?: EventRule[]              # AW-hosted/capable projection only
    }

writes remains advisory scheduling footprint, never authority.

interactive=false/omitted means stdin is limited to the constructor's optional initial stdin and is closed/retired afterward; jobs.send to that Process rejects live input. interactive=true retains explicit live-stdin custody for jobs.send until EOF/process settlement. **Custody is not lasting input authority:** every subsequent stdin text/EOF control must pass the current exact `jobs.send` Control grant **and independent current `process.input` launcher/source/epoch/resource grant** against the original Process identity and its *actual* retained sandbox/process privileges. If current policy would prohibit using that already-running Process's original execution environment for new input, reject the write (or separately authorize safe termination); do not pretend a changed host grant narrowed a running OS sandbox. A revoked or narrowed parent/Session/Process policy never becomes an input-escalation channel. Factual prior writes remain committed/unknown and ambiguous delivery is never blindly retried. interactive never creates filesystem/network/process authority.

batch:

    batch {
      ops: [operation,args,when?][]   # 1..64
      title?: string,
      background?: boolean,
      wake?: EventRule[]              # AW-hosted/capable projection only
    }

Graph nodes reject background/title/wake fields that are direct-constructor presentation/subscription metadata unless a field is explicitly part of the admitted node's semantic execution args. In particular, a nested agent/bash node does not create a nested background Job by setting background. A nested shell may still carry interactive because that changes the node's Process-input semantics.

agent creates a new long-lived Agent Job and its initial Turn:

    agent {
      task: string,                   # non-empty, bounded prompt
      title?: string,
      background?: boolean,
      wake?: EventRule[]              # AW-hosted/capable projection only
    }

Agent Jobs are intrinsically continuable through jobs.send; there is no separate interactive Agent flag.

There is intentionally no id/agent_id argument. The host allocates one stable immutable Agent handle in the current domain/principal/project Agent namespace and records parent/origin metadata. Existing Agents continue only through jobs.send.

### Typed canonical handles and hierarchical execution refs

The jobs.* family shares the same ExecutionRef and Revision types only where they are semantically useful; each verb owns the smallest safe schema. In particular, jobs.list has no ref, jobs.cancel/jobs.subscribe require explicit ref, and jobs.read/jobs.ack have no rev.

Every path segment is explicitly typed:

    d:<handle>   domain
    i:<handle>   principal / identity
    p:<handle>   project / workspace
    a:<handle>   Agent
    t:<handle>   Agent Turn
    j:<handle>   durable child Job
    n:<handle>   Graph node

The handle itself is the canonical immutable identity for that resource within its containing namespace. The specification does not mandate UUIDs, ULIDs, integers, hashes, slugs, or any other concrete handle format for d/i/p/a/t/j. **Graph n:<handle> is the exception:** its handle is the immutable 1-based Graph coordinate in canonical decimal (n:1, n:2, ...; no leading zeros). This reuses DL-150's existing node identity, eliminates a separate mapping/allocation table, and lets a model derive a node ref directly from its submitted Graph.

Normative handle requirements:
- immutable once allocated;
- unique within the containing namespace;
- never reused or silently reassigned within that namespace, even after retention expiry; a stale canonical ref becomes gone/expired forever rather than identifying a later resource;
- 1..64 ASCII bytes;
- first byte is ASCII alphanumeric;
- remaining bytes are ASCII alphanumeric or ".", "_", "~", "-";
- byte-exact and case-sensitive; no Unicode normalization, percent-decoding or alternate escaped spelling;
- directly usable as canonical identity without a second alias-resolution layer.

This lexical grammar deliberately does not constrain the allocator: a handle may still be a number, slug-like token, UUID/ULID/hash-like value, or another owner-chosen stable identifier so long as its canonical spelling fits the grammar. Values that do not fit receive another canonical handle; human/display names remain metadata and may change freely.

Reserved structural default handles:

    d:0   local/default domain
    i:0   default principal
    p:0   default virtual project
    a:0   root/default Agent

The value 0 has this reserved meaning only for d/i/p/a. It is not implicitly special for t/j; n:0 is invalid because Graph coordinates start at 1. Structural :0 denotes one concrete, explicitly provisioned canonical default object within its containing namespace, never an ambient globally privileged principal, caller-relative alias or request to create a missing object. For example, /d:abird/i:0 always names the same default principal object in d:abird for every caller; authorization decides who may access it, but authentication never rewrites its identity.

### Canonical refs versus contextual refs

There are exactly two ref forms.

Canonical refs:
- begin with "/";
- are already fully resolved;
- always contain the full structural d/i/p/a tuple;
- may then contain execution suffixes t/j/n;
- never inherit caller context.

Example:

    /d:abird/i:pvl/p:zg/a:0
    /d:abird/i:pvl/p:zg/a:2/t:5/j:1
    /d:abird/i:pvl/p:zg/a:2/t:5/j:2/n:4

Canonical durable storage/replay/routing always uses this fully resolved form, including explicit :0 structural defaults. "/a:0", "/i:pvl", and other partial slash-prefixed forms are invalid.

Contextual refs:
- do not begin with "/";
- are interpreted relative to the exact current Agent structural context;
- resolve once **on first admission**; a matching retry looks up the original invocation claim **before** re-resolution and uses its stored canonical target.

Special contextual refs:

    .    current Agent exactly
    ^    current authorized coordination-parent Agent

There is no filesystem-style ".." traversal, wildcard, range, query syntax, or alternate alias syntax.

### Contextual structural-overlay resolution

Treat the current Agent structural identity as the tuple:

    [d, i, p, a]

For example:

    current = /d:abird/i:pvl/p:zg/a:7

A contextual typed structural ref is an overlay with this rule:

1. inherit all structural ancestors above the first explicitly supplied structural segment from current context;
2. set every explicitly supplied structural segment;
3. reset omitted structural descendants below the first changed/supplied level to their reserved :0 defaults unless explicitly supplied later;
4. after Agent resolution, append any t/j/n execution suffix exactly as written.

Examples:

    a:0
      -> /d:abird/i:pvl/p:zg/a:0

    a:2
      -> /d:abird/i:pvl/p:zg/a:2

    p:core
      -> /d:abird/i:pvl/p:core/a:0

    p:core/a:2
      -> /d:abird/i:pvl/p:core/a:2

    i:alice
      -> /d:abird/i:alice/p:0/a:0

    i:alice/a:2
      -> /d:abird/i:alice/p:0/a:2

    d:remote
      -> /d:remote/i:0/p:0/a:0

    d:remote/i:alice/p:core/a:2
      -> /d:remote/i:alice/p:core/a:2

    t:5
      -> /d:abird/i:pvl/p:zg/a:7/t:5

    t:5/j:1
      -> /d:abird/i:pvl/p:zg/a:7/t:5/j:1

    a:2/t:3
      -> /d:abird/i:pvl/p:zg/a:2/t:3

    p:core/a:2/t:5/j:1
      -> /d:abird/i:pvl/p:core/a:2/t:5/j:1

If no current Agent context exists, the parser may use the structural base:

    /d:0/i:0/p:0/a:0

before applying the same overlay rule. **Parsing is not admission.** These :0 values are fixed canonical objects, not caller-relative aliases. The host must have provisioned them and the caller must have current authority; otherwise resolution is unavailable/denied. A read, list, send or cancel never materializes a principal, project or root Agent. The current authenticated principal is represented by contextual inheritance, not remapping i:0. Standalone dotlink uses its own explicit live-session finite Job ref, not AW defaults.

**Contextless AW SDK/host constructors:** an AW Process/Graph/Agent Job ref is anchored under a real, already provisioned and currently authorized `AgentRef` in the exact d/i/p namespace. An SDK call outside a current Agent Turn must **explicitly select such an existing Agent owner** (the project's bound `a:0` if available, or another real Agent) and establish its caller/payer/ToolPolicy/Session/writer authority; it cannot silently create a synthetic Agent, bind an unbound `a:0`, or convert the parsing-only defaults into grants. Without that anchor, Job-producing AW calls fail before any effect. The host can still expose directly authorized non-Job filesystem operations without creating an Agent. Standalone dotlink continues to own explicit live-session `j:<handle>` Jobs without AW Agent context.

This means "dropped" structural segments are contextual, not aliases:
- before the first explicit structural level they inherit current context;
- below that level they reset to :0 unless explicitly provided.

The parser must enforce structural segment order d -> i -> p -> a and reject duplicates or upward/backtracking structural order inside one contextual ref.

Execution suffixes are typed ownership transitions rather than one hard-coded path shape:
- t:<handle> may resolve only from an Agent and identifies one exact Turn;
- j:<handle> may resolve only from a resource that authoritatively owns that durable child Job;
- n:<handle> may resolve only from a Graph Job and identifies one exact Graph node;
- an n:<handle>/j:<handle> continuation is valid only when that node genuinely owns independently durable/control-worthy child execution;
- additional future transitions are added only by the owning Job kind, never guessed from segment text.

Once an execution suffix starts, d/i/p/a structural segments cannot reappear. Invalid type transitions fail before any control/effect dispatch. This permits future composition such as t:<h>/j:<h>/n:<h>/j:<h> without manufacturing redundant IDs or freezing the namespace to one implementation topology.

No generic id+scope pair exists in the public API.

### Stable Agent identity, parent coordination and origin

Agent identity is namespace-stable and does not encode the Turn that created it.

If current Agent:

    /d:abird/i:pvl/p:zg/a:0

creates another Agent during Turn t:5, allocate a stable immutable Agent handle in that same d/i/p namespace, for example:

    /d:abird/i:pvl/p:zg/a:2

not:

    /d:abird/i:pvl/p:zg/a:0/t:5/j:1

Record creation provenance separately:

    origin_agent = /d:abird/i:pvl/p:zg/a:0
    origin_turn  = /d:abird/i:pvl/p:zg/a:0/t:5
    origin_step  = 2
    origin_call  = c7

Step remains journal/scheduling metadata only. There is no public s:<...> segment. A Step is one model invocation plus its complete tool-receipt phase; a durable ToolCall may create a child Job beneath the owning Turn, but the Step itself is not a Job/addressable public resource.

Each Agent may have an authorized current coordination-parent relation separate from immutable origin:

    parent_agent?: AgentRef

A coordination-parent update is an authenticated, journaled **host-owned relation change**, never an effect of merely supplying `^` or an Agent path. Reject self-parent, cycles, stale expected parent generation and any parent not currently permitted to coordinate that Agent. Peers use explicit authorized Agent refs; the `^` chain remains a true acyclic parent relation without silently changing durable Agent identity or execution ownership.

"^" resolves this current coordination parent at admission time. It means coordinator/parent, not creator/origin. If no parent exists, "^" is unavailable.

Parent resolution is atomic with authorization. Preserve both caller spelling and canonical resolution in immutable ToolCall/control evidence:

    requested_ref = "^"
    resolved_ref  = "/d:abird/i:pvl/p:zg/a:0"

Later parent changes cannot reinterpret an already admitted request.

Agent identity/parent relationships never grant authority by themselves. Every resolved ref still passes normal actor/source/site/epoch/resource checks. Cross-domain/principal/project communication therefore uses the same contextual overlay grammar only when explicitly authorized.

### Hierarchy is identity/provenance, not implicit lifecycle authority

The canonical hierarchy gives stable address/origin structure. It must not by itself imply:
- authorization;
- cancellation propagation;
- retention;
- current ownership;
- ancestor liveness.

A long-lived descendant may outlive a finite ancestor/container. Its canonical ref remains stable; its **current authoritative owner** remains directly reachable independently of the ancestor's live process. Retain only the bounded routing identity/expiry or replay tombstone required while that descendant is legitimately live/addressable; release it once no live or protocol/replay owner requires it. A dead ancestor supplies no renewed grant, and no tombstone resurrects an unowned OS Process or an expired dotlink live-session Job.

Never reparent or rename canonical refs after creation. Mutable coordination/ownership edges are metadata/authority state, not identity.

Child cancellation still follows explicit current ownership relations. Path ancestry alone is never sufficient proof that cancellation should propagate.

### Revision semantics

rev is orthogonal to ref:
- ref identifies the exact canonical execution resource after contextual resolution;
- rev identifies one externally meaningful observed state of that resource.

Every addressable resource exposes rev when meaningful. Internal noise does not advance it; any change that can alter a caller's next safe control/read decision does.

Operation semantics:
- jobs.send/cancel/subscribe: omitted rev means atomically act on current state; supplied rev is exact compare-and-act and stale returns with no effect;
- jobs.get: rev is a conditional observation token used for cheap lost-wakeup-safe long polling, as defined below;
- jobs.read and jobs.ack deliberately have no rev: exact ref + immutable raw offset/opaque cursor/ACK watermark already provide the stable identity needed for replayable reads and idempotent release.

SDK helpers should carry observed rev automatically **only for intended compare-and-act controls**, never blindly attach a snapshot rev to ordinary get/read/ACK. Observation returns current factual revisions; mutation may optionally fence its expected state. This prevents stale-read loops and makes output ACK idempotent across unrelated Job status changes.

A **new** contextual ref is canonicalized once at its trusted admission boundary before effects; a replay first looks up the original authenticated invocation ID and reuses its recorded canonical ref **without** current-context reinterpretation. Journal both requested_ref and resolved_ref.

### Job-kind specialization

The hierarchical ref supplies identity/address. Each resolved Job/resource kind still owns its typed semantics:

    Meta
    Input
    View
    Event
    supported send modes
    default send mode

Generic snapshots return:

    {
      ref,
      kind,
      state,
      rev,
      meta,
      children?
    }

Children/actions exposed to models should use the shortest safe contextual ref from current Agent context when possible. SDK/audit projections may additionally expose the fully resolved canonical_ref.

Kind-specific examples:
- Agent meta exposes current turn/step/queued/parent facts; t:<handle> is an addressable Turn while Step stays metadata only.
- Graph Job meta exposes done/total; n:<handle> is an addressable Graph-node ref beneath that Graph Job.
- Process Job meta may expose interpreter/stdin state and usually has no child resource.

Future Job kinds extend their own path/resource semantics beneath j:<handle> without adding new generic jobs.* request fields.

jobs_list:

    {
      cursor?: string,
      limit?: integer >= 1             # host-capped
    }

jobs.list is flat discovery of exposed Job roots, not a recursive namespace query. In an AW Agent context its scope is **exactly the caller's authenticated d/i/p namespace**, filtered by current per-Job authority: exposed Agent roots and finite Process/Graph Jobs owned by authorized Agents in that namespace. A changed coordinator/parent relation does not silently broaden discovery. Crossing d/i/p requires a known authorized explicit ref and get/read, never an implicit broader list. It never lists Turn or Graph-node descendants. Standalone dotlink lists only exposed finite Jobs in its authenticated live runtime session.

**One host-neutral exposure-order contract, two storage modes:** a bounded monotonic `(sequence, canonical ref, owner generation, exposure state)` index serves `jobs.list` across Agent roots and exposed finite Jobs; it **never copies** lifecycle, output or history. A Session-backed AW host stores this tiny index in its existing project/SessionStore owner, atomically publishing Agent creation and its list entry. A **sessionless Local AW host** uses the **same index contract** in memory, scoped to one fresh runtime generation; it needs no SQLite/JSONL, SessionStore or policy network lookup. It lists only locally owned exposed finite Jobs and any explicitly provisioned live Agent root, with no durable history/restart promise. Standalone dotlink retains its live-session finite index.

Reserve a bounded exposure/recovery slot with the original claim **before** a finite external effect, even for optimistic foreground completion that might need lost-receipt promotion. Publish the reserved slot idempotently when exposed. On failure or uncertain publication, the original replay owner retains custody and returns pending/unknown or fails before effects; it must never claim a successful, discoverable Job without an index entry. On restart, a sessionless runtime rejects prior-generation cursors/handles and never resurrects old Process/Graph execution. **Finite `j:` handles never reuse identities across generations in the same canonical parent namespace** (use generation-scoped unique handle allocation, not a reset counter). The index is a small projection of the original owners, not a second Job manager or new mandatory I/O path for direct filesystem tools.

Results use this common immutable newest-exposure sequence descending, with canonical ref only as an exact deterministic tie-breaker. A list cursor means "continue strictly older than this position"; activity cannot reorder an existing page.

A list cursor is owner-verifiable/stateless and binds:
- exact canonical AW d/i/p listing scope, or standalone live-runtime session generation;
- authenticated listing principal/authority epoch;
- immutable exposure position/tie-breaker;
- projection format version.

The next page revalidates current authority. Scope/session-generation mismatch, authority-epoch change or cursor tampering rejects rather than continuing in a different namespace/runtime. Expired/revoked resources may create gaps but one valid traversal never aliases an identity or returns the same exposure twice. Use jobs.get(ref) to inspect one Job's current actionable descendants.

jobs_get:

    {
      ref?: string,                    # default "."
      rev?: integer,
      wait_ms?: integer >= 0           # host-capped
    }

jobs.get is observation, not an effect. rev is an observed-revision/change token here:
- omitted rev returns the current snapshot immediately unless wait_ms is supplied, in which case the owner atomically snapshots/subscribes and may wait for the next externally meaningful revision while the resource remains active;
- supplied rev returns immediately if current rev already differs; if equal and wait_ms > 0, wait until rev changes, the resource settles/closes, or timeout;
- timeout returns the current factual snapshot, not a synthetic failure;
- lost wakeups are prevented by one owner-specific subscribe/snapshot boundary.

Effectful controls continue to use supplied rev as exact compare-and-act precondition.

ReadView remains a small advertised enum:

    result | stdout | stderr | messages | tool_calls | tool_outputs | trace

Optional Agent-history Filter has only two presets and one custom predicate:

    "default" | "none" | {
      result?: boolean, messages?: boolean,
      tool_calls?: boolean, tool_outputs?: boolean,
      tools?: string[], exclude?: boolean
    }

jobs_read:

    {
      ref?: string,                    # default "." in authenticated AW Agent context
      view?: ReadView,                  # one specific kind-supported view
      filter?: AgentHistoryFilter,      # Agent history only; mutually exclusive with view
      cursor?: string,                 # opaque record/history continuation
      offset?: integer >= 0,           # raw retained byte streams only
      limit?: integer >= 1,
      encoding?: "text" | "base64",
      wait_ms?: integer >= 0
    }

Strict kind/view validation:
- Process: result/stdout/stderr; offset/encoding/wait_ms are stream controls; `filter` is invalid.
- Graph: root result or n:<coordinate>-specific result/stdout/stderr when that node supports them; `filter` is invalid.
- Agent: result/messages/tool_calls/tool_outputs/trace; exact Turn selection uses t:<handle>, while Step remains journal metadata and root historical reads use opaque cursor/limit.
- `view` and `filter` are mutually exclusive. An omitted view/filter means the natural result; an Agent `filter` selects a chronological combined public projection without a second history tool.
- fields irrelevant to the resolved kind/view are rejected rather than ignored. No regex/glob, historical query engine or second filter alias.

jobs_ack:

    {
      ref: string,
      view?: "result" | "stdout" | "stderr",
      offset: integer >= 0
    }

ACK is valid only for an exact ackable retained byte owner. Agent durable history views are never ACK targets. Exact ref + resolved natural/explicit view + raw offset is the idempotence key; no revision fence is needed or permitted, so a lost ACK response can be retried even if retention/cleanup state advanced.

DL-150 retention invariants remain:
- offsets/watermarks are raw retained-byte positions independent of text/base64 presentation;
- ACK is monotonic/idempotent and cannot advance beyond bytes actually retained for that owner/view;
- acknowledging retained bytes never erases immutable terminal outcome/truncation metadata;
- physical release waits until every replay/MCP Task/protocol owner of that range has released it.

SendInput is intentionally tiny and typed:

    {text: string} | {eof: true}

Exactly one variant is accepted; unknown/extra fields are rejected.

jobs_send:

    {
      ref?: string,                    # default "."
      rev?: integer,
      mode?: "auto" | "queue" | "steer" | "interrupt",
      input: SendInput
    }

Generic send modes describe caller intent:

- auto: use the resolved resource's safest natural input behavior; default.
- queue: defer input to the next normal input opportunity without altering active execution.
- steer: preserve the current indivisible decision/effect unit, then redirect at the next safe steer boundary.
- interrupt: redirect at the earliest safe boundary, settling/cancelling already-started effects truthfully first.

There is deliberately no live/ready/next/after/finish/complete/turn/step delivery vocabulary in the generic API.

Initial specialization:
- Agent root (., ^, a:<handle>, any contextual structural overlay resolving to an Agent, or a fully resolved canonical /d:<h>/i:<h>/p:<h>/a:<h>): auto -> queue.
- active Agent Turn t:<handle>: auto -> steer within that same Turn. Turn rev naturally fences which current Step/state was observed; Step itself needs no public ref.
- Process Job j:<handle>: auto -> immediate live stdin behavior; queue/steer/interrupt unsupported unless a future Process specialization explicitly defines them.
- Graph node .../j:<handle>/n:<handle>: auto delegates to the underlying node owner's natural behavior.
- a target that cannot accept input rejects jobs.send.

Input is kind-specific after the common SendInput schema validates:
- Process input: {text} or {eof:true}; requires retained interactive stdin custody.
- Agent input: {text}; eof is invalid.
- Graph-node input delegates to the underlying interactive node specialization.

jobs_cancel:

    {
      ref: string,                     # always explicit; "." intentionally means self
      rev?: integer
    }

Cancellation is destructive, so omission never defaults to self. Generic meaning: request cancellation of exactly the resolved resource. The resolved kind defines whether that ref is cancellable and how cancellation settles. No generic turn/node/scope-kind field exists.

jobs_subscribe:

    {
      ref: string,                     # explicit event source
      rev?: integer,
      events: EventRule[]
    }

Example:

    events:[
      ["result","queue"],
      ["attention","steer"]
    ]

Event names and supported modes are validated by the resolved kind. Omitted event mode means auto. events is semantically a bounded unique set: each event may appear at most once; duplicate event names are invalid even when modes match. The owner canonicalizes the admitted/replay-hashed order by enum order result, settled, attention so caller array order does not create a second semantic rule. An empty events array clears the current recipient's future rule for that explicit source ref. Registration is non-retroactive. Because the recipient is the current Agent, an Agent source resolving to that same recipient is rejected to prevent reflexive result/attention wake loops.

All string/array/byte/time/ref ceilings remain host-bounded. Canonical refs and contextual paths must have a small bounded segment count and byte length. Generated schemas should expose only ceilings that materially help the model form a valid call. The generic jobs.* schema stays small: compact enums/oneOf variants are preferred over untyped strings/objects, but it must not inline every Job-kind metadata shape.

## 3. Job is the universal execution resource

### 3.1 What is a Job?

A Job is an execution scope worth independent lifecycle/observation/control because it may:
- outlive the immediate model/tool request;
- wait;
- retain output;
- require cancellation;
- hand off;
- expose meaningful progress;
- settle with uncertain external effects;
- receive live input;
- generate future wake events.

Initial Job kinds:

    Process
    Graph
    Agent

Public lifecycle state is one fixed vocabulary, separate from DL-150's execution outcomes:

    running
    waiting
    idle
    blocked
    cancelling
    cancelled
    closing
    closed
    settled

Meaning:
- running: active execution may progress;
- waiting: active execution is deliberately waiting on input/condition/child work and consumes no model inference merely because it waits;
- idle: a long-lived Agent has no active Turn and may claim future input;
- blocked: a long-lived owner cannot safely admit further execution until authority/reconciliation changes; queued input remains factual/durable;
- cancelling: cancellation intent is recorded and owned work is settling;
- cancelled: a finite execution's cancellation has settled;
- closing: a long-lived Agent accepts no future input and is settling current owned work;
- closed: a long-lived Agent is terminal;
- settled: a finite execution is terminal without cancellation; its factual DL-150 outcome is reported separately.

Kind subsets:
- Process/Graph/Graph-node/Turn: running|waiting|cancelling|cancelled|settled;
- Agent: running|waiting|idle|blocked|closing|closed.

Do not invent aliases such as run/terminal in the public model/SDK vocabulary. DL-150 primary outcomes ok|unchanged|fail|mixed|unknown|skip remain separate from lifecycle state and are the only Graph when predicates.

Future Job kinds, such as a durable Wait/Watch, require separate measured need but use the same control plane and lifecycle vocabulary where applicable.

### 3.2 Job-producing constructors

    bash        -> Process Job
    pwsh        -> Process Job
    powershell  -> Process Job
    batch       -> Graph Job
    agent       -> Agent Job

Direct foreground completion may keep a finite Job unexposed only while response delivery/replay ownership can still recover that exact execution. Background/handoff exposes the same Job; it never creates a second execution.

Before the first external effect, every finite constructor reserves the bounded recoverability capacity needed for its worst-case delivery path: replay identity plus an exposure/list slot if lost-receipt promotion may be required. An exposed live Job is never silently omitted from jobs.list because an index filled later; admission fails before effects if required recoverability capacity cannot be reserved.

A background or automatic handoff receipt is compact and immediately actionable:

    <ref> <kind> <state> rev=<rev> [progress]

It includes the shortest safe contextual ref plus current rev so the model can call jobs_get({ref,rev,wait_ms}) directly without an extra status round. It never echoes constructor arguments.

DL-150 lost-receipt rule remains:
- if the transport has durable replay identity, an ambiguously delivered synchronous result stays attached to that hidden same-Job replay owner until positive delivery or bounded replay retirement;
- if no durable replay identity remains and a response attempt disappears before positive delivery, promote/expose that same finite Job so jobs.list can recover it;
- positive delivery may retire unexposed finite state only after replay/output owners permit it;
- response loss never discards an effectful execution and never causes constructor re-execution.

Agent Jobs are long-lived and remain addressable while idle between Turns.

### 3.3 What is not a public Job?

Do not allocate public Jobs for every small activity.

Ordinary bounded filesystem calls such as fs.read/fs.edit are ToolCall/effect receipts inside their owning execution.

An Agent Step is not a Job.
An Agent ToolCall is not automatically a Job.
A passive Notify is not a Job.

A nested activity that itself has durable/control-worthy execution semantics remains Job-backed. It may stay hidden if it completes synchronously and never needs independent model control.

### 3.4 Hierarchical execution and redundant child IDs

The canonical hierarchical ref already provides exact durable addresses; do not add a parallel scope-ID system.

Examples:
- Agent Turn: t:5 under the Agent.
- durable Process/Graph child launched during that Turn: t:5/j:1.
- Graph node 4 inside Graph Job j:2: t:5/j:2/n:4.
- additional Agent: stable a:<handle> at the same d/i/p Agent namespace, with creation origin recorded separately.

A Bash node inside a Graph therefore keeps process custody/output/cancellation semantics while jobs.* addresses it directly as:

    t:5/j:2/n:4

The Graph owner still owns canonical node semantics internally. n:<handle> is simply the Graph owner's typed node segment.

Do not manufacture t:5/j:2/n:4/j:1 merely because n:4 executes a process if the Graph node itself already provides sufficient durable custody/control. Add a child j:<handle> only when independently durable/control-worthy execution genuinely exists beyond the container's own address.

An Agent ToolCall that launches a Process/Graph may create a child j:<handle> beneath the owning Turn. A ToolCall that creates an Agent allocates a stable a:<handle> in the current d/i/p Agent namespace instead; the Agent is not permanently nested beneath the creator Turn.

If finite nested work completes synchronously and never needs independent control/handoff, it may remain hidden while its factual ToolCall receipt is preserved.

No duplicate execution engines or ledgers are created by exposure.

## 4. Heterogeneous Job ownership without duplicated state

The unified jobs.* API is a control plane, not a requirement that one monolithic data structure own every runtime.

Authoritative owners remain:

- aw-tools Graph/Job engine for Graph state and retained node results;
- Process owner for OS process custody/streams;
- AW Session/Actor/TurnJournal for Agent Job identity, Turn state and durable conversation history.

Introduce only host-neutral common execution-reference/interfaces needed for routing:

    PrincipalId
    ProjectId
    AgentId
    TurnId
    JobId
    NodeId
    Revision
    ExecutionRef
    ResolvedExecutionRef
    JobKindId
    JobState
    JobSnapshot
    JobReadRequest
    JobReadPage
    JobSendRequest
    JobControlResult

ExecutionRef parses the tiny typed-handle grammar. Contextual refs are resolved from exact current Agent context into one fully resolved canonical ResolvedExecutionRef before dispatch.

The generic kind contract owns typed specialization rather than extending every common schema:

    JobKind {
      Meta
      Input
      View
      Event
      supported_send_modes
      default_send_mode
    }

and a registry/router from fully resolved canonical refs to authoritative owners.

Revision identity is generic even though only control/observation operations that need it expose it:
- each addressable execution resource has an externally meaningful revision when useful;
- revision changes only for externally observable state relevant to control decisions, not internal noise;
- jobs.send/cancel/subscribe use supplied rev as compare-and-act;
- jobs.get uses supplied rev as the caller's observed revision and returns/waits for a newer factual snapshot rather than treating advancement as an error;
- jobs.read/ack use stable ref + cursor/offset instead of revision tokens, preserving DL-150 replayable paging/ACK semantics.

A ref is identity/address, never authority.

The generic owner router does not duplicate mutable Job state: it holds bounded ref→owner routing/exposure identity and tombstones only while needed for replay or live descendants. Its **single exposure-order primitive** uses SessionStore-backed project persistence when a Session backend exists, or bounded runtime-scoped memory in a sessionless Local host (Section 2, `jobs.list`). Both reserve capacity before finite effects and idempotently publish only actual exposed Jobs. No cross-database transaction manager, mandatory SessionStore for Local mode or additional Agent/Graph scheduler is introduced.

A control call:
1. authenticates original source/invocation identity and **checks replay first**; an existing claim uses its stored canonical target, returning the original receipt under current disclosure without redispatch;
2. only for a **new** identity, resolves the contextual ref exactly once and authenticates current actor, `^` relation, resource, source/site and writer authority at one owner admission boundary;
3. atomically stores requested spelling, canonical resolved ref and original request fingerprint under that invocation ID before effects;
4. routes the accepted canonical ref to its authoritative owner for one exact operation;
5. returns the factual typed result. Never re-resolve an admitted ref on retry.

Do not copy Agent Turn state into aw-tools merely to make jobs.get work.

## 5. Process Jobs

Direct shell invocation remains Job-backed, preserving DL-150.

Example:

    bash({command:"cargo test"})

Conceptually:
1. prepare exact Process Job identity/replay claim;
2. start one Process execution;
3. spool bounded retained stdout/stderr;
4. wait optimistically when foreground;
5. if result fits synchronously, deliver once and retire unexposed finite Job;
6. otherwise expose the same Job.

Process views:

    stdout
    stderr
    result

jobs.send supports retained interactive stdin where explicitly enabled.
jobs.cancel requests process-tree termination through original custody.
Unknown stop/outcome remains unknown; never manufacture cancellation success.

## 6. Graph Jobs

Batch remains one immutable bounded Graph Job.

Graph node execution stays within that Job and is addressed hierarchically as n:<handle> beneath the Graph Job ref.

Preserve:
- all-node preflight before any root starts;
- immutable args/topology;
- exact leaf authority/effect/resource checks;
- captured semantic work/result ceilings;
- deterministic conflict admission;
- status grammar;
- more/incomplete semantics;
- raw result/stream retention;
- cancellation truthfulness.

Graph Job itself is Control admission plus independently authorized leaves.

No model-visible job_start. background=true on batch is direct response disposition for the same Graph Job.

No model-visible restart. Re-execution is a fresh explicit constructor call.

## 7. Agent Job

### 7.1 Stable Agent identity

An Agent is a long-lived Job kind with one stable immutable a:<handle> identity inside one d/i/p namespace.

Examples:

    /d:0/i:0/p:0/a:0
    /d:abird/i:pvl/p:zg/a:0
    /d:abird/i:pvl/p:zg/a:2

a:0 is the reserved root/default Agent for every d/i/p namespace. Additional agent(...) calls allocate another immutable Agent handle in the current d/i/p namespace rather than nesting the Agent under the creator Turn.

Internally each Agent ref binds exactly one existing AW owned Session + branch + Actor/writer identity **once for the lifetime of that Agent identity**. There is no second public Agent ID and no separate Agent mapping database. A project root `a:0` may lazily bind its first primary executable Session/branch once; closing/retiring that Session retires that Agent binding rather than freeing `a:0` for reuse. Other independently executable or later Sessions/branches need distinct `a:<handle>` bindings. Sharing a bound Session adds authenticated participants to that same coordinator/input queue, never another Agent writer/team identity. Coordinating multiple Sessions through authorized parent relations never rebinds an existing Agent.

A model-facing agent(...) call from current Agent /d:abird/i:pvl/p:zg/a:0:
- allocates another stable Agent handle in the same d/i/p namespace, for example /d:abird/i:pvl/p:zg/a:2;
- binds/creates exact durable Session/branch identity;
- records immutable origin Agent/Turn/Step/ToolCall evidence;
- establishes the authorized initial coordination parent, normally the creating Agent;
- starts its first Turn.

Agent creation is durably discoverable before paid provider effect. Identity + Session/branch + parent/origin + first-Turn admission and the **one project Job-list index entry** are committed as one replay-stable unit. SessionStore remains the Agent's source of creation/lifecycle evidence; the existing host listing/routing index assigns the comparable list exposure sequence for both Agent and finite Job roots, without a second Agent history/lifecycle store or migration-only per-Agent exposure field. Matching replay returns the same Agent/index position, and authoritative SessionStore identity allows recovery of that same index entry after restart. The first synchronous result includes its compact Agent ref.

The host creates/resolves a:0 for root/default Agent contexts; the model does not need a special root-agent constructor.

### 7.2 Long-lived lifecycle and Agent metadata

An Agent Job spans multiple finite Turns:

    /d:abird/i:pvl/p:zg/a:2  kind=agent
      t:1 running
      t:1 settled outcome=ok
      idle
      t:2 running
      t:2 cancelled
      idle
      ...
      closing
      closed

The Agent Job does not settle merely because one Turn completes. It uses exactly the Agent subset of the public lifecycle vocabulary: running|waiting|idle|blocked|closing|closed.

blocked is specifically fail-closed: no new paid/model execution starts while current inference/writer/authority/reconciliation requirements cannot be proven. It is not an outcome and does not erase queued Input or prior Turn facts.

Agent-specific facts remain typed metadata, not extra generic jobs.* request fields.

Illustrative SDK snapshot:

    {
      ref:"a:2",
      canonical_ref:"/d:abird/i:pvl/p:zg/a:2",
      kind:"agent",
      state:"running",
      rev:17,
      meta:{
        turn:"t:5",
        step:2,
        queued:1,
        parent:"a:0"
      },
      current:{
        ref:"a:2/t:5",
        canonical_ref:"/d:abird/i:pvl/p:zg/a:2/t:5",
        kind:"turn",
        state:"running",
        rev:7,
        meta:{turn:"t:5",step:2}
      }
    }

Compact model projection from a peer Agent in the same namespace:

    a:2 agent running rev=17 turn=t:5 step=2 queue=1
    a:2/t:5 turn running rev=7 step=2

When querying self, prefer the shortest local form:

    . agent running rev=17 turn=t:5 step=2 queue=1
    t:5 turn running rev=7 step=2

Internal owner enums may be richer, but the public projection must use exactly the sealed lifecycle vocabulary above and must not confuse a settled Turn with a closed Agent Job.

### 7.3 Turn generations and revision stability

Turns are durably ordered generations beneath the Agent. Each has a stable t:<handle> identity; the owner may keep a separate monotonic sequence/ordinal for ordering, but that ordinal is not the identity contract. An implementation may choose the decimal ordinal itself as the handle for compactness, but callers must not depend on that encoding.

    t:<handle>
    t:<handle>
    ...

Each Turn owns:
- exact admitted Input;
- model Steps;
- tool proposal/receipts;
- child execution relations;
- final result/outcome;
- reconciliation status.

Only one Turn may hold the branch writer at a time.

A Turn ref is stable and never retargets:

    t:5

always means that exact Turn generation for the resolved Agent. If it is no longer retained, it becomes gone/expired rather than being reused.

Turn rev provides optional stronger state fencing. In particular, the Turn's externally meaningful rev advances when control-relevant Step state changes. This lets a caller precisely steer/cancel the Step it observed without exposing Step as another public ref.

### 7.4 Steps, ToolCalls and child Jobs

One Turn may contain multiple Steps.

A Step includes one model invocation and the complete set of tool-call receipts proposed by that invocation before the next model Step.

Step is journal/scheduling evidence only:

    agent=/d:abird/i:pvl/p:zg/a:2
    turn=t:5
    step=2
    call=c7

There is no public s:<handle> ref.

Ordinary bounded filesystem ToolCalls remain receipts, not Jobs.

If a ToolCall launches independently controllable durable Process/Graph work, allocate a local child Job beneath the owning Turn:

    t:5/j:1
    t:5/j:2

and record exact origin Step/ToolCall metadata on that child.

If the ToolCall creates another Agent, allocate a stable a:<handle> in the current d/i/p Agent namespace instead and preserve creator Turn/Step/call only as origin metadata.

### 7.5 Normal continuation and parent/peer communication

Use jobs.send rather than another Agents API.

The Agent root's natural send behavior is:

    auto -> queue

Current Agent continuation requires no ref:

    jobs_send({
      input:{text:"Now compare against dotlink"}
    })

Equivalent explicit self:

    jobs_send({
      ref:".",
      input:{text:"Now compare against dotlink"}
    })

Meaning:
- durably accept one future Agent Input;
- never disturb the currently active Turn;
- if idle, the owner may attempt to claim the next FIFO Input immediately;
- if busy, preserve FIFO order until the next normal input opportunity;
- successful send proves durable Input custody only, not model consumption, Turn admission, provider selection, payment authorization, or tool authority.

Before any queued Input becomes a new Turn, the authoritative Agent owner must atomically revalidate the current:
- Session/Agent identity and branch writer epoch;
- source/lineage/recipient authority;
- model/provider/account availability;
- payer/budget/usage authority;
- ToolPolicy and delegated tool/resource ceilings;
- cancellation/closing/blocked/reconciliation state.

Only after those checks succeed is the Input bound to a new immutable Turn and journaled before paid provider effect. Queue acceptance therefore never freezes stale authority into a future capability. If current authority cannot be proven, no paid/model effect starts: the Agent remains/enters blocked with the Input still durably queued unless an authoritative policy explicitly terminally rejects that Input. Future reauthorization may make it claimable again without changing its original evidence.

**Bounded autonomous continuation:** a model-issued `jobs_send` to itself, a peer or a child that can start a future paid Turn is an *automatic continuation*, even if no `jobs.subscribe` rule fired. It requires explicit current delegated continuation authority and consumes the same finite causal-work allowance as subscription wakes and model-created Agent/Graph descendants. A root allowance (pending Input slots, total automatic Turns/model budget, child depth/work and causal hops) is allocated only by authenticated external user/host admission, within a fixed host ceiling; models, peers, tool text and descendants cannot mint/reset it. Reserve bounded queue/work capacity before durable Input acceptance, account for consumption at actual authorized Turn/child admission, and release unused reservations only through factual cancellation/retirement. The original Agent/Actor/TurnJournal owner maintains the causal root and monotonic spent/reserved budget, not a second workflow engine. Repeated self-send is allowed **only within** that explicit allowance; an ordinary model tool grant alone never implies unlimited future Turns. When exhausted, refuse new autonomous Input with a compact actionable status; existing paid/effect receipts remain intact. Hosts unable to meter a cross-owner/cross-host root atomically must reject that automatic path rather than splitting the allowance.

Parent communication is equally small:

    jobs_send({
      ref:"^",
      input:{text:"I found a cancellation race"}
    })

Peer/child Agent communication in the same namespace:

    jobs_send({
      ref:"a:2",
      input:{text:"Please review this change"}
    })

No caller needs to know the parent's canonical Agent ID to communicate upward. "^" resolves the current authorized coordination-parent relation exactly once at admission.

### 7.6 Precise steering: Turn ref + optional rev

The active Turn's natural send behavior is:

    auto -> steer

Every model Step, including one resumed by steer/interrupt or by tool receipts, references the Turn's current immutable AuthoritySegment and a StepAuthorityContext narrowed from it immediately before provider dispatch. A same-actor continuation naturally stays in the current segment. A cross-participant controller Input establishes a **new authority segment**; that segment governs the next Step **and every causally following tool-result/model Step** until another authenticated Input/control boundary replaces it or the Turn terminates. The Turn never silently reverts to its original actor. If required continuation context cannot be safely projected for the new segment, reject steer/interrupt before provider dispatch and require a queued/new Turn or approved-context Session.

steer means:
- target the exact active Turn;
- preserve and settle the current indivisible model/tool receipt Step **including all proposed ToolCall receipts**, never equate BeforeToolBatch with StepEnd;
- append a new **separately identified immutable controller-input receipt** to that same Turn, with issuer, source, request identity, admission revision and causal order; the original user prompt/provider-signed context remains untouched;
- create a new AuthoritySegment from that controller Input after current writer/model/provider/payer/tool/budget/disclosure revalidation; every subsequent causally derived Step in that Turn remains in this segment until another authenticated Input/control boundary replaces it;
- never mutate provider-signed evidence, invent a replacement Turn or invoke a second paid Agent loop.

Example:

    jobs_get({ref:"t:5"})
      -> t:5 turn running rev=7 step=2

    jobs_send({
      ref:"t:5",
      rev:7,
      input:{text:"Also inspect the symlink race"}
    })

Because t:5 is an active Turn, auto resolves to steer. The rev fences the observed Step/state. If Step 2 settles and Step 3 begins first, the Turn rev changes and this request returns stale.

Explicit steer:

    jobs_send({
      ref:"t:5",
      rev:7,
      mode:"steer",
      input:{text:"Also inspect the symlink race"}
    })

interrupt means:
- target the exact active Turn and redirect it at the earliest safe boundary;
- stop admitting additional work for the currently observed Step/state;
- request cancellation of cancellable owned work;
- settle already-started effects truthfully;
- append the steering Input to that same Turn only when the prior Step/state is safely settled enough for its next model Step.

Example:

    jobs_send({
      ref:"t:5",
      rev:7,
      mode:"interrupt",
      input:{text:"Stop waiting and use what we have"}
    })

queue against an active Turn preserves it and defers the input to a future normal Agent input opportunity. It is the only send mode on an active Turn that intentionally creates future-Turn input rather than steering the targeted Turn.

Before any next model Step after steer/interrupt, normal current writer/model/provider/account/payer/budget/ToolPolicy authority is revalidated; accepting the control Input never freezes future inference authority. If a started paid/model or external attempt cannot be proven settled, the **Turn stays `waiting`** (or `cancelling` if cancellation is pending) while the **Agent becomes `blocked`**; no automatic next Step is dispatched. Once the owner can factually settle the Turn, publish `settled` with its actual `unknown`/`mixed` outcome if required. `blocked` is never a public Turn state; the steering Input remains a factual accepted receipt. Durable input admission, consumption, model dispatch and terminal Turn settlement are distinct evidence stages.

If the exact Turn settles before a steer/interrupt is admitted, the request fails rather than retargeting to a newer Turn. A supplied stale rev likewise fails without retargeting.

### 7.7 Agent Job close versus Turn cancel

Two intents use the same generic jobs.cancel shape.

Whole current Agent:

    jobs_cancel({ref:"."})

The destructive target is always explicit; jobs_cancel({}) is invalid.

jobs_cancel({ref:"."}) closes/cancels the Agent Job itself and disallows future Turns.

Exact Turn:

    jobs_cancel({ref:"t:5", rev:7})

cancels Turn 5 only if it is still at the observed rev.

For Turn cancellation:
1. stop admitting new work for that exact Turn;
2. propagate cancellation to active descendants still explicitly owned by that Turn;
3. settle already-started external effects truthfully;
4. if cancellation is fully proven, Turn lifecycle becomes cancelled; its canonical execution outcome is fail cancelled when work had started, or skip cancelled when it was admitted but no material work started;
5. if any material effect is uncertain, do not claim cancelled: settle the Turn with canonical unknown/mixed outcome as appropriate and put the Agent into blocked/needs-inspection rather than automatically starting another Turn.

There is no Step-scoped public cancel. Precise Step-state fencing uses the Turn rev. Turn lifecycle and DL-150 outcome remain separate, so Agent Graph nodes always have one canonical outcome for when predicates.

Stale Turn/rev never affects a later Turn.

Whole-Agent cancellation records closing, rejects future admission, requests cancellation of the active Turn if one exists, and factually settles every queued-but-unconsumed Input as unconsumed/closed in the authoritative journal rather than silently dropping or executing it. Once owned work is safely settled, the Agent becomes closed while preserving all committed/unknown facts.

## 8. Child execution, Agent coordination and hierarchical cancellation

An Agent Turn may own durable child execution.

Example from current Agent /d:abird/i:pvl/p:zg/a:0:

    t:5
      call c3 -> Process Job t:5/j:1
      call c8 -> Wait Job    t:5/j:2
      call c9 -> Agent       a:2

For Process/Graph/Wait child Jobs the canonical path already records execution ancestry. Keep exact journal metadata too:

- parent Agent ref;
- parent Turn ref;
- parent Step;
- parent ToolCall;
- child resolved ref;
- source/authority generation.

For Agent creation, stable identity is a:<handle> rather than t:5/j:<handle>. Record the same origin evidence separately and establish an explicit coordination/ownership relation.

Two Agent relationships must remain distinct:

- origin: immutable provenance of where the Agent was created;
- parent/coordinator: current authorized communication/control relation used by "^".

Changing parent/coordinator never renames the Agent's canonical a:<handle>.

Turn cancellation propagates cancellation intent only to descendants/Agents still explicitly owned by that Turn through their authoritative control path. Path ancestry or origin alone is never sufficient cancellation authority.

A child's already committed or uncertain effect remains factual; parent cancellation cannot rewrite it.

**Transitive owned-effect barrier:** at an Agent Graph node's admission, capture one immutable **Graph-node execution scope** (original causal root + exact node/owner identity + bounded authority/footprint). Its initial Agent Turn and every Agent/ToolCall/Process/queued autonomous continuation derived from that scope retain an explicit **current ownership edge** to it, even though child Agents have independent stable `a:` identities. Reuse the existing child-owner/JobManager receipts and causal budget—no new executor, descendant ledger or per-call heap graph. A node may publish complete `ok|unchanged` and release its captured conflict footprint only when **all transitive effectful work and accepted future work under that scope** have factually settled/quiesced and required output/commit evidence is reconciled. An idle long-lived Agent identity alone does not keep the node waiting. A running, queued or unknown scoped effect does. The parent remains `waiting`/exposed, or reports truthful `unknown`/`incomplete` while holding necessary physical conflict protection; `N.*` cannot bypass it.

Initial vNext has **no implicit detach/transfer** of Graph-owned work, including queued model-generated Inputs; unsupported transfer fails admission, not at publication. While an Agent/descendant carries an **active Graph-node owner scope**, it may create bounded child Agents but may **not call `batch`** to hide a nested Graph. Its native/shell operations use the node's original effect and conflict bounds. Completion of the scoped initial Turn and descendants releases only their **current execution ownership**; the child Agent's stable identity and authorized coordination-parent link remain, with **no inherited ability to run later effects**. A subsequent Turn admitted from a genuinely new independent authenticated Input has a **fresh** causal root, ToolPolicy/payer/writer/footprint admission and conflicts independent of the settled Graph; it cannot reuse the old Graph's replay claim, authority or scheduling reservation. Previously queued Graph-scoped work cannot be relabeled as independent merely to release the barrier. Parent cancellation follows actual current ownership edges, not durable origin or path ancestry. No shadow owner manager or implicit background service is created.

## 9. Waiting, watch-like work and notifications

### 9.1 Waiting inside a Turn

A tool may wait without consuming model inference.

Example:

    Turn 5
      Step 2
        until(...)
          waiting
          ...
          condition satisfied
        tool receipt
      Step 3 model

While waiting:
- no new model Step runs;
- Agent Job reports waiting with exact Turn/Step;
- cancellation remains serviceable;
- no global Actor/runtime lock is held.

If the wait is durable/independently controllable or may outlive its original request, implement it as Job-backed child execution rather than an ad-hoc sleeping task.

No generic watch/until model tool is added by this plan; this section defines how such a future primitive must compose.

### 9.2 Notify is not a Job

Notify is a factual event projection delivered through existing Actor authority.

It does not:
- execute a model;
- create a Job;
- prove wake acceptance;
- imply new input authority.

### 9.3 Persistent watch

If future workloads justify a persistent independent watch that survives Turns or fires repeatedly, design it as a Job kind using the same jobs.* control plane.

Do not overload passive Notify or Agent history to simulate it.

## 10. Job events and wake subscriptions

Events are Job-kind capabilities, not a fixed Agent-centric global enum.

Common initial events:

    result
    settled

Agent additionally supports:

    attention

An AW-hosted Graph whose admitted topology contains an attention-capable Agent node may project that child's authenticated `attention` as a **Graph-scoped** event with its immutable `n:<coordinate>` and currently authorized child Agent/Turn ref. The preflight-captured host predicate is **false for completion-only Agent nodes** and true only when the actual delegated authority permits a blocking parent-attention/request-response path. Nonblocking parent `jobs.send`/Notify can remain authorized separately, but a completion-only child may not wait for a reply from its synchronously blocked parent; attempting such a wait must return a truthful unsupported/blocked result. The same immutable predicate governs proactive handoff, event capability and runtime enforcement; later policy widening cannot silently enable parent-blocking behavior for an already admitted node. Standalone dotlink Graphs do not invent Agent attention. This adds no new model-facing flag or event owner.

Meaning:

- result: a new consumable result became available.
  - Agent: one Turn committed a result.
  - finite Process/Graph: terminal result became available.
- attention: authenticated Agent request for external input.
- settled: the entire Job became terminal/closed.

Future Job kinds may extend the small event enum only through an intentional public-schema revision; arbitrary event strings are not accepted.

Subscription delivery reuses the same send-mode vocabulary:

    auto
    queue
    steer
    interrupt

Example rule:

    ["result","queue"]

means: when this source event occurs, admit the corresponding bounded factual wake Input to the authenticated receiving Agent using queue semantics.

    ["attention","steer"]

means: when attention occurs, resolve the receiving Agent's current active Turn at delivery, bind that exact Turn/ref and its current revision atomically, and steer at the next safe Step boundary.

For interrupt, the receiver owner similarly binds the exact current Turn/revision at delivery before requesting earliest-safe redirection. The subscriber does not predeclare Agent Turn/Step identifiers in the source subscription.

auto is event/recipient-policy specific and should choose the safest normal behavior. Initial defaults should favor queue for ordinary result/settled events; attention may choose steer only when the current policy explicitly permits interactive wake.

For finite Process/Graph, result and settled may occur at the same factual checkpoint. If one subscription requests both, coalesce to **one** wake admission for that terminal checkpoint/recipient. If their requested delivery modes differ, reject that combination at subscription validation rather than making an event-order-dependent steering decision. An AW Graph may project child-Agent attention only when its captured topology marks that node attention-capable and the event has real immutable Agent origin evidence; that alert is not stdout or a second child runner.

For Agent:
- each Turn can emit result;
- attention can occur during an active Turn;
- settled only occurs when the Agent Job itself closes.

Subscription lifetime is explicit:
- a rule is persistent until replaced/cleared, source terminality makes it irrelevant, or recipient/source authority is revoked;
- Agent result rules therefore persist across multiple future Turns;
- finite Job rules naturally retire after terminal delivery/settlement;
- registration is non-retroactive and never replays already-past events.

Every publishable event is committed atomically with an externally meaningful revision of its source resource. `event_rev` is that resulting source revision; it is stable for replay and never synthesized by the transport. If result and settled are published at one finite terminal checkpoint they may share the same source rev while remaining distinct event names.

**One source-owned registration/commit boundary, one recipient-owned Input boundary:** the existing authoritative source Job owner serializes constructor `wake` registration **before first effect**, subsequent `jobs.subscribe` replace/clear, and event publication using its own Job revision/transaction. A source that cannot serialize registration with publication is **not wake-capable** and rejects the rule before effects/registration; AW cannot infer wake from polling or asynchronous arrival. Keep the reusable contract small: a source-side `register/commit_event` owner hook plus a receiver-side idempotent `admit_input` callback, using existing Job/Actor journaling—not another event bus, subscription database, generic plugin engine or paid listener loop. The source's committed event captures recipient, **subscription generation/mode**, source ref, event and `event_rev` in a bounded pending-delivery receipt. Replacements/clears affect **future** event commits only. The receiving AW Agent owner checks current recipient rights, writer fence, causal/payer budget and disclosure before converting that committed fact into one durable Input; it may suppress delivery after revocation without altering source history. Duplicate delivery of the captured event cannot admit a second Input or paid Turn. For finite live-session Jobs, source loss reports unavailable/unknown delivery, never falsely promises cross-restart wake.

Wake dedupe identity is exact (canonical source ref, event, event_rev, canonical recipient Agent ref, captured subscription generation). Redelivery of the same committed event cannot create a second wake Input or paid Turn. A subscription replaced before event commit selects the new generation; one replaced after event commit does not reinterpret that event. If the source is an AW Graph relaying child attention, record the source node/child origin without adding a second child event owner.

**Subscription state/event ordering are source-owned; recipient authority and Input custody are receiver-owned.** Verify recipient grants at registration and again at delivery; never copy the source's subscription state into another receiver-side registry. A queued wake is an ordinary replay-stable AW Input, **not** inference authority: every later Turn claim revalidates current writer/provider/account/payer/budget/ToolPolicy bounds.

**Causal loop and work admission:** a subscription wake is one form of the **same model-origin autonomous continuation** bounded in Section 7.5. A wake Input, any subsequent model-issued self/peer `jobs.send`, and model-created child Agent/Graph work carry the original authenticated causal root and its remaining allowance through immutable Input/Turn/ToolCall evidence. Automatic wake cycles (A -> B -> A) are rejected when the recipient recurs in that wake chain; repeated direct self-continuation is permitted only under the finite explicit self/peer/delegation allowance, never an infinite bypass. Apply host-capped causal hops, recipient pending Inputs, total automatic Turn/child-work/payer budget and rate at acceptance/admission; exhaustion suppresses *new* autonomous work but leaves source events and factual effects inspectable. No automatic subscription is installed by default. Only an independently authenticated external user/host Input can open a new causal root; model text or delegated work cannot. The original AW Actor/Session/journal owner meters it; no second event database or model execution engine.

`jobs.subscribe` replaces/clears the future rule in the source owner, with original invocation replay lookup before contextual-ref resolution. **Initial AW capability:** AW-hosted finite Process/Graph sources whose existing Job owner has source-serialized event commits and AW Agent sources whose Session/Actor owner has durable event revisions; the recipient must be one real authorized AW Agent. No wake support is advertised for an owner without that exact contract. Dotlink-owned remote Process/Graph sources remain passive until their own fenced source-event protocol is implemented. Unknown delivery never triggers blind paid inference replay.

**AW↔dotlink bridge boundary:** initial vNext offers subscribed/wake events only for sources with an authoritative **AW-hosted, source-serialized event owner**. A dotlink-owned live Process/Graph source is **not wake-capable** merely because its caller has an authenticated AW Agent: reject constructor `wake` **before its first Process/Graph effect**; reject a later `jobs.subscribe` against an already-running remote source **before registration or any wake Input delivery**. Do not implement an AW-side poller that could miss a fast terminal event. Passive `jobs.get/read` remain available through the bridge. A future capability can be enabled without a new public tool only if dotlink's source owner supplies a small private atomic register-before-start / register-or-replace-and-event-commit protocol that returns stable source generation/revision and captured recipient subscription generation/mode, delivers deduplicated factual events, and reports owner-loss/unknown honestly. Until that source-fenced protocol exists, do not advertise/promise remote wake or pretend transport arrival time is source event time.

## 11. Universal jobs.* control plane

### 11.1 jobs.list

Provider:

    jobs_list({cursor?, limit?})

Semantic:

    jobs.list

jobs.list is bounded flat discovery, not namespace traversal. Its AW scope is exactly the caller's authenticated canonical d/i/p namespace, filtered by current per-Job authority:
- live/addressable Agent roots from authoritative SessionStore;
- exposed finite Process/Graph Jobs owned by authorized Agents in that same d/i/p.

Coordinator/parent changes never broaden discovery, and crossing d/i/p is never implicit. It does not list Turn or Graph-node descendants; use jobs.get(ref) for one Job's current actionable descendants. Standalone dotlink lists only exposed finite Process/Graph Jobs in its authenticated live-runtime session.

Ordering is immutable newest **project-scoped list exposure sequence** first, canonical ref only for a deterministic tie-breaker. One lightweight AW host listing/routing index assigns that sequence across Agent roots and finite Job roots; SessionStore's original creation evidence and JobManager's original admission remain the respective factual owners. Publication of a real Agent/index entry is one admission unit; finite Jobs reserve bounded exposure capacity before effects and publish exactly once. The index stores only ref/owner/position/tombstone, never copied Agent or Graph state. Standalone dotlink uses its own local finite exposure order. New exposure cannot reorder older entries.

cursor is opaque, copyable and owner-verifiable. It binds the exact canonical d/i/p scope or standalone runtime generation, authenticated listing authority epoch, exposure position/tie-breaker and projection version. The next page revalidates current authority; scope/runtime/authority mismatch or tampering rejects rather than continuing elsewhere. Newly exposed Jobs do not shift an older page. Expired/revoked resources may create gaps, but one valid traversal never aliases an identity or returns the same exposure twice.

Compact model examples from current Agent:

    . agent idle rev=9 turn=t:4
    a:2 agent running rev=12 turn=t:2
    t:4/j:1 bash running rev=4
    t:4/j:2 graph running rev=9 3/7
    more cursor=...

A short title may be included only if measured recovery workflows show it lowers total model/tool rounds. Never include bulk arguments/output.

### 11.2 jobs.get

Provider:

    jobs_get({ref?, rev?, wait_ms?})

Default ref is ".".

Semantic:

    jobs.get

Status/state/progress only.

Examples from current Agent:

    . agent running rev=17 turn=t:5 step=2 queue=1
    t:5 turn running rev=7 step=2
    t:5/j:1 bash running rev=4
    t:5/j:2 graph running rev=9 3/7
    t:5/j:2/n:4 bash running rev=3
    a:2 agent idle rev=9 turn=t:2
    ^ agent idle rev=12

Model-facing get emits the shortest safe contextual refs from current Agent context whenever possible. If contextual shortening would be misleading or unavailable, emit the fully resolved canonical ref.

SDK returns typed metadata and may additionally expose canonical_ref so application code never has to parse the compact model renderer.

Root get exposes only compact current/actionable descendants by default, not historical execution trees.

Conditional observation:
- without rev/wait_ms, return current snapshot immediately;
- with rev and current rev differs, return current snapshot immediately;
- with rev equal and wait_ms>0, atomically wait until rev changes, resource settles/closes, or timeout;
- with wait_ms>0 but no rev, atomically snapshot current rev and wait for the next meaningful change while the resource is active;
- timeout returns the latest factual snapshot with the same rev when unchanged.

This is observation only: jobs.get never returns stale merely because state advanced. wait_ms never changes execution timeout and uses one owner-specific snapshot/subscribe boundary so a transition cannot be lost between inspection and wait.

No bulk data, transcript, stream bytes or ACK.

Progress reuses DL-150's compact factual grammar: absolute cumulative snapshots, fixed denominator only when naturally known, bare cumulative count for unknown total, optional terse phase, never guessed percentages. Progress/notifications are ephemeral; the final Job/Turn result remains authoritative even if no progress was observed.

### 11.3 jobs.read

Provider conceptual shape (identical to the normative Section 2.6 schema):

    jobs_read({
      ref?,
      view?,
      filter?,              # Agent-history only; mutually exclusive with view
      cursor?,
      offset?,
      limit?,
      encoding?,
      wait_ms?
    })

There is **no top-level `tools`** field. Exact tool-name matching exists only inside Agent-history `filter` custom objects. Standalone dotlink omits `filter` entirely.

Default ref is ".".

Semantic:

    jobs.read

The generated schema stays small. Kind/path-specific view validity is checked after canonical ref resolution; the common schema must not inline every kind's metadata union. The optional `filter` is advertised only by an AW host that can serve Agent history; standalone dotlink does not advertise it.

**Natural defaults are normative:** Process and Process-backed Graph nodes -> stdout; Graph/native nodes -> result; Agent root/Turn -> latest committed public Turn result. An explicit invalid view fails, never silently substitutes another. Status stays behind jobs.get and ACK stays explicit. For one category, `view` remains the shortest model call; `filter` is for combined/negative history selection that would otherwise require multiple calls or unbounded trace.

Initial specializations:

Process Job:
    result
    stdout
    stderr

Graph Job:
    result

Graph n:<handle> ref:
    result
    stdout/stderr where that node supports them

Agent root/Turn:
    result
    messages
    tool_calls
    tool_outputs
    trace

Future Job kinds define their own explicit view set; no untyped arbitrary field dump.

#### Retained byte reads

For Process/Graph byte streams:
- offset is raw retained-byte position;
- encoding is text or base64 where relevant;
- base64 is exact bytes; text is the readable UTF-8 projection defined by the retained-result specialization and never changes raw offset accounting;
- wait_ms may wait for bytes/EOF using lost-wakeup-safe owner observation;
- read never ACKs;
- exact next offset is independent of presentation encoding;
- retained truncation/omitted-byte facts remain immutable even after ACK releases the retained prefix.

#### Record/history reads

cursor is an opaque copyable continuation for record/history pagination. It is owner/view-specific and never a natural-language mini-DSL. A cursor binds the **fully resolved canonical Agent/Turn ref, exact selected view or canonical Agent history filter (including tool operation selection and inversion), immutable record identity/intra-record offset, projection format version and effective disclosure epoch**. It is bounded, integrity-protected or deterministically verified by the original owner; no second cursor/session database is needed. The next read revalidates current disclosure and rejects a stale/modified cursor rather than mixing historical pages under different permissions. A raw Process stream offset remains a separate ordinary integer, not this cursor.

History pagination preserves DL-150 atomic-record truthfulness:
- ordered public records are messages, tool calls, tool outputs, trace events and results;
- the page's semantic byte budget is independent of `limit` (which counts records/Turns) and transport framing; a single huge result/ToolRecord must be pageable without dumping an entire Turn or silently claiming completeness;
- whole records are returned whenever they fit;
- if the first selected record itself exceeds the semantic result budget, only its payload/content may be emitted as an explicit fragment with stable record identity; structure/IDs/status are never silently truncated;
- the opaque cursor may therefore identify either the next record or an exact intra-record payload position;
- every fragment carries an advancing more cursor and never pretends the record/view is complete;
- if a record kind cannot be safely fragmented by its typed projection, return a concise actionable over-limit error rather than lossy output.

Agent root historical reads use cursor/limit. Exact Turn reads use t:<handle>. Step remains journal metadata inside Turn views rather than another public path segment.

Default current-Agent read:

    jobs_read({})

returns the latest committed Turn result only.

Examples:

    jobs_read({
      view:"messages",
      limit:3
    })

    jobs_read({
      ref:"t:5",
      filter:{tool_calls:true,tools:["read","grep"]}
    })

    jobs_read({ref:"a:2",filter:{result:true,tool_calls:true}})
    jobs_read({ref:"a:2",filter:{tool_outputs:true,exclude:true}})
    jobs_read({ref:"a:2",filter:"none"})

    jobs_read({
      ref:"a:2/t:2",
      view:"trace"
    })

    jobs_read({
      ref:"t:5/j:1",
      view:"stdout"
    })

**One filter contract for Agent history:**
- omitted filter + omitted view, or `filter:"default"`, returns only the latest committed public result (default one Turn). `filter:"none"` requests all **currently authorized sanitized** public categories, still bounded by the same Turn/byte ceilings; it never disables redaction or accesses hidden provider context.
- custom `filter:{result?,messages?,tool_calls?,tool_outputs?,tools?,exclude?}` uses true category booleans as its matching predicate (false/omitted means not matched); require at least one true category. `tools` is an optional bounded unique list of **flat model tool names** (read, grep, bash, agent, ...) that restricts matching records within true tool categories; reject tools without a true tool_calls/tool_outputs category. The trusted owner maps only verified semantic operation identities and versioned historical provider-wire mappings; never derive authority from untrusted argument text.
- default custom `exclude:false` includes matches only, with no implicit result. `exclude:true` removes matches from the **currently authorized** public projection; a true tool category plus `tools` removes only those operations, not the entire category. Excluding inaccessible fields does not require disclosure grants to read them. Explicit positive selection of denied categories fails, never silently grants access.
- combined records preserve one chronological order, include the final assistant answer once even when both result and messages match, and retain original tool call IDs and factual truncation. `view:"trace"` remains the explicit unfiltered chronological view when all its disclosed categories are authorized. `filter` is an alternative selector, not usable with `view`; no top-level `tools`/`include` alias is advertised.
- Cursor identity binds exact normalized `filter` (including exclusion and tool selection) **or** the selected `view`, plus source/projection/disclosure epoch; changing selection invalidates continuation. Filtered reads stay in the original SessionStore/Turn/ToolRecord projection, not a new transcript/cache engine.

Agent historical reads are durable sanitized SessionStore projections, not raw provider traces and not retained Process spool bytes. The projection uses **explicit allowlisted public fields** from original Turn/ToolRecord evidence, never a serialized raw Turn with redaction by deletion. Current permission for result, messages, tool_calls, tool_outputs and trace is checked independently; `trace` requires the combined permissions for all data it actually reveals, not an implicit superset grant. Tool arguments, stored historical output, user/assistant text and originally authorized but now revoked resources are each rechecked before disclosure. Hidden reasoning, system/context blocks, provider continuation, credentials, payer/account secrets and unauthorized branch content remain unavailable even in an otherwise public trace. A denied/redacted/truncated projection is labeled as such, not represented as a complete original transcript.

#### Active Agent Turn

jobs.read does not expose provider-private in-flight state.

Committed previous Turns remain readable if the authoritative store can provide a race-free view without weakening writer fences.

If the selected active Turn is unreconciled, fail closed or expose only explicitly public committed prefixes proven safe by the underlying owner. Initial implementation should prefer the existing safe branch/history fence rather than inventing an in-flight transcript reader.

Use jobs.get for live state.

### 11.4 jobs.ack

Provider:

    jobs_ack({ref, view?, offset})

Semantic:

    jobs.ack

ACK requires an explicit ref to an exact ackable retained byte owner.

Examples:
- t:5/j:1 stdout/stderr/result;
- t:5/j:2/n:4 retained byte/result streams where that Graph node exposes ACK.

Agent durable history views are not ACK-consumed.

Attempting ACK on messages/tool_calls/tool_outputs/trace is invalid.

ACK is idempotent on exact ref/view/offset and may be retried after a lost response even if internal cleanup state advanced. The requested watermark must never exceed bytes that were actually retained **and made available/delivered for that caller's exact output lease**; an ACK is not permission to skip unseen data or clear another owner's result. Physical cleanup waits for every exact owner/lease already required by replay/MCP Task semantics. Expired content returns a truthful unavailable/retired status with immutable terminal/truncation facts intact.

### 11.5 jobs.send

Provider:

    jobs_send({
      ref?,
      rev?,
      mode?,
      input
    })

Default ref is ".".

mode:

    auto        # default
    queue
    steer
    interrupt

Generic semantics:

- auto: use the resolved resource's safest natural input behavior.
- queue: accept input for the next normal input opportunity without altering active execution.
- steer: preserve the current indivisible decision/effect unit, then redirect at the resolved resource's next safe steer boundary.
- interrupt: redirect at the earliest safe boundary, settling/cancelling already-started effects truthfully first.

There is no live/ready/next/after/finish/complete/now/step/turn delivery vocabulary in the common API.

rev is optional but encouraged when the caller is acting on observed state. Omitted rev means atomically use current state; supplied rev is compare-and-act.

Initial specializations:

Agent root (., ^, a:<handle>, contextual structural overlay resolving to an Agent, or fully resolved canonical /d:<h>/i:<h>/p:<h>/a:<h>):
- Input = {text}.
- auto -> queue.
- queue means durable FIFO future Input; idle Agent may claim it immediately.

Active Agent Turn t:<handle>:
- Input = {text}.
- auto -> steer.
- steer settles the current Step, journals the steering Input in the same Turn, then redirects before another Step begins.
- interrupt requests earliest-safe settlement/cancellation, then journals the steering Input in the same Turn before its next Step.
- queue preserves the active Turn and defers the Input to a future normal Agent Turn.
- Turn rev optionally fences the observed Step/state; a settled Turn is never retargeted.

Process Job .../j:<handle>:
- Input = {text} | {eof:true}.
- auto means immediate live stdin behavior.
- queue/steer/interrupt unsupported initially unless a future Process specialization explicitly defines them.

Graph node .../j:<handle>/n:<handle>:
- input support delegates to the node specialization.
- a Bash node therefore auto-sends live stdin.
- a noninteractive read node rejects send.

A stale ref target or stale supplied rev fails without retargeting.

### 11.6 jobs.cancel

Provider:

    jobs_cancel({ref, rev?})

ref is required. "." remains the explicit spelling for current Agent self-cancel.

Generic meaning: request cancellation of exactly the canonical resolved resource.

- Process Job j:<handle>: terminate through original Process custody.
- Graph Job j:<handle>: cancel whole Graph Job.
- Graph node n:<handle>: supported only if that exact node owner exposes safe node cancellation; otherwise reject.
- Agent root a:<handle>: close/cancel whole Agent Job and prevent future Turns.
- Agent Turn t:<handle>: cancel that exact Turn while retaining the Agent Job if settlement is safe.
- no public Step cancellation target exists.

Cancellation is idempotent intent, not rollback.

rev is optional. A stale supplied rev returns stale and does not fall forward to a newer resource state.

### 11.7 jobs.subscribe

Provider:

    jobs_subscribe({
      ref,
      rev?,
      events:[
        [event, mode?],
        ...
      ]
    })

ref is required because it names the event source. Omitted event mode means auto. Agent source==recipient is invalid; self-result/self-attention subscriptions cannot create autonomous wake loops.

Examples:

    events:[
      ["result","queue"],
      ["attention","steer"]
    ]

Event names and modes are capabilities of the resolved resource kind. Unknown event/mode combinations fail validation.

An empty events array clears the current recipient's future rule for that source ref.

No retroactive event firing.
Current recipient authority is checked at registration and again at delivery.
steer/interrupt delivery resolves the receiving Agent's exact current Turn and optional revision fence at delivery before acting.
Unknown delivery never blindly executes another paid Turn.

## 12. jobs.read internal typed result

The common semantic result is typed and transport-neutral.

Illustrative shape:

    enum JobReadPage {
      Process(ProcessReadPage),
      Graph(GraphReadPage),
      Agent(AgentReadPage),
      ...
    }

ProcessReadPage contains:
- Job/view identity;
- exact retained bytes or terminal result;
- raw offset/next offset;
- truncation/EOF;
- ACK capability.

GraphReadPage contains:
- canonical resolved ref/view;
- kind-specific Graph metadata for the resolved Graph Job/node where needed;
- typed Text/ExactRead/Bytes/Media result;
- exact retained paging facts;
- node/Graph status facts as needed without duplicating jobs.get.

AgentReadPage contains:
- canonical Agent/Turn ref;
- selected single view or normalized Agent-history filter;
- one or more TurnView records or explicit bounded record fragments;
- exact historical cursor.

Record fragments are typed, never silent truncation: they carry stable record identity, the payload field being continued, exact fragment position/completeness, and next cursor. SDK consumers never infer a complete object from a fragment.

Agent Turn views:

    result:
      turn_ref
      outcome
      final public result
      bounded public error/reconciliation marker

    messages:
      turn_ref
      outcome
      ordered sanitized public user/assistant messages

    tool_calls:
      turn_ref
      ordered call records:
        call id
        original provider wire identity where authorized/audit-needed
        canonical semantic operation
        authorized arguments
        effect
        status

    tool_outputs:
      turn_ref
      ordered output records:
        call id
        canonical semantic operation
        status
        typed content/summary
        error/truncated marker

    trace:
      turn_ref
      chronological discriminated public events:
        message
        tool_call
        tool_output
        result
      with no duplicate final answer/tool payload

Hidden chain-of-thought, provider_context, system messages, credentials, private host context and unauthorized historical content never enter JobReadPage.

## 13. Model renderer versus SDK projection

There is one semantic JobReadPage and two projections.

### 13.1 Model-facing

Optimize for tokens and copyability.

Default Agent result:

    t:5 ok
    <answer>

Messages:

    t:4
    user
    Review cancellation.

    assistant
    I found two races...

    t:5
    user
    Compare it with dotlink.

    assistant
    ...

    more cursor=...

Tool calls:

    t:5
    c1 read ok {"path":"src/job.rs"}
    c2 grep ok {"pattern":"cancel","path":"src"}

Tool outputs:

    t:5
    c1 read ok
    <bounded content>

Trace:

    t:5
    user
    ...

    assistant
    ...

    call c1 read {...}

    output c1 ok
    ...

    result ok
    ...

Use separate content/status blocks when exact bytes would be corrupted by appended control text.

No repetitive JSON envelope per line unless the transport requires it.

### 13.2 SDK / Code Mode

Return structured typed values.

Illustrative messages result:

    {
      ref: ".",
      canonical_ref: "/d:abird/i:pvl/p:zg/a:0",
      kind: "agent",
      view: "messages",
      turns: [
        {
          ref: "t:4",
          canonical_ref: "/d:abird/i:pvl/p:zg/a:0/t:4",
          turn_ref: "t:4",
          outcome: "ok",
          messages: [
            {role:"user", content:"..."},
            {role:"assistant", content:"..."}
          ]
        }
      ],
      next: {cursor:"..."}
    }

Tool call result for exact Turn:

    {
      ref: "t:5",
      canonical_ref: "/d:abird/i:pvl/p:zg/a:0/t:5",
      kind: "agent",
      view: "tool_calls",
      turn_ref: "t:5",
      calls: [
        {
          id:"c1",
          step:2,
          operation:"fs.read",
          arguments:{path:"src/job.rs"},
          effect:"read",
          status:"ok"
        }
      ]
    }

Tool output result:

    {
      ref:"t:5",
      canonical_ref:"/d:abird/i:pvl/p:zg/a:0/t:5",
      kind:"agent",
      view:"tool_outputs",
      turn_ref:"t:5",
      outputs:[
        {
          callId:"c1",
          step:2,
          operation:"fs.read",
          status:"ok",
          content:{type:"text", text:"..."},
          truncated:false
        }
      ]
    }

Trace result:

    {
      ref:"t:5",
      canonical_ref:"/d:abird/i:pvl/p:zg/a:0/t:5",
      kind:"agent",
      turn_ref:"t:5",
      view:"trace",
      events:[
        {type:"message", role:"user", content:"..."},
        {type:"tool_call", step:2, id:"c1", operation:"fs.read", arguments:{...}},
        {type:"tool_output", step:2, callId:"c1", status:"ok", content:{...}},
        {type:"result", outcome:"ok", content:{...}}
      ]
    }

Binary/media SDK content uses bytes/Uint8Array/native equivalents, not forced text/base64 where the language binding can carry bytes.

The SDK does not parse the compact model renderer.

## 14. Agent Graph nodes

`agent` is explicitly **model-facing Graph-eligible on AW**, in addition to the reviewed DL-150 native/shell leaves. It is never standalone-dotlink-visible. **Nested `batch` is not Graph-eligible on any surface in initial vNext**; Agents can spawn authorized subordinate Agents, subject to inherited finite depth/cost/causal budgets and cycle checks.

A Graph Agent node:
- creates one stable Agent a:<handle> in the current d/i/p Agent namespace and admits its initial Turn under the Graph-node owner/causal scope; it may create child Agents but cannot run an indirectly nested `batch` while that scope is active;
- records Graph/node/Turn/Step/ToolCall origin metadata separately from the Agent's stable identity;
- is one immutable Graph node invocation;
- node terminal status corresponds to that admitted Turn **plus its owned-effect settlement barrier** (Section 8), not the lifetime of the long-lived Agent Job;
- may return/expose the stable Agent ref (for example a:2) as a continuation handle when authorized;
- does not keep the Graph node running merely because the Agent Job remains idle afterward, **but does remain waiting while any effectful child it still owns can affect dependent nodes**.

Graph `when` predicates retain DL-150 semantics: bare `N` succeeds only on complete `ok|unchanged` with no `more`/`incomplete`. In-progress or still-queued scoped descendant work cannot satisfy it. If an uncertain external effect might still be active, mark it truthful `unknown`/`incomplete` and keep conflicting dependents fail-closed until the original owner can prove safe release; an explicit `N.*` does not override physical conflict/authority checks. No implicit background detach or released footprint on mere model-Turn completion.

**Proactive parent-controller handoff:** at Graph admission, before roots run, use the same captured `attention_capable` predicate and actual current controller grants. If a child may block awaiting its **only** controller (the invoking Agent) while that Agent synchronously awaits the Graph, expose and return the **same Graph Job handle immediately** so the parent's ToolCall/Step can finish. Ordinary completion-only Agent nodes remain eligible for optimistic foreground completion without an extra model/tool round. A separate authorized controller may permit foreground waiting, but losing all controllers causes bounded same-Job handoff or truthful blocked settlement, never an indefinite wait. A direct foreground `agent(...)` constructor follows the same rule if it may await its launching Agent as sole controller. Authorized Graph attention includes actionable `n:<coordinate>` and child Agent/Turn ref without another Job or paid model callback. Requested wake registration commits before roots/initial child Turn start.

### 14.1 Agent conflict footprint

An Agent is adaptive; task text cannot define its filesystem footprint or arbitrary external effects.

At Graph preparation derive the conservative Agent node footprint from the **entire captured effective delegated authority ceiling**, not task text or only native filesystem grants. Include native filesystem roots and access modes, permitted Process interpreters and subprocess filesystem reach, network/external MCP/extension effects, and further child delegation. Trusted resource bounds can narrow a known footprint; advisory shell `writes` does not become authority or a proof of confinement.

Examples:

    child has read-only /repo
      -> Read(/repo)

    child has read-write /repo
      -> ReadWrite(/repo)

    child has disjoint /repo/src + /tmp/build
      -> conservative union

If the host cannot prove an exact bounded footprint, classify the Agent node as **opaque** and conservatively conflict with every Graph node whose observable effects could interact, including native filesystem, Process and other Agent nodes. This is a scheduling barrier within the Graph, not an assertion that arbitrary external services are transactional. No task-text analysis or advisory `writes` hint may bypass the barrier.

Revalidate the captured delegated ceiling immediately before the node's first effect/Start. A changed/revoked topology settles unlaunched rather than executing under stale conflict assumptions.

This is required to preserve DL-150 deterministic Graph behavior.

External side effects cannot be made transactional merely by serial Graph scheduling. The opaque classification nonetheless **prevents the scheduler from introducing unrequested overlap** with interacting work; explicit `when` dependencies provide additional ordering where a workflow requires it. Revalidate the original captured grants and current revocation before every delegated effect and child spawn; a later expansion never widens an already admitted Graph node. This extends DL-150's conservative ShellNode footprint rule without a second conflict engine.

## 15. Effects and authority

Trusted static effect classes remain:

    Read
    Write
    Process
    Control
    Unknown

Examples:

    fs.read                    Read
    fs.write                   Write
    bash/pwsh/powershell       Process
    agent                      Process
    jobs.list/get/read         Read
    jobs.ack/send/cancel/
      subscribe                Control
    batch                      Control outer admission + exact leaf effects

Effect is not sufficient authority.

Examples:
- jobs.read Agent history additionally requires exact resolved Agent/Session/branch disclosure authority on every page; cursors never freeze disclosure authority;
- jobs.send Process additionally requires exact resolved Process/input custody **plus current independent `process.input` authority and the launcher's source/epoch/resource ceiling against its actual retained sandbox/privileges**; a revoked or narrowed current grant cannot be bypassed by an already-open stdin handle. If unsafe, reject new input or use separately authorized safe shutdown; never re-send ambiguous bytes;
- jobs.send Agent additionally requires exact resolved Agent/Turn/Actor Input authority;
- accepting Agent Input does not authorize future inference: every queued Input claim revalidates current Session writer/source lineage/model/provider/account/payer/budget/ToolPolicy/resource ceilings before immutable Turn admission;
- jobs.cancel Agent Turn additionally requires exact Turn generation ownership;
- jobs.subscribe additionally requires current receiving Agent wake authority, and delivery revalidates it again.

**All ToolPolicy decisions flow through the one scoped Policy → Decision → Execution pipeline in Section 2.5.1.** `ToolPolicy` remains the exact source/effect/site grant enforcement materialization, not an independently configurable second execution engine. Dynamic guardian `Allow/Deny/Ask/Narrow`, chosen Local/DelegateAgent route, provider, payer, execution site, limits and disclosure derive from the same effective scoped context and generation fences. Dynamic policy may deny, narrow or ask within the static ceiling but never mint missing authority. A durable ref, queued Input, subscription, prior parent relation or historical execution authority is never a capability token for future paid/model/tool execution.

**Control is a first-class static effect, not an alias for Write or Unknown.** Implementation must add it exhaustively through the portable authority stack: kernel Effect, agent-core ToolEffect, shared aw-tools effect where used, ToolSpec/Envelope/grant matching, adapter conversions/ranks, journal/store/portable projections and scheduling/read-wave classification. Control must not join Read parallel waves and must not inherit filesystem Write authority. Existing ReadOnly/Off behavior, exact source/site/epoch/actor/resource grants and non-read scheduling barriers remain fail-closed. There is no pre-1.0 compatibility encoding/migration for an older effect enum; update the current code/schema/tests atomically.

Read-only Graph outer Control admission is separate from every leaf's actual Read/Write/Process authorization; unconditional and conditional mutating leaves fail preflight before any root starts. Keep out-of-band host cancellation serviceable while an Agent/Job is waiting.

## 16. Read-only semantics

Filesystem ReadOnly is orthogonal to execution Control.

A session may have:

    fs read          allowed
    fs write         denied

    read-only batch  allowed with outer Control + all Read leaves

    jobs.get/read    allowed for owned Jobs
    jobs.cancel      independently granted
    jobs.send        denied

    agent            optionally allowed under bounded delegation
    bash             denied

Do not implement global ReadOnly as ToolEffect::Read-only.

## 17. Replay and lost responses

Every Job-producing constructor **and any potentially paid guardian/router preparation** must establish the original replay claim before external effects or paid inference. The claim keys the unchanged original request, source and target; revised arguments/route/policy generations are immutable accepted evidence *inside* that claim, not a second retry key. A crashed/unknown guardian attempt cannot be replayed as a fresh charge without independent explicit authorization.

Direct Process/Graph:
- matching redelivery reattaches to same execution;
- changed semantic request under same replay identity fails closed;
- ambiguous response delivery follows the same hidden-owner/promote-to-exposed lost-receipt rule as Section 3 rather than rerunning work.

Agent creation:
- stable canonical Agent ref + exact Session/branch binding + Turn t1 admission are recoverable before paid provider effect;
- allocation of a:<handle> is replay-stable, so matching redelivery never creates a second Agent or paid Turn;
- parent/origin metadata is bound to the same replay claim.

Agent continuation via jobs.send:
- requested_ref is canonicalized once to exact resolved Agent/Turn ref before durable Input acceptance;
- optional rev fences the observed state but is not required for correctness;
- duplicate accepted continuation must not create two queued Inputs or two next Turns;
- changed request under the same control identity fails closed;
- accepted Input and its replay identity remain factual even if later Turn admission is blocked by changed authority.

When a queued Input is claimed, the owner establishes a distinct replay-stable Turn admission identity only after current writer/inference/payer/tool authority succeeds and before paid provider effect. A crash/lost response may recover that same admission without re-resolving an already accepted target, but stale queued authority can never silently authorize another Turn.

All effectful control delivery is replay-safe. **Invocation identity is host-envelope authority, not a model argument.** Provider calls bind the original provider call ID to the exact Session/Turn, source/site/epoch and transport request; SDK or AW-bridge clients establish a fresh stable invocation ID **before** sending any effectful control and reuse that same ID for automatic retries or explicit retry of the same intent. The SDK owns its prepared-call/retry context; direct calls with new IDs are new intents even when arguments match. Never infer idempotency from matching text, canonical refs or a now-stale rev alone. If an SDK/transport cannot retain an original identity across an ambiguous failure, it must not blindly retry a side-effecting operation; it may query factual status/receipt or fail closed. No added model-facing `request_id` field or separate Agent runner is needed.

**Replay identity is checked before new revision-state admission:** for a matching, already-admitted send/cancel/subscribe request, return its original factual receipt even if the resource revision later advanced; only a genuinely new control request with a stale supplied rev fails. The recipient's current authority is still required to disclose the old receipt. Changed arguments under a reused request ID fail closed. For Process stdin/EOF, an ambiguous external write may remain `unknown`; the host may not write it a second time merely to manufacture a confirmed response.
- jobs.send establishes control replay identity before applying Agent Input, Process stdin/EOF or delegated Graph-node input; matching redelivery returns the same control receipt and never writes bytes/input twice;
- changed send content/mode/ref under the same request identity fails closed;
- jobs.cancel is idempotent cancellation intent on the exact resolved ref;
- jobs.subscribe replacement/clear is idempotent for exact source/recipient/rule request identity;
- jobs.ack needs no separate replay ledger because exact ref/view/offset is itself monotonic idempotent state.

Agent Turn cancel/send idempotence is keyed to exact canonical Agent/Turn ref + request identity (and rev when supplied), never whichever Turn happens to be current later.

## 18. Retention

Finite Process/Graph Jobs use bounded live-session retention consistent with current shared Job ownership. Their stream bytes may expire after all exact owners release and retention policy allows.

Agent Job durable Turn history is owned by SessionStore and can outlive finite spool retention. jobs.read routes to the authoritative owner.

This difference is intentional. Job is an addressable execution resource, not a promise that every kind has the same storage lifetime.

jobs.get/read must report expired/unavailable data truthfully.

No cross-process resurrection of live finite Process/Graph Jobs is implied.

Agent Job durable identity may be rediscovered from its authoritative Session mapping after runtime restart, but resuming future Turns requires the normal owner/writer/current-policy checks; do not fabricate that an old in-flight Turn survived if it did not.

## 19. Removed workflow surfaces

Remove from the canonical model/tool API:

    subagent
    team
    plan
    review_release

Rationale:
- the removed **team workflow tool** is parallel/serial composition of ordinary Agent Jobs; it does **not** remove teams as verified **membership/role/policy scopes** used in unified Policy resolution. Teams are not a replacement for the authenticated principal or an added `t:` reference (reserved for Turn);
- plan is ordinary Agent reasoning + Graph/Agent composition;
- review release belongs to policy/approval authority, not a general model primitive.

If product code still uses internal team/planner libraries for UI or experiments, they may remain non-tool implementation details only if independently needed. They must not create a competing Agent runtime or canonical tool surface.

Human/policy approval belongs in the existing policy/approval path and resumes/retries only through explicit factual authority; no model-facing release tool is required.

## 20. DL-150 Job simplification, hard pre-1.0 cutover and restart removal

The new vNext intentionally breaks the pre-1.0 implementation/API. Preserve DL-150's **correctness invariants**, not obsolete code paths, wire names, state formats or compatibility obligations. Initial Graphs accept permitted native/shell leaves and AW-owned `agent` leaves, but **never a nested `batch`** (model or typed SDK/host); Agents may create nested Agents under finite causal/child-depth limits. This is a clean hard cutover: one new catalog, one new parser/control plane, no dual registration, migration layer, compatibility decoder or legacy replay path. **Only verified isolated, explicitly disposable development/test state can be reset; a pre-1.0 label is not permission to discard live external obligations.**

Deliberate replacements:
- old job_start(ops) -> batch({ops,background:true}); same Graph constructor, same preflight/replay identity, immediate exposure of the same Job;
- `background:true` admission order is **complete Graph/Process preflight -> exact current authority -> reserve durable replay identity/Job handle and optional wake rule -> expose that same handle -> start roots**. An optimistic foreground constructor has the same semantic execution; if its response is lost, reattach using the original replay owner or expose the existing Job, never spawn a replacement merely to obtain a result;
- old shell-shaped job_start -> bash/pwsh/powershell(...,background:true); no second shell constructor;
- old job_output -> jobs.read;
- old job_list/get/ack/cancel -> jobs.list/get/ack/cancel;
- old Job IDs -> typed canonical/contextual refs;
- old separate Agent control/read APIs -> the same jobs.* control/data plane.

Remove rather than preserve:

    job_start
    job_list/job_get/job_output/job_ack/job_cancel/job_restart
    jobs.output
    jobs.restart
    agents.*
    grouped agents({op})
    grouped jobs({op})
    subagent mode aliases
    old argument aliases that exist only for compatibility

There is no compatibility deserializer, migration or automatic drain subsystem. Delete obsolete provider registrations, old `job_*`/grouped Agent/Job decoders, restart execution code, legacy-only state adapters and their compatibility-only tests in the same source cutover. **One-time reset gate:** the operator explicitly identifies the exact isolated runtime generation/profile as disposable, closes ingress, stops the old runtime and all supervised children, and verifies there are no active or ambiguously settled external effects, paid/provider attempts, connected clients, outstanding MCP Tasks, replay result claims or output/ACK leases owed to external owners. Durable Agent/Session records outside the designated transient namespace are never reset by implication. If any precondition is unknown, abort the reset in that environment; require explicit operator resolution or launch the new schema in a separately isolated deployment. This is a shutdown/quiescence **safety gate**, not a compatibility, migration or dual-schema service. No old request is reinterpreted or executed again under a new tool name.

This hard cutover does **not** weaken the new API's own replay/MCP Task/ACK correctness, protected-path/security invariants, or unrelated durable Agent/Session history. Those are validated under the new schemas from a clean current state.

job_restart is intentionally removed rather than renamed. A retained historical Job definition is audit/replay evidence, not executable authority. Re-execution is a fresh explicit bash/pwsh/powershell/batch/agent constructor call with current arguments, which:
- allocates a fresh execution identity;
- re-runs current schema/preflight/authority/interpreter/resource checks;
- may repeat side effects only because the caller explicitly requested a new execution;
- is independently replay-protected so duplicate delivery of that new constructor does not create another execution.

This is simpler and safer than treating an old retained Job as a capability to execute again. SDK/application code that wants "run this again" may retain the original typed arguments locally and submit them as a fresh constructor. The new API does not introduce a hidden `rerun`, `resume`, `retry` or executable saved-Graph handle; retained definitions are immutable history only. In model-efficiency tests, compare a deliberate long-Graph re-execution against old one-call restart and justify the new extra argument tokens as the cost of explicit current authority.

Do not confuse removed model-facing restart with:
- process restart detection;
- runtime startup reconciliation;
- cleanup recovery;
- replay tombstones;
- filesystem private-artifact reconciliation.

Those correctness mechanisms remain mandatory.

### DL-150 adoption/simplification ledger (normative for vNext)

| Original DL-150 contract | vNext disposition | Invariant retained |
| --- | --- | --- |
| 13 independent native leaf tools and simple schemas | **Keep unchanged** | exact mutation paths; bounded complete records; metadata opt-in; no success echoes |
| `batch(ops)` / `job_start(ops)` | **One `batch(ops, background?)`** | same immutable Graph compiler, implicit sequencing, `*` roots, `when` outcomes and preflight |
| Graph node IDs `1..N` | **`n:1..n:N` under Job ref** | same deterministic coordinate, no new opaque node map or discovery call |
| Six outcomes plus `more`/`incomplete` | **Keep unchanged** | pagination and outer response budget cannot change captured node outcome/branch |
| Job list/get/output/ack/cancel | **`jobs_list/get/read/ack/cancel`** | distinct status/data/consumption; raw offsets, multi-owner Task/replay leases |
| `job_start(command,...)` | **`bash/pwsh/powershell(background:true)`** | original interpreter/sandbox/timeout/hand-off/Process custody |
| `job_restart(id)` | **Remove** | historical Graph not executable authority; fresh explicit constructor, full current policy and replay |
| Shell-coupled Job capability | **Separate Control/Read observation** | read-only native Graph/Job control without implicit shell or file Write |
| Exact native operation correctness | **Both Local and DelegateAgent are valid execution strategies** | for delegated filesystem/Process/control/constructor operations the exact original prepared executor, authority, identity and factual result owner remain mandatory; no Agent text substituted for effect evidence |
| Captured Graph conflicts and completed dependencies | **Extend to Agent-owned effectful child barrier** | no complete node success or conflict release until still-owned effectful descendants are quiescent; no implicit detach |
| Interactive Process stdin | **Reauthorize each new input** | exact Control + current `process.input`/launcher/source/resource policy checked against actual retained sandbox |
| AW-bridged source notifications | **AW-owned reliable; remote dotlink passive initially** | never promise wake without source-fenced atomic registration/event commitment; passive get/read remain available |
| Native MCP Tasks and OpenAI redelivery | **Keep as transport projections** | same Job/result owner, no second execution, protected replay tombstones/horizons |
| Linux/Windows/macOS native safety and progress | **Keep unchanged** | rooted descriptor authority, sandbox/denies, cumulative fixed/unknown progress |
| Schema/result-token budget | **Measure at vNext cutover** | flat names, small schemas, compact results, no repeated args/rounds; attributable parent/router/delegate/transformer provider tokens, payer cost and latency |

This ledger supersedes **only** the old conceptual Job surface, restart and identity naming. All underlying filesystem, Graph, mutation, conflict, output, replay, native-platform and Task correctness from sealed DL-150 remains normative for the new implementation. Do not retain obsolete pre-vNext code merely to replay or migrate its old wire/state forms; port the invariant to the new canonical implementation and delete the superseded implementation.

## 21. Shared-layer placement

### Stable crate topology — thin local implementation now, reusable growth later

**Architectural decision (build skeleton now):** introduce **`aw-policy`, `aw-session` and `aw-host` as separate, acyclic library boundaries**. Neither an all-owning `aw-tools-adapter` nor three "manager" implementations is acceptable. Source ownership must match the lifetime and invariant being enforced; a crate name does **not** justify a second engine. Existing hosted Actor and Session persistence crates remain the source of truth.

| Crate | Owns (stable public/reusable responsibility) | Explicitly excludes |
| --- | --- | --- |
| **`aw-agent-kernel`** | Small portable Actor/Turn/Envelope/capability invariants plus tiny generic coordination value types such as `WriterFence = NotApplicable | Epoch(NonZeroRevision)`; existing no_std+alloc cut | Provider, Session store, policy configuration, filesystem, host |
| **`aw-tools`** | Prepared operations, native filesystem, immutable Graph/finite Job algorithms, owner/route/receipt/fence *primitives* | AW user/team/Session/member/payer/provider decisions |
| **`aw-policy` (new)** | **Pure policy composition:** typed trusted scope/provenance inputs, deny precedence, candidate-route intersections, bounded ceilings, explicit preferences/decision shapes; no_std-capable, no I/O | Actual source grants, issuer authentication, policy DB, provider/guardian calls, Actor, Session, fs |
| **`aw-agent-core`** | **The one hosted Agent/Actor/Turn engine**, existing ToolPolicy/ToolBindingAuthority, original per-effect grant enforcement | A second hosted Agent runner or team/session policy database |
| **`aw-store-api`** | Existing backend-neutral Session/branch/cursor/TurnJournal contracts and store capabilities | SQLite state, host Agent orchestration |
| **`aw-session` (new)** | Backend-neutral **actor-scoped Session access and coordinator-binding contracts**, over `aw-store-api`; eventual approved-context/participant admission service over authoritative backend hooks | An independent Session/Actor DB, membership authority, provider inference, tool adapters |
| **`aw-store` / `aw-store-jsonl`** | Existing SQLite/JSONL authoritative Session, history, writer/ACL/commit/replay persistence in their capabilities | A second SessionManager or Graph scheduler |
| **`aw-tools-adapter`** | AW native filesystem/Process/Graph/Job **executor adapters**, exact ToolSpec/projection and original result/custody bridge | Policy compiler, Session manager, participant ACL, Actor runner or aggregate host |
| **`aw-host` (new)** | **Thin application/SDK host composition:** authenticate context, use policy constraints, select execution strategy, wire original Agent, Session backend and tool adapters; one bootstrap entrypoint | Its own Actor/Session/Job engine, separate grants DB, second queue or hidden provider runner |
| **`aw-cli` / web/SDK frontends** | User-facing configuration, interaction, backend/feature selection, local/full startup | Duplicated policy, Session, Actor or tool business logic |
| **dotlink** | Host-independent use of `aw-tools` and its own concrete native Process/Graph/sandbox ownership | AW host/policy/session/provider dependencies |

**Dependency direction is a DAG:** `aw-host` depends on `aw-policy`, `aw-agent-core` and `aw-tools-adapter`, and **optionally** `aw-session`. `aw-session` depends on the already reusable `aw-store-api` contracts (which in turn reference Agent core), **not** on SQLite/JSONL implementations or `aw-tools-adapter`. `aw-tools-adapter` depends downward on `aw-tools` and existing Agent/tool contract types, never on `aw-host`. `aw-policy` is pure and does not import Session, Agent, tool executor, network or storage. `aw-cli` chooses the concrete store/providers and enters through `aw-host`; the store implementations never depend back on the host. Keep these edges acyclic and check with Cargo.

**No new `aw-actor` crate.** Hosted `aw-agent-core::runtime` and portable `aw-agent-kernel::actor` already own Actor/Turn state. If multiple callers need more coordination, expose host-verified hooks through the existing owner, not another ActorManager, child queue or persisted lifecycle ledger. Similarly, `aw-session` adds a distinct *service contract*, not a replacement `aw-store-api` or a second SessionStore, while `aw-policy` compiles constraints but never grants authority. Future additional backends/teams merely implement current store/host policy hooks.

**First real implementation (already scaffolded, intentionally thin):**
- `aw-policy` is `no_std`, allocation-free for simple policy slices, and resolves deterministic Local routing, explicit denies, required-route precedence, same-scope preference ambiguity, route intersection and tightest model-round ceilings. **`EffectivePolicy::narrow` cannot retarget or widen a previously selected route/limit.** Every revision remains host-validated evidence, not a trusted membership/issuer grant; future domain/team resolution stays above the pure crate.
- `aw-session` supplies a generic `Sessions<'_, B: SessionBackend + ?Sized>` wrapper, statically dispatched for a concrete backend and dynamically dispatched only if chosen, plus evidence-only `CoordinatorBinding` comparing exact owner, Session/branch, Agent ref, binding revision and two portable `WriterFence` values. The current skeleton's optional integer representation is temporary: local single-writer backends explicitly use `NotApplicable`; durable/multi-writer sites require real `Epoch` fences at the authoritative store. Once shared `ResolvedExecutionRef` exists, replace raw Agent-ref input with the typed form rather than another parser. Durable binding uniqueness/sharing/audience remain SessionStore work.
- `aw-host::LocalHost` now owns **the one stock-AW local execution-site/scheduler selection and bounded model rounds**, then delegates to the original `aw-tools-adapter::configure`, which independently narrows/enforces its exact native resource ceilings. Root `src/lib.rs` no longer has a duplicate local `execution_policy` function. Pre-granted embedding host tool scheduler limits may **only narrow**, and an already granted execution Site cannot be changed. Reapplying an additional scoped policy to the same LocalHost cannot restore a previous ceiling. `DelegateAgent` remains explicitly denied until real Agent/payer/executor admission exists; never report fake success or implicit fallback.
- The root AW library has a small composition seam through `aw-host::configure_local` for its existing local Agent construction. This path does not open a Session DB, create a team/actor service, perform a policy network call, invoke a guardian or add a model Step. The real local runtime still runs through the original Agent/adapter/ToolPolicy.
- `aw-host`'s `sessions` feature imports `aw-session` only for hosts that need backend Session access. The minimal/userless path works without it, with the existing host-authenticated local principal serving as an exact grant identity—not a fake multi-user login.

**Future growth stays within this topology:** first migrate additional root CLI composition into `aw-host`, then add real AW Session coordinator/writer/audience hooks through `aw-session` and `aw-store-api`, then add host-resolved domain/team/workspace policies via `aw-policy` decisions validated by the existing ToolPolicy/Envelope. Optional Graph-wide guardian, Node-level routing and `DelegateAgent(Graph|Node)` plug into `aw-host` using the original Agent/Graph engines. Move code only when its owning invariant is actually implemented; do not bulk-rewrite existing storage, create a general plugin framework, or keep two paths indefinitely.

**Scale and maintenance gate:** each new manager-like API must either own an independent invariant or bridge a genuinely different backend. Reexports/pure forwarding with no semantics belong in a module, not another crate. The three crates above have concrete ownership and separately testable contracts. Keep the local hot path allocation/IO/paid-inference cost unchanged, and benchmark against current minimal mode before widening any defaults.

### aw-tools

Own reusable, host-neutral:
- canonical fs operation IDs/schemas/requests;
- host-neutral prepared-operation/effective-policy constraint/decision-result types, routing/executor hooks and, where real consumers need them, opaque request-origin/coordinator-scope/receipt/replay-cancel contracts;
- small reusable bounded coordination primitives actually needed by tool/Graph owners: borrowed dependency-closed `GraphActivationContext`, opaque host-issued admission/owner-generation tokens and wait/control ancestry callbacks; **do not import/re-export `aw-agent-kernel::WriterFence` or any AW Agent crate from `aw-tools`**. The portable typed `WriterFence` remains owned by `aw-agent-kernel` and is checked by the AW Session/Actor owner before the callback authorizes a shared Graph/Job transition. Opaque shared evidence never becomes an executable capability or a duplicate writer-fence enum; **no domain/team/workspace membership, Session ACL/history or issuer authority engine**;
- Graph grammar/planning/status/footprints;
- finite Graph Job state/result retention where already shared;
- common hierarchical ExecutionRef/ResolvedExecutionRef, Revision, Job kind/read/control request/result types where genuinely generic;
- routing traits/interfaces for heterogeneous Job owners;
- **one bounded exposure-order value/owner-callback contract** shared by in-memory live-generation and SessionStore-backed indexes; no compulsory DB, new manager, or per-filesystem-call indexing;
- **source-owned event/revision + idempotent recipient-admission callback primitives** reusable by Process/Graph/Agent owners; source and recipient authority stay in their actual hosts, with no global event bus;
- typed Text/ExactRead/Bytes/Media representation;
- bounded cursor/result helpers.

Do not add AW Session/provider/Actor/kernel dependencies. **Do expose generic, host-neutral prepared-operation execution callbacks, candidate route envelopes, Graph-coordinator execution scopes, per-node activation hooks, opaque owner-generation checks delegated to the actual host, and conservative route-footprint/receipt correlation primitives**, enabling any operation/Graph node to use either concrete execution path or accept work from multiple authenticated request origins without hard-coded type exclusions. Shared input provenance is opaque validated host evidence, not an executable grant or a new queue. AW's concrete `Local | DelegateAgent` policy, guardians, provider/model invocation, delegate identity and payer authority live in the host/adapter, not the reusable primitives. Implement d/i/p/a defaults, coordination-parent `^`, current Agent, cross-principal authorization and alias-free routing only in AW. Prefer small tagged owner dispatch over an extensible generic Job-kind/plugin framework. No second Graph scheduler.

### aw-tools-adapter

Own AW host projection/integration:
- ToolSpec flat provider projection;
- PreparedOperation mapping and bounded origin/coordinator/branch-fence metadata forwarding for **the same original execution**, not a separate collaboration input pipeline;
- host execution-dispatch adapters for **all prepared operations** to Local or AW's DelegateAgent coordinator while reusing original exact native/Process/Job/constructor executors and factual receipts;
- generic bounded coordination callbacks for one Graph-scoped Agent/Turn or runtime per-node Agent/Turn, without copying Graph execution;
- Local Process/native adapters, and Graph integration that captures **union of all admitted route footprints before conflict planning**, freezing each dynamic per-node route only at activation;
- unified Job registry/router glue;
- SDK adapter bindings where appropriate.

It must not own provider/model policy, participant membership, Session history, collaboration audience, a second durable inbox or a second Agent runner.

### AW Agent/runtime/store

Own:
- one scoped Policy configuration compiler and generation-fenced effective decision pipeline for host/domain/identity/team membership/workspace/Session/Graph/Node; trusted issuer authorization and source/target-domain/workspace owner resolution;
- principal/project/Agent namespace and current-Agent relative-ref resolution;
- stable Agent a:<handle> identity, reserved a:0 root/default handle, immutable origin and mutable authorized parent/coordinator relation;
- Agent Job exact Session/branch binding and **one primary-project-root `a:0` per d/i/p**, lazily bound to one executable Session/branch; new Agent identities only for independently executing Session branches;
- Session owner/member and verified team audience, independent View/Contribute/Submit/Control/Manage capability checks and vetted shared history projection;
- multi-user participant input provenance, current membership/audience epoch, **one existing Agent Input queue and Turn/branch writer**, and original payer/initiator authority at every effect;
- Session share/revoke and explicit curated-context Session creation or team ownership transfer, with original canonical identities never silently renamed;
- Turn generation/writer;
- Step/model invocation;
- ToolCall journal;
- one **scoped Policy** configuration across domain/identity/team/workspace/Session/Graph/Node, compiling exact ToolPolicy/Envelope authority, guardian Allow/Deny/Ask/Narrow, Local or DelegateAgent with scope Graph (delegated default) or Node, site/provider/payer limits and current disclosure; **one policy compiler and one execution pipeline**, not parallel engines;
- host PolicyDecision guardian (`Allow/Deny/Ask/Narrow`) and ExecutionPolicy **fields inside the same unified Policy**, including one Graph-wide guardian, dynamic Node routing, provider/model/site/payer selection and exact current grants; one Graph Agent/Turn coordinator or per-activated-node Agent/Turn selection, independent inference/payer/tool/resource authorization and causal-work reservation;
- delegated child Agent/Turn ownership, parent wait/resume/cancellation and routing through original prepared semantic executor/result owner;
- child Job/Agent ownership relations;
- jobs.send Agent input/steering;
- jobs.cancel Turn/Agent;
- Agent history JobReadPage projection;
- current per-participant shared-session audience and context disclosure/redaction, no combined participant privileges or unverified private-history branch fork;
- wake recipient admission and cross-host Workspace writer epochs that forbid a stale follower from admitting another shared Session Turn.

A routing/guardian model may advise these decisions through ordinary separately authorized AW inference, but never owns execution authority.

### dotlink

Own only product-specific:
- sandbox/process launcher custody;
- OAuth/ngrok/transport;
- MCP/OpenAI replay;
- MCP Tasks;
- wall-clock finite Job retention;
- standalone Process/Graph Job exposure.

Consume shared types through exact reviewed AW Git revision.

Standalone dotlink does not create Agent Jobs and does not implement AW ExecutionStrategy/OperationRouter delegation. It executes its standalone Process/Graph semantics locally. With an authenticated AW bridge, AW remains the routing/delegation authority; the same wire jobs_* calls resolve/canonicalize Agent refs and route them back to AW ownership. Dotlink-owned live Process/Graph sources are **passive get/read/control only for initial remote event delivery**: AW rejects their constructor wakes and `jobs.subscribe` unless a separately reviewed source-fenced atomic registration/event bridge has actually been implemented. Authentication of the receiving AW Agent alone does not confer reliable remote wake capability.

## 22. Performance and implementation discipline

Prefer:
- static enums and compact tagged request types;
- zero-copy/borrowed views where practical;
- no serialization round-trip between internal layers;
- bounded BTree/Vec structures where deterministic ordering matters;
- direct trait dispatch or enums over dynamic scripting in hot paths;
- lazy rendering only at transport boundary;
- raw bytes retained once with multiple owner leases rather than copies.

Do not sacrifice clarity for speculative micro-optimization.

The default model path should avoid:
- repeated result echoes;
- duplicate Agent result read after synchronous completion;
- polling when wait_ms can block cheaply;
- full transcript reads when result is enough;
- exposing hidden child Job refs that the model does not need.

## 23. Model-first efficiency acceptance

Before final advertised cutover measure actual generated schemas and workflows.

Measure:
1. total serialized provider schema bytes/token estimate for the effective-grant catalog;
2. first valid tool-selection/call rate;
3. parent model/tool rounds, result tokens and recovery/cancellation correctness;
4. **total attributable provider input/output/cached tokens, payer cost and wall-clock latency** across parent + authorized router/guardian + delegate Agent/nested tool calls + optional result transformer, not merely what the parent sees;
5. **every canonical operation class** under Local vs authorized DelegateAgent equivalence of factual output/effect/replay, including 13 FS tools, shell/Process, `batch`, `agent` constructors and `jobs.*`; default Local must involve zero router inference;
6. for Graphs, **Local vs one DelegateAgent(Graph) Agent/Turn vs DelegateAgent(Node) dynamic activation-time routing**, independently with/without **one-shot whole-Graph guardian**; compare one Graph coordinator with per-node Agent count, skipped branches, `when`/conflict/result correctness, real model Steps, latency, tokens and preparation budgets;
7. closed helper Agent Session/Turn retention size and cleanup over repeated transparent delegations, without losing parent replay/receipt evidence;
8. effective **policy resolution/guardian cost** on the Local hot path versus Graph/Node advanced policies: scope-merge/cache cost, one-shot guardian token/latency, number of policy lookups, denied/Ask/Narrow retries avoided, and truthful re-evaluation after domain/team/workspace/Session revocation. Do not add policy configuration or reasoning tokens to the provider tool schemas.

Representative workflows:
- direct read;
- fs grep/edit;
- 3-node read-only Graph;
- Bash synchronous completion;
- Bash handoff -> jobs.get(wait_ms) -> jobs.read(stdout) -> jobs.ack using t:<h>/j:<h>;
- current Agent continuation with omitted ref;
- parent communication through "^";
- same-namespace peer Agent communication through a:<handle>;
- structural overlay to another project/principal/domain, including descendant :0 reset;
- direct use of a fully resolved canonical /d:<h>/i:<h>/p:<h>/a:<h> ref;
- contextless resolution from d:0/i:0/p:0/a:0 base;
- Agent synchronous Turn result;
- Agent background initial Turn -> jobs.get -> jobs.read default result;
- Agent root jobs.send auto/queue with no rev;
- same send with current rev;
- stale Turn rev refusal;
- active Turn auto->steer with Step fenced by Turn rev;
- active Turn explicit interrupt;
- exact Turn cancellation while child Process is running;
- Graph interactive n:<handle> ref -> Process stdin using auto;
- Agent waiting child then Turn cancellation;
- Agent jobs.read messages from root history cursor;
- Agent jobs.read exact t:<handle> filtered tool_calls/tool_outputs;
- stable a:<handle> creation from another Agent with origin + parent metadata;
- reparented Agent keeps canonical identity while "^" resolves new coordinator;
- Graph Agent node plus native filesystem conflict;
- transparent DelegateAgent execution: parent model proposes one action, a differently configured Agent/Turn executes it while the parent model remains idle, and the original ToolCall receives the factual semantic result;
- delegated ToolCall replay reattaches the same child Agent/Turn rather than rerouting;
- Graph leaf delegated during preflight preserves original coordinate/when/outcome/ceilings while delegated footprint affects conflicts;
- **one-shot whole-Graph guardian/router** for up to 64 original nodes: one persisted, replay-claimed decision attempt returns distinct hints per coordinate for the admitted Graph-eligible native/Process/Agent leaves (SDK-only `jobs.*` leaves only after their owner/control contract is explicitly enabled). Nested `batch` is rejected at any depth. No operation class is intrinsically Local-only **once separately admitted**. Test **Graph** one coordinating Agent/Turn and **Node** dynamic Local/Agent selection for runnable nodes. Every native effect uses the original executor/receipt and the same Graph Job manager; no duplicate execution or extra public tool;
- required guardian denial/ask/invalid/missing coordinate, untrusted attempted grant, stale host registry/policy or invalid/narrowed args fails Graph admission before any root; matching replay reuses the same one-shot decision with no second paid invocation;
- result/tool effects and captured native Graph conflict/ceiling/`when` semantics remain unchanged after routing, even when most conditional nodes skip;
- lost first Agent creation response recovery.

Any material schema/token regression must be justified by measured reliability/capability rather than assumed from tool count.

## 24. Correctness acceptance matrix

### Typed handles, contextual resolution and revisions
- segment types are d/i/p/a/t/j/n; all have immutable canonical identity. Graph n is **exactly its 1-based decimal Graph coordinate** (n:0 and leading zeros invalid), not another owner-allocated opaque handle;
- allocator format is implementation-defined, but canonical spelling is one 1..64-byte delimiter-safe ASCII token with no normalization/escape aliases;
- d:0/i:0/p:0/a:0 are fixed provisioned canonical default objects, never caller-relative aliases; /d:<h>/i:0 has one identity for every caller, while contextual inheritance represents the current principal; t/j value 0 is ordinary, n:0 invalid;
- slash-prefixed canonical refs are fully resolved and include explicit d/i/p/a, including :0 defaults;
- partial slash refs such as /a:0 or /i:pvl are invalid;
- new contextual refs resolve once under current authenticated context before effects; matching replay lookup happens **first** and uses its original requested/canonical target, even after reparent, actor change or default availability changes;
- "." resolves self and "^" resolves current coordination parent;
- contextual structural overlays inherit ancestors above the first explicit level and reset omitted descendants to :0;
- without a current Agent, d:0/i:0/p:0/a:0 is a **parsing base only** over those fixed objects; missing or unauthorized defaults fail closed, no authentication-dependent remapping occurs, and no ref lookup creates principals/projects/Agents;
- structural segment order is monotonic d->i->p->a; duplicates/backtracking, "..", wildcard/range are invalid;
- execution suffix transitions are owner-typed: t only from Agent, n only from Graph Job, j only from an authoritative child-Job owner; invalid transitions fail before dispatch;
- stable Agent a:<handle> identity is separate from immutable origin and mutable coordination parent;
- a:0 is the root/default Agent in every d/i/p namespace;
- every allocated d/i/p/a/t/j handle is permanently non-reusable within its containing namespace; expired refs remain gone forever. Graph n coordinates are reusable only under a different Graph Job parent because that is a distinct namespace;
- effectful controls work without rev; supplied rev is exact compare-and-act;
- jobs.read/ack expose no rev and preserve stable paging/release through exact ref + cursor/offset/watermark; jobs.get rev is a conditional observation/change token and never fails merely because state advanced;
- revisions advance only on externally meaningful state;
- Agent Turns t:<handle> remain durable generations with one writer;
- Step is metadata only, never a public s:<...> resource;
- Agent root auto->queue and active Turn t:<handle> auto->steer;
- stale Turn/ref/send/cancel never affects a later Turn;
- clean cancelled Turn can return Agent Job to idle;
- uncertain Turn blocks automatic later Turn.

### Unified scoped policy/decisions/strategy
- one host-side Policy shape resolves constraints/denies, guarded decisions, execution/provider/site/payer preferences, limits and disclosure for host/domain/identity/verified team/workspace/Session/Agent/Graph/Node. Positive authority is separate authenticated `AuthenticatedGrantPath` evidence from existing ToolPolicy/ToolBindingAuthority/resource stores; pure `aw-policy` never issues/infers grants and no preference layer can be constructed as grant authority;
- exact actor principal is distinct from canonical resource owner, active team is verified membership (not a Turn or a free grant), and cross-domain work satisfies both source and target authority; `i:0` is never remapped to the authenticated caller;
- a direct/identity or verified team role grant path is required for the exact source/effect/site/resource, and all applicable host/domain/workspace restrictions and explicit denies apply. No multi-team path laundering, denied resource via delegate, missing grants from guardian `Allow` or permission widening via Session/Node preferences;
- limits and allowed routes/providers/sites are bounded by the tightest applicable mandatory ceiling; non-authoritative preferences resolve **Node/ToolCall > Graph > Session > Workspace > active Team > Identity > Domain > Host**, with no accidental preference from unrelated teams;
- original ToolPolicy and Envelope remain exact/narrow-only enforcers; ToolRevisionPolicy proves admission-time semantic argument `Narrow` before direct execution/Graph seal. Activation-time Node decisions cannot mutate prepared args/OperationId/dependencies/when/topology/semantic ceilings and may only Allow/Deny/Ask or narrow route/resource/provider/payer/limits. Direct/Node Ask uses one replay-stable PendingApproval. A coordinate-keyed whole-Graph guardian may produce several Ask entries; the existing Graph Job owns one bounded canonical PendingApprovalSet with unique deterministic per-target approval identities. Graph roots remain zero until every required Ask is Approved; any required Denied/Expired/stale-scope/current-authority failure settles admission without effects, and matching replay returns the same set/state without rerunning guardian;
- optional guardian **Graph or Node** scope is independent of DelegateAgent **Graph or Node** scope; default Local uses zero model guardian/route calls, and Graph preflight retains union route/footprint bounds for dynamic Node routing;
- one coherent effective-policy epoch/vector fences host/domain/team/identity/workspace/session/registry/source/owner; revocation rechecks every new effect/ToolCall/Turn, Agent/wake and current data read, without broadening already admitted work. Fail closed on unknown remote owner/membership revision;
- user-selected provider/model/execution site and payer are independently authorized, never implied by delegate strategy. One causal payer budget is reserved/debited once and cannot reset at team/Session/Graph boundaries;
- replay lookup is by original authenticated invocation identity **before contextual-ref re-resolution**; matching `^`/`.`/overlay request reuses the stored exact target even after parent or namespace changes. Changed original wire request under the same identity rejects; old ambiguous/retired identity cannot silently become fresh work;
- completed source/ToolCall replay returns factual previous result under current disclosure, never reruns guardian/inference, retargets policy, grants or repeats external effects. Policy provenance is auditable but compact result schemas remain unchanged.

### Authority/effects
- Control is a first-class exhaustive portable effect across kernel/core/shared-tool/adapter/policy/Envelope/journal/store projections;
- Control never inherits filesystem Read/Write authority and never joins parallel Read waves;
- default Off/ReadOnly remains fail-closed except for explicitly granted Control operations;
- Graph outer Control admission never substitutes for exact leaf Read/Write/Process authorization.

### ExecutionStrategy / transparent delegation
- ExecutionStrategy is an AW-host policy selecting Local or DelegateAgent **for every admitted operation**; shared aw-tools provides host-neutral route/Graph execution primitives, without AW Agent/provider dependencies or operation-class bans; no new model schema, OperationId, ToolEffect or Job kind;
- original ToolCall identity, canonical operation, admitted args, semantic ToolEffect, semantic ceilings and factual result/effect contract remain unchanged by strategy;
- guardian/routing inference can recommend a route but cannot grant route/inference/payer/tool/resource authority. AW defaults Local with no paid invocation. Dynamic Node guardian/router input is the canonical dependency-closed GraphActivationContext only; unrelated parallel node completions cannot affect the decision. An opt-in guardian can examine the whole Graph once, separate from one DelegateAgent Graph coordinator Turn or runtime Node routing;
- DelegateAgent requires independent exact Agent/provider/account/model/payer/causal-work/tool/resource authority; a semantic Read grant cannot implicitly authorize paid inference;
- additional execution mechanisms may consume separately authorized inference/private-scratch/process resources, but delegated user-visible/persistent semantic side effects stay within the trusted operation implementation contract; semantic Read never becomes filesystem Write merely because it is delegated;
- Graph strategy *possibilities* and conservative union footprint are captured before scheduling; **Graph** coordinator choice is frozen at Graph admission, **Node** actual Local/Agent choice freezes at eligible node activation under current grants/replay and never widens admitted ceilings. Optional whole-Graph guardian is one bounded/journaled auxiliary invocation with a coordinate-keyed map; duplicate invocation/dispatch reuses factual decisions. No fallback or duplicate effects after an ambiguous started node;
- scheduler uses a separate captured execution footprint; semantic Read through opaque delegation does not join a Read wave;
- parent model remains idle while delegated Turn executes; child may use a different authorized model/provider/tool set without mutating parent Agent/Turn/model identity;
- execution authority and delegate disclosure are independent. Delegate/provider receives only a bounded disclosure-authorized projection; exact secret-bearing args/commands/results, private transcript/provider context and credentials remain sealed in the trusted executor unless explicitly permitted. A delegate may request execution of an opaque prepared operation without seeing its payload;
- delegate returns the original **prepared semantic executor's** typed factual result/owner receipt for native FS, shell/Process, Job controls, batch and Agent constructors; free-form Agent text never fabricates effect success. Graph coordinator submits only admitted prepared nodes to the original shared Graph manager; visible-result transformation never changes factual evidence;
- completion-only is the transparent default; a delegate cannot block waiting on its synchronously blocked parent model without an independent controller/explicit Agent-Job handoff path;
- parent cancellation propagates only while exact delegated ownership remains; committed/unknown child effects stay factual;
- DelegateAgent consumes the same finite authenticated causal-work/payer allowance as explicit child Agent/Graph work;
- every semantic operation may use Local or DelegateAgent through the original exact prepared native/Process/Job/constructor executor and one replay identity, without hard-coded class restrictions;
- Graph uses one coordinating Agent/Turn rather than a second Graph runner, and Node is an activation-time Local/Agent selector with no per-node inference for skipped branches;
- provider/model-facing `batch` admits reviewed DL-150 native/shell leaves **plus `agent` on AW** with exact Agent/Turn/owned-child settlement; standalone dotlink keeps native/shell only. Neither direct `batch` Graph leaves **nor Agent-mediated `batch` during active Graph ownership** are permitted in initial vNext. Explicit `jobs.*` Graph controls remain qualified typed SDK/host-only, with exact owner/ref, authorization and mechanical wait/control-cycle checks. `agent` children may themselves create Agents under one finite inherited depth/cost/causal budget; transparent DelegateAgent remains ineligible for self/ancestor controls or any control whose completion depends on its waiting parent;

### Shared Session and layered coordination

- Sharing a personal Session **does not** change its d/i/p owner, branch, bound Agent ref, Writer/Session/Turn identity, execution source or payer and never creates a synthetic team user, team master or second Agent loop.
- One executable Session/branch has one Agent coordinator binding and one Turn writer. Agent↔Session/branch binding is immutable for the Agent identity lifetime: `a:0` may bind its first primary executable Session/branch once and is never sequentially rebound after retirement; later executable Sessions/branches get fresh Agent refs. Writer fences use portable `aw-agent-kernel::WriterFence = NotApplicable | Epoch(nonzero)`; durable/multi-writer backends require real Epoch fences.
- Existing member/read-append access, sealed draft Submit and shared CRDT audience remain distinct from View/Contribute/Submit/Control/Manage predicates. A viewer cannot trigger inference; an input submitter cannot steer/cancel the other participant's Turn without separate current Control.
- Multiple users submit through one durable existing Agent Input queue. Every authenticated Input/control boundary establishes one immutable AuthoritySegment in the existing TurnJournal; every model Step references exactly one segment plus current narrowed StepAuthorityContext. Tool-result/model continuation Steps inherit the current segment until another authenticated boundary replaces it or Turn ends. Cross-participant steer/interrupt starts a new controller segment and never later reverts to the prior actor. If actor/disclosure changes, provider input is rebuilt from authorized public projection and previous segment's provider-private continuation/cache/state is not reused; if safe projection is impossible, control rejects and requires queue/new Turn or approved-context Session. Every paid/provider attempt is replay-bound to the exact authority segment/context fingerprint.
- A **full-history share** requires current disclosure of all relevant Session/context dependencies; an **approved-context share** uses a separately authorized sanitized Session/Agent binding. A mere fork of a private Session branch cannot be claimed to hide private Session history.
- Every submitted ToolCall requires the initiating actor's currently valid grant path **intersected** with Session participation, coordinator, workspace/domain/source/site/provider/payer policy; no union of Alice/Bob permissions and no automatic use of owner/private credentials. Unattended work requires an explicitly authorized real resource/service principal and payer, not a fake human.
- A membership revocation changes the audience generation and invalidates new Inputs/ToolCalls/read pages and sharing notifications, while preserving accepted/auditable receipts and truthful unknown effects. Shared-draft audience changes rotate its epoch rather than retaining old disclosure.
- One current **Workspace writer epoch** and exact Agent writer protect Session/branch admission across nodes; stale owner/follower cannot start a duplicate Agent Turn or overwrite the branch. Explicit owner/coordinator replacement is separately authorized and fenced.
- Personal-to-team ownership change requires an authorized curated transfer/import into a distinct target d/i/p project/root; sharing a personal Session does not rename immutable original refs or reparent Agents without consent.
- **Shared aw-tools core** only provides host-neutral opaque origin/owner/writer-generation/receipt and Job/Graph callback extension points, reused by private and multi-user callers; no membership service, SessionStore, second queue, second Agent runner, fake team principal, persistent collaboration database or new public tools.
- **aw-tools-adapter** bridges those exact callbacks and original native/Process/Job receipts to one AW host owner; **AW SessionStore/Policy/Actor** holds real membership, branches, coordinator bindings, queue, context/disclosure and payer rights; **dotlink** owns concrete remote sandbox/execution only.
- Single-user fast path allocates no sharing-specific Agent, queue, team lookup or extra provider/guardian invocation; sharing adds checks only at the relevant Session/Turn/input boundary.

### Steps/tool calls
- complete tool batch receipt belongs to Step;
- every Step references exactly one current AuthoritySegment; tool-result/model continuation Steps inherit that segment until another authenticated Input/control boundary replaces it or Turn ends;
- every paid/provider Step has one immutable ModelInvocationAdmission binding Agent/Turn/Step + authority_segment_id + authority_context_fingerprint + existing provider/account InvocationIdentity + host invocation ID; replay under a different segment/fingerprint is a new admission and never reuses the old paid attempt/private continuation;
- started external effects retain factual outcome;
- child Job correlation exact;
- no Job explosion for atomic fs calls.

### Waits
- waiting consumes no model invocation;
- cancellation remains serviceable;
- no held global runtime lock;
- condition completion resumes only if owning Turn is still current;
- Policy Ask reuses the same owner wait machinery/replay identity. Direct/Node Ask uses one immutable PendingApproval. Whole-Graph admission may hold one bounded canonical PendingApprovalSet with independent idempotent coordinate approvals; roots remain zero until every required entry is Approved, while any required Denied/Expired/stale-current-authority entry settles admission without effects. Matching replay returns the same set/current states. If the only approver/controller would itself be synchronously blocked, proactively hand off/expose the same owner or reject foreground rather than deadlock.

### Events
- advertised event enum is result|settled|attention and resolved kind validates support; AW Graph exposes attention iff admitted topology contains an attention-capable Agent node. Completion-only is the default; parent-blocking attention exists only when immutable captured host delegation explicitly permits that request/response path;
- AW-owned source Jobs with source-serialized event commits support constructor `wake` and `jobs.subscribe` through the **same** source-owner registration/event-revision boundary; constructor wake registration is atomic with Job admission before first effect, and non-capable owners reject before effect/registration;
- subscription rules persist until replace/clear/source terminality/authority revocation and are non-retroactive; EventRule is a unique semantic set, duplicate event names reject and replay hashing uses canonical enum order result,settled,attention;
- subscription delivery uses only auto/queue/steer/interrupt;
- source-owner captured subscription generation/mode + exact source-ref/event/event-rev/recipient dedupe prevents duplicate wake Inputs; receiver owns current authorization and durable Input admission only. No parallel event bus/subscription registry; bounded causal lineage/recipient queues/automatic paid-Turn budgets prevent **new-event cross-Agent cycles**;
- finite result+settled duplicate wake coalescing, with conflicting coalescible modes rejected during subscription validation;
- Agent per-Turn result does not settle whole Agent Job;
- steer/interrupt wake binds receiver's exact current Turn ref/rev at delivery;
- queued wake acceptance never substitutes for current inference/payer/tool authority at Turn claim;
- no stdout/tool text forges attention; an AW-hosted Graph may project authenticated Agent-child attention with its n:<coordinate> and authorized child/Turn reference; standalone dotlink never fabricates it.

### Read
- jobs.list scope is exact canonical d/i/p or standalone live runtime; cursor binds authority, exposure position, projection version and runtime generation for ephemeral owners. One host-neutral exposure-order primitive supports durable mixed Agent/finite SessionStore indexing **or** sessionless in-memory Local finite Jobs, with no mandatory DB and no ID reuse across restarts;
- one host-neutral bounded exposure-order contract has **two backends**: existing project/SessionStore for durable hosts, fresh in-memory runtime-generation owner for sessionless Local. Both list finite Jobs without copying lifecycle state or creating a new manager. Finite slots are reserved before effects; publication/recovery is idempotent; sessionless cursors cannot cross restart and `j:` identities never collide across generations;
- get is status only and its rev/wait_ms conditional observation is lost-wakeup safe;
- read is data only and never implicitly ACKs;
- ReadView is a small typed enum and resolved kind rejects invalid combinations;
- ACK only for ackable byte retention and requires explicit ref;
- Agent durable history never consumed by ACK;
- record/history cursor may continue at a record boundary or explicit intra-record payload fragment, bound to canonical ref/view/tools/projection format and disclosure epoch; changed policy/replay/tampering rejects rather than combining pages;
- model continuation spelling is exactly more cursor=... and is directly copyable into jobs.read(cursor);
- typed SDK result and model renderer derive from same JobReadPage;
- current per-category/resource disclosure including composed trace, denied historical outputs/secrets and no hidden reasoning/system/provider context leakage;
- pagination advances and never lies about completeness.

### Graph
- Agent footprint conservatively covers complete delegated authority (native FS, Process reach, external/network/extension effects, child delegation); Graph-node execution scope propagates to every initial Turn/transitive child and queued autonomous continuation. Its `batch` authority is blocked while it is owned by an active Graph, even through an Agent-mediated direct call;
- Graph preflight is root-free: every leaf passes semantic/schema/eligibility (`agent` allowed on AW, nested `batch` rejected), ToolPolicy/topology/when/ceiling checks. **Claim the original replay identity and bounded guardian/model budget before any paid guardian inference**, then revalidate all admission-time argument narrowing, capture Local/DelegateAgent route unions/conflict footprints, seal and run roots. Ambiguous paid decisions never silently retry. Node activation receives only deterministic GraphActivationContext from dependency closure including implicit previous-success sequencing and cannot revise prepared args. Before activation-time guardian/router/Ask, scheduler claims the node's deterministic union-footprint conflict position; conflicting successors cannot overtake while decision waits, though non-conflicting work may continue. DelegateAgent(Graph) uses one coordinator Turn; skipped nodes pay no route inference. Guardian routing cost is preparation evidence, not leaf effect/result;
- DelegateAgent keeps original coordinate/semantic operation/outcome/when/more/incomplete/ceilings while scheduling uses its captured delegated footprint;
- opaque Process/network/provider/external effects **conflict with interacting Graph nodes** by default; conflict plan uses the conservative union of all admissible routes, and later per-node selection never widens it. Explicit Graph dependencies give additional ordering but no outside transaction guarantees;
- the same captured attention-capable Agent predicate drives proactive handoff: if such a child would otherwise depend on the synchronously blocked sole controller, the same Graph Job is handed off before roots run;
- all leaves authorized before roots;
- outer presentation cannot change node status/more/incomplete/when;
- long-lived Agent identity alone never keeps its Graph node nonterminal. **All transitive running/queued/unknown work under the Graph-node causal ownership scope must settle/quiesce before complete success or conflict release.** No implicit detach; after release, subsequent child-Agent Turns require fresh independently authorized origin, causal/payer budget and conflict admission and cannot revive Graph authority;

### Removal
- no old job_* advertised;
- job_start is replaced by background=true on the original typed constructor;
- job_restart/jobs.restart are removed: a fresh constructor is a new replay-protected execution under current authority;
- retained historical Job definitions are never executable capabilities;
- no jobs.output/restart;
- no agents API;
- no team/plan/review_release/subagent model tools;
- no grouped op model tools;
- no compatibility aliases, old decoders, migration, drain or dual-state path; obsolete pre-vNext implementation code/tests are deleted in one hard source cutover and incompatible transient development Job/replay/Task state may be reset.

## 25. Final decision record

The proposed vNext contract is:

    Model:
      13 flat filesystem tools
      bash / pwsh / powershell / batch / agent
      jobs_list / jobs_get / jobs_read / jobs_ack /
      jobs_send / jobs_cancel / jobs_subscribe

    SDK:
      tools.fs.*
      tools.bash / tools.pwsh / tools.powershell
      tools.batch / tools.agent
      tools.jobs.*

    Canonical operations:
      fs.*
      bash / pwsh / powershell / batch / agent
      jobs.*

    Model Graph:
      [flat_tool_name,args,when?]
      same leaf spelling as direct model tools; resolve below wire to semantic OperationId

    Typed canonical namespace:
      /d:<h>/i:<h>/p:<h>/a:<h>[/typed execution suffix...]
      slash-prefixed refs are fully resolved canonical identities
      each <h> is an owner-chosen immutable canonical handle
      allocator format is unconstrained for d/i/p/a/t/j
      Graph n:<coordinate> uses canonical positive decimal 1-based index
      canonical spelling is one bounded delimiter-safe ASCII token
      execution transitions are owner-typed (Agent->Turn, owner->child Job, Graph Job->node)

    Reserved structural defaults:
      d:0 = fixed canonical local/default domain
      i:0 = fixed canonical default principal within d
      p:0 = fixed canonical default virtual project within d/i
      a:0 = fixed canonical root/default Agent within d/i/p
      current principal comes from contextual inheritance, never by remapping i:0

    Contextual refs:
      omitted or "." = current Agent
      "^"            = current coordination parent Agent
      a:<h>          = Agent overlay in current d/i/p context
      p:<h>/a:<h>    = inherit d/i, replace p, reset omitted descendants to :0
      i:<h>/p:<h>    = inherit d, replace i, reset omitted descendants to :0
      d:<h>/i:<h>    = replace d, reset omitted descendants to :0
      t:<h>/j:<h>    = execution beneath resolved/current Agent
      no "..", wildcard/range, alias layer, or partial slash-canonical forms

    Overlay algorithm:
      inherit structural ancestors above first explicit d/i/p/a segment
      apply explicit structural segments in d->i->p->a order
      reset omitted structural descendants to :0
      without current Agent use d:0/i:0/p:0/a:0 **only as parser base**;
      defaults must exist under current authentication; never implicitly create authority

    Job:
      universal controllable execution resource
      public lifecycle = running / waiting / idle / blocked /
                         cancelling / cancelled / closing / closed / settled
      DL-150 outcomes remain separate and are the only Graph when predicates

    Agent:
      stable a:<handle> long-lived Job kind backed by one exact AW Session/branch
      a:0 is reserved root/default Agent
      additional Agents use owner-chosen immutable handles
      "subagent" is only a relationship/usage description, not a resource type

    Turn:
      stable t:<handle> identity for one durably ordered Agent generation
      optional monotonic ordinal is separate metadata, not encoded identity contract

    Step:
      internal model invocation + complete tool receipt phase inside Turn
      journal metadata only; no public s:<handle> ref

    ToolCall:
      activity/receipt; only durable/control-worthy nested execution becomes Job-backed
      finite Process/Graph children use j:<handle> beneath the owning Turn
      created Agents get stable a:<handle> identity with separate origin metadata

    ExecutionStrategy / Graph scope:
      every operation: Local | DelegateAgent; AW ordinary default Local
      Graph: Local | DelegateAgent(Graph) | DelegateAgent(Node)
      when Graph delegation selected: Graph is default, one Agent/Turn for entire Graph
      Node: dynamic Local/Agent choice at runnable node activation
      optional one-shot whole-Graph guardian is independent from execution shape
      core aw-tools: generic host-neutral prepared operation, route envelopes, Graph/Job callbacks; no aw-agent-kernel dependency
      AW adapter/host: Agent/model/payer/guardian policy and configurable advanced pipelines
      before roots capture all candidate routes and their conservative union footprints
      never changes original ToolCall/OperationId/ToolEffect/Graph topology/result contract
      Graph nodes: DL-150 native/shell plus AW agent; no nested batch via direct Graph leaves
      or Agent-mediated calls under a live Graph-node owner scope, including SDK/host
      nested Agents: allowed within the same inherited finite child/causal/payer ceiling
      original native/Process/Agent/Job/Graph executors own real effects and factual receipts
      no duplicate runner, Graph scheduler, model tool or paid routing in default Local path

    Discovery/data plane:
      jobs.list = flat bounded exposed-Job discovery with opaque older-than cursor
      jobs.get  = compact status + conditional rev/wait observation
      jobs.read = typed result/stream/history data with truthful continuation

    Generic reference:
      ref + optional rev where the operation defines it
      self-oriented get/read/send may omit ref -> current Agent
      jobs.cancel and jobs.subscribe always require explicit ref
      jobs.list has no ref
      jobs.read / jobs.ack have no rev
      non-slash refs are contextual overlays
      slash refs are fully resolved canonical identities
      new requests canonicalize once before effects; replay lookup first, reuse original target
      send/cancel/subscribe rev = compare-and-act
      get rev = conditional observation/change token

    Parent coordination:
      "^" resolves current authorized parent/coordinator at admission
      origin is immutable provenance; parent is mutable coordination relation
      neither path nor parent relation grants authority by itself

    Job specialization:
      each kind owns Meta and capability validation
      common model schemas stay strongly typed:
        SendInput = {text} | {eof:true}
        ReadView  = result|stdout|stderr|messages|tool_calls|tool_outputs|trace
        Event     = result|settled|attention
      resolved kind rejects unsupported combinations

    Send modes:
      auto / queue / steer / interrupt

    Control:
      first-class static effect across kernel/core/policy/Envelope; not Read/Write
      jobs.* operate on the exact canonical resolved ref
      queued Agent Input is custody, never future inference authority
      every Turn claim revalidates writer/provider/account/payer/budget/tool policy
      stale ref/rev never retargets

    Events:
      explicit source ref; Agent source==recipient rejected
      event_rev = source revision atomically committed with publication
      persistent replace/clear rules with exact source/event/event_rev/recipient dedupe
      constructor wake registration is atomic before first effect

    DL-150 Job simplification:
      job_start -> original constructor + background=true
      job_output -> jobs.read
      job_restart removed; re-run = fresh explicit replay-protected constructor
      retained Job definition is evidence, never executable authority

    Compatibility:
      none; hard pre-1.0 source cutover
      no migration / legacy decoder / drain / dual-state implementation

This keeps the model language small while making Jobs the single coherent durable execution/control abstraction and ExecutionStrategy the orthogonal AW host execution-backend seam. Models choose semantic actions; trusted AW may execute them locally or through an authorized specialized Agent without adding another model-visible workflow language.
