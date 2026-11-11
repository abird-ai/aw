# Design — AI-native unified Jobs + Agent runtime

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **FINAL DESIGN SEALED**, 2026-11-12. This defines the accepted vNext implementation target after fresh whole-plan DL-150/model-first/correctness reviews and the final AW ExecutionStrategy/delegation review. It does not claim the API exists.

No known model-facing or runtime-architecture design questions remain open in this plan. Implementation discoveries that would change tool names, schemas, reference grammar, lifecycle/replay semantics, Graph behavior or DL-150 inheritance must explicitly reopen this successor design rather than silently diverging in code.

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

The Graph admission resolver immediately maps the flat leaf name through the same mechanical mapping as direct provider calls (read -> fs.read, etc.) before policy/preparation/journaling. SDK/internal code may continue to use canonical semantic IDs such as fs.read.

Keep DL-150's coordinate, implicit sequencing and when grammar. No natural-language Graph DSL, output substitution, loops, nested batch, dynamic nodes or jobs.* Graph nodes.

### 2.5 Host-owned ExecutionStrategy

Semantic operation identity is orthogonal to **where/how AW executes it**.

After provider/SDK/Graph input has been resolved to one semantic operation, and after any host guardian/revision stage has produced an admissible candidate, AW may select a host-owned execution strategy:

    Local
    DelegateAgent

This is an internal AW execution seam, not:
- a model argument;
- another ToolEffect;
- another OperationId;
- a workflow/Graph language;
- a Job kind;
- an aw-tools Agent dependency.

The normal path is:

    immutable provider ToolCall / SDK operation
      -> semantic resolve + schema validation
      -> host guardian / decision / candidate revision
      -> revision authorization
      -> ToolPolicy / Envelope / exact semantic authority
      -> ExecutionStrategy selection + route authority
      -> scheduling / execution
      -> immutable factual receipt
      -> optional model-visible result projection
      -> parent Step receipt

A guardian/routing model may recommend Local versus DelegateAgent or a delegate role/target, but its output is evidence/advice only. Trusted AW policy makes the route decision and cannot mint missing authority, payer budget, tools, provider/account access or resource scope.

If routing/guardian logic uses inference, that decision is a separate bounded AW auxiliary invocation with its own InvocationIdentity, purpose, payer/usage authority and immutable receipt. It is replay/ambiguity-safe like other paid invocations: matching parent ToolCall replay reuses already committed decision evidence rather than silently paying for/rerunning the router. A failed/unknown decision invocation cannot authorize execution.

#### Semantic identity remains fixed

ExecutionStrategy never changes:
- original ToolCall ID/name/provider proposal evidence;
- canonical semantic OperationId;
- semantic ToolEffect;
- admitted arguments after any separately authorized revision;
- result/work ceilings;
- required factual effect/result contract.

The journal records the selected strategy separately from the operation. A delegated implementation cannot launder a different operation into the original receipt.

For an exact native operation such as read/write/bash, a child Agent's free-form answer is **not** proof that the native operation succeeded. The parent ToolCall can settle success only from authoritative evidence satisfying that operation's exact semantic/effect/result contract. High-level host operations whose contract is itself Agent-produced analysis may intentionally use the delegated Agent's typed settled result through an operation-specific adapter.

#### Delegation authority is additional, never implied

A semantic Read grant does not silently authorize paid inference merely because AW prefers a delegate.

DelegateAgent therefore requires independent current host authority for:
- Agent target/allocation and exact child Turn admission;
- provider/account/model and payer/budget use;
- delegated ToolPolicy/resource ceilings;
- causal-work allowance and child depth/work;
- any additional Process/network/external execution footprint.

Delegated authority must be no wider than the captured host-approved delegation ceiling **and the trusted semantic implementation contract**. Route selection never upgrades the operation's semantic authority. Additional execution mechanisms such as provider inference, private bounded scratch or an internal Process adapter require separate execution authority, but they may not introduce new user-visible/persistent semantic side effects that the operation does not permit. A semantic Read cannot become filesystem Write merely because an Agent executes it.

For exact native operations, the allowed semantic resource/effect footprint remains the native contract. For a higher-level host operation, registration may define a broader trusted implementation footprint (for example bounded reads plus Agent inference), but task text, router output or child model reasoning cannot widen it at runtime.

The delegate receives only a bounded host-authored task/context projection derived from the admitted operation plus explicitly delegated context. Parent transcript, provider-private context, credentials and unrelated branch state are not inherited by default.

#### Scheduling footprint is separate from semantic effect

ExecutionStrategy is frozen **before scheduling**.

AW derives a host-only execution footprint/class from the selected strategy:
- Local uses the ordinary native/Process footprint;
- DelegateAgent uses the delegate's captured execution ceiling, including inference, filesystem, Process, network/external and further child delegation.

This footprint affects scheduling/conflict admission but does not rewrite ToolEffect. A semantic Read routed through an opaque Agent is therefore not admitted into a parallel Read wave merely because its semantic effect is Read.

For direct model tool batches, strategy selection occurs before wave/concurrency scheduling.

Graph admission uses a two-phase immutable preflight before any root starts:

1. **semantic preflight for every node**: parse/schema, canonical operation, admitted arguments, ToolPolicy, base semantic authority, topology/when and semantic work/result ceilings;
2. **execution preflight for every node**: select ExecutionStrategy, perform any separately authorized guardian/router decision, validate route/delegate authority, capture execution footprint and freeze the strategy.

Only after **all nodes pass both phases** may the Graph build the final conflict plan and start roots.

Any paid guardian/router inference in phase 2 is separately journaled auxiliary preparation work, not a Graph leaf effect or branch result. If routing fails/unknown for any node, no Graph root starts; already incurred decision-model usage remains factual. Hosts that require zero paid preparation before complete strategy admission can use deterministic routing.

The selected strategy and captured footprint are frozen with node admission. Outer presentation, later routing-policy changes or task text cannot widen/change them. Graph node coordinates, when predicates, outcome, more/incomplete and captured semantic ceilings remain those of the original leaf.

#### Transparent delegated execution

DelegateAgent runs the operation through an ordinary AW Agent/Turn. The host router may select an already-authorized suitable Agent or allocate a new ordinary Agent through the same canonical Agent admission path; there is no "delegate Agent" resource type or second runner. A selected existing Agent must be in a state where the exact delegated Turn can be admitted under current ownership/writer policy; otherwise that route is ineligible or explicitly queued according to host policy, never silently injected into another active Turn. A newly allocated transparent completion-only delegate is normally closed after its delegated Turn settles unless host policy intentionally retains/reuses it; stable Agent/Turn identity and history remain factual, while closed helpers do not clutter live jobs.list discovery.

The delegated execution has its own:
- stable Agent/Turn/Invocation identity;
- provider/account/model choice;
- ToolPolicy and resource ceiling;
- payer/usage evidence;
- causal-work lineage.

The parent model does not run while the delegated Turn is executing, and the parent Agent's provider/model identity is unchanged. The delegate may use a different authorized provider/model/tool set without mutating parent Turn identity. The parent ToolCall/Step waits without consuming parent model inference and remains cancellable. When the delegated execution factually settles, AW adapts the authorized child result/evidence into the original ToolCall receipt and the parent Step continues normally. An optional result-guardian/transformer may itself use separately authorized auxiliary inference, but it operates only on the model-visible projection; it cannot rewrite the immutable semantic receipt/status/effect evidence. If projection inference fails, AW retains the factual receipt and returns/withholds projection according to host policy rather than fabricating a different execution result.

The model-facing default may therefore remain:

    c7 review_patch ok
    <bounded result>

while authorized journal/audit evidence retains:

    strategy = delegate_agent
    delegate_agent = a:<handle>
    delegate_turn = a:<handle>/t:<handle>
    route_policy/version
    delegated authority/footprint
    child outcome/result provenance

Delegation transparency hides orchestration noise, never factual provenance.

A transparently delegated action is **completion-only by default**. It may send nonblocking factual progress/Notify under existing authority, but it cannot block waiting for the synchronously blocked parent model. If the selected delegate requires request/response attention and no independent authorized controller exists, transparent delegation is ineligible: the host must choose Local/another completion-capable delegate or use the explicit Agent/Job handoff/control path. No nested callback into the blocked parent model is introduced.

#### Replay, ownership and cancellation

ExecutionStrategy selection/admission is replay-stable and is committed before delegated paid/external effects. The router may evaluate Local/delegate candidates before admission, but once one strategy is journaled/admitted there is no automatic fallback to another strategy after start or ambiguous delivery. Matching ToolCall/control redelivery reattaches to the same delegate Agent/Turn; it never reruns routing and chooses a different delegate.

Once admitted, later policy/router/model changes do not reinterpret the in-flight ToolCall.

The parent ToolCall owns the delegated Turn until factual settlement or an explicit authorized transfer/handoff. Parent Turn cancellation propagates only while that ownership still exists. Committed or uncertain child effects remain factual and can never be rewritten by parent cancellation.

If delegated execution settles with unresolved effect ambiguity, the parent operation cannot fabricate success; it preserves the corresponding truthful fail/unknown/blocked semantics required by the semantic operation and Agent Turn.

DelegateAgent work consumes the same finite external-origin causal-work/payer allowance already used by model-created Agents/Graphs, self/peer sends and subscription wakes. Routing is not a quota bypass.

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

interactive=false/omitted means stdin is limited to the constructor's optional initial stdin and is closed/retired afterward; jobs.send to that Process rejects live input. interactive=true retains explicit live-stdin custody for jobs.send until EOF/process settlement. interactive never changes filesystem/network/process authority.

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
- are canonicalized exactly once before authority/replay/effect.

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

before applying the same overlay rule. **Parsing is not admission.** These :0 values are fixed canonical objects, not caller-relative aliases. The host must have provisioned the referenced default objects and the caller must have current authority to use them; otherwise resolution returns unavailable/denied. A read, list, send or cancel never materializes a principal, project or root Agent. The current authenticated principal is represented by contextual inheritance/omission, not by remapping i:0. Standalone dotlink has no AW Agent context and does not use this fallback to authorize Job control; it requires its explicit live-session finite Job ref.

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

Contextual refs are canonicalized exactly once at the trusted admission boundary before authority, replay or execution. Replay/journaling preserve both requested_ref and fully resolved canonical resolved_ref.

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

Results are ordered by immutable newest exposure sequence first with canonical ref as deterministic tie-breaker. The opaque cursor means "continue strictly older than this exposure position"; activity cannot reorder an existing page.

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

The registry does not duplicate mutable lifecycle state. It stores only routing/exposure identity and bounded mappings/tombstones necessary for exact ownership/replay, including resolution of descendants that outlive finite ancestors.

A control call:
1. resolves a contextual ref once against the exact current Agent context, or accepts an already fully resolved slash-prefixed canonical ref;
2. records requested_ref + canonical resolved_ref in immutable evidence;
3. resolves canonical ref to its authoritative owner;
4. revalidates current actor/source/site/epoch/resource authority and any coordination-parent relation used by "^";
5. calls the owner's exact operation;
6. projects the factual result.

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

Internally each Agent ref binds exactly one existing AW owned Session + branch + Actor/writer identity. There is no second public Agent ID and no separate Agent mapping database.

A model-facing agent(...) call from current Agent /d:abird/i:pvl/p:zg/a:0:
- allocates another stable Agent handle in the same d/i/p namespace, for example /d:abird/i:pvl/p:zg/a:2;
- binds/creates exact durable Session/branch identity;
- records immutable origin Agent/Turn/Step/ToolCall evidence;
- establishes the authorized initial coordination parent, normally the creating Agent;
- starts its first Turn.

Agent creation is durably discoverable before paid provider effect. jobs.list reuses the authoritative SessionStore's existing immutable Session/Agent creation/commit ordering as the Agent exposure key; do not add a second Agent exposure database or a migration-only exposure field. Identity + Session/branch + parent/origin + first-Turn admission are committed as one replay-stable admission unit before inference, and the store's existing immutable creation evidence supplies deterministic Agent listing order. Matching replay recovers that same Agent; after replay retirement, jobs.list can still rediscover the live Agent root from authoritative SessionStore. The first successful synchronous result also includes the compact Agent ref so the normal path needs no extra discovery round.

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

steer means:
- target the exact active Turn;
- preserve and settle the current indivisible model/tool receipt Step **including all proposed ToolCall receipts**, never equate BeforeToolBatch with StepEnd;
- append a new **separately identified immutable controller-input receipt** to that same Turn, with issuer, source, request identity, admission revision and causal order; the original user prompt/provider-signed context remains untouched;
- compose this authorized additional Input only for the next model Step of that same Turn after current writer/model/provider/payer/tool/budget revalidation;
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

Before any next model Step after steer/interrupt, normal current writer/model/provider/account/payer/budget/ToolPolicy authority is revalidated; accepting the control Input never freezes future inference authority. If a started paid/model or external attempt cannot be proven settled, the Turn remains blocked/needs-inspection and **no automatic next Step** is dispatched, even though the steering Input remains a factual accepted receipt. Durable input admission, consumption, model dispatch and terminal Turn settlement are distinct evidence stages.

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

Child completion becomes the ToolCall receipt and allows the parent Step/Turn to continue if still current and authorized.

A child execution/Agent deliberately detached/transferred under a separately authorized ownership rule must not be silently cancelled by stale parent ownership. Initial vNext should avoid detachable ownership unless required beyond the stable Agent coordination-parent relation.

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

**Subscription/event ordering has one linearization point:** the *authoritative source Job owner* serializes rule installation/replacement/clear and event commit under its existing Job revision/transaction. Each matched committed event captures the effective recipient-bound subscription **generation and delivery mode at commit**, along with source ref, event, event_rev and recipient. Delayed delivery uses that captured rule generation, never whichever rule happens to be current when transport finally delivers it. A later replacement/clear affects only later committed events; it cannot undo an already accepted Input or retroactively reschedule an earlier event. Current recipient permission/revocation, causal-work budget and writer authority are still checked when converting that pending fact into an AW Input; revocation may suppress delivery. Keep the minimum bounded pending-delivery receipt alongside the existing source Job/Actor journal, not a second event bus. For finite live-session Jobs, owner loss yields an inspectable unavailable/unknown delivery fact rather than a false cross-crash wake guarantee.

Wake dedupe identity is exact (canonical source ref, event, event_rev, canonical recipient Agent ref, captured subscription generation). Redelivery of the same committed event cannot create a second wake Input or paid Turn. A subscription replaced before event commit selects the new generation; one replaced after event commit does not reinterpret that event. If the source is an AW Graph relaying child attention, record the source node/child origin without adding a second child event owner.

Subscription is receiver-owned and current-authority checked at registration and again at delivery. Delivery of a queued wake has the same rule as direct jobs.send: durable Input acceptance is not inference authority, and any later Turn claim revalidates current writer/provider/account/payer/budget/ToolPolicy authority.

**Causal loop and work admission:** a subscription wake is one form of the **same model-origin autonomous continuation** bounded in Section 7.5. A wake Input, any subsequent model-issued self/peer `jobs.send`, and model-created child Agent/Graph work carry the original authenticated causal root and its remaining allowance through immutable Input/Turn/ToolCall evidence. Automatic wake cycles (A -> B -> A) are rejected when the recipient recurs in that wake chain; repeated direct self-continuation is permitted only under the finite explicit self/peer/delegation allowance, never an infinite bypass. Apply host-capped causal hops, recipient pending Inputs, total automatic Turn/child-work/payer budget and rate at acceptance/admission; exhaustion suppresses *new* autonomous work but leaves source events and factual effects inspectable. No automatic subscription is installed by default. Only an independently authenticated external user/host Input can open a new causal root; model text or delegated work cannot. The original AW Actor/Session/journal owner meters it; no second event database or model execution engine.

jobs.subscribe replaces/clears the future rule in the authoritative owner; it does not create a second event database. Unknown wake delivery never triggers blind paid inference replay.

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

Ordering is immutable newest exposure/creation sequence first, with canonical ref as deterministic tie-breaker. Activity never reorders an existing exposure. Agent creation sequence comes from the authoritative Agent/Session record; finite exposure sequence comes from the bounded Job exposure index. Neither is a second lifecycle/history database.

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

Provider conceptual shape:

    jobs_read({
      ref?,
      view?,
      cursor?,
      offset?,
      limit?,
      encoding?,
      tools?,
      wait_ms?
    })

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

agent is Graph-eligible.

A Graph Agent node:
- creates one stable Agent a:<handle> in the current d/i/p Agent namespace and admits its initial Turn according to the node args;
- records Graph/node/Turn/Step/ToolCall origin metadata separately from the Agent's stable identity;
- is one immutable Graph node invocation;
- node terminal status corresponds to that admitted Turn's factual outcome, not the lifetime of the long-lived Agent Job;
- may return/expose the stable Agent ref (for example a:2) as a continuation handle when authorized;
- does not keep the Graph node running merely because the Agent Job remains idle afterward.

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
- jobs.send Process additionally requires exact resolved Process/input custody;
- jobs.send Agent additionally requires exact resolved Agent/Turn/Actor Input authority;
- accepting Agent Input does not authorize future inference: every queued Input claim revalidates current Session writer/source lineage/model/provider/account/payer/budget/ToolPolicy/resource ceilings before immutable Turn admission;
- jobs.cancel Agent Turn additionally requires exact Turn generation ownership;
- jobs.subscribe additionally requires current receiving Agent wake authority, and delivery revalidates it again.

Dynamic policy may deny, narrow or ask within the static ceiling but never mint missing authority. A durable ref, queued Input, subscription, prior parent relation or historical execution authority is never a capability token for future paid/model/tool execution.

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

Every Job-producing constructor must establish replay identity before external effects.

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

When a queued Input is claimed, the owner establishes a distinct replay-stable Turn admission identity only after current writer/inference/payer/tool authority succeeds and before paid provider effect. A crash/lost response may recover that same admission, but stale queued authority can never silently authorize another Turn.

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
- the removed team workflow tool is parallel/serial composition of ordinary Agent Jobs; this does not remove team as a Principal kind in the identity namespace;
- plan is ordinary Agent reasoning + Graph/Agent composition;
- review release belongs to policy/approval authority, not a general model primitive.

If product code still uses internal team/planner libraries for UI or experiments, they may remain non-tool implementation details only if independently needed. They must not create a competing Agent runtime or canonical tool surface.

Human/policy approval belongs in the existing policy/approval path and resumes/retries only through explicit factual authority; no model-facing release tool is required.

## 20. DL-150 Job simplification, hard pre-1.0 cutover and restart removal

The new vNext intentionally breaks the pre-1.0 implementation/API. Preserve DL-150's **correctness invariants**, not obsolete code paths, wire names, state formats or compatibility obligations. This is a clean hard cutover: one new catalog, one new parser/control plane, no dual registration, migration layer, compatibility decoder or legacy replay path. **Only verified isolated, explicitly disposable development/test state can be reset; a pre-1.0 label is not permission to discard live external obligations.**

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
| Native MCP Tasks and OpenAI redelivery | **Keep as transport projections** | same Job/result owner, no second execution, protected replay tombstones/horizons |
| Linux/Windows/macOS native safety and progress | **Keep unchanged** | rooted descriptor authority, sandbox/denies, cumulative fixed/unknown progress |
| Schema/result-token budget | **Measure at vNext cutover** | flat names, small schemas, compact results, no repeated args or avoidable model/tool rounds |

This ledger supersedes **only** the old conceptual Job surface, restart and identity naming. All underlying filesystem, Graph, mutation, conflict, output, replay, native-platform and Task correctness from sealed DL-150 remains normative for the new implementation. Do not retain obsolete pre-vNext code merely to replay or migrate its old wire/state forms; port the invariant to the new canonical implementation and delete the superseded implementation.

## 21. Shared-layer placement

### aw-tools

Own reusable, host-neutral:
- canonical fs operation IDs/schemas/requests;
- Graph grammar/planning/status/footprints;
- finite Graph Job state/result retention where already shared;
- common hierarchical ExecutionRef/ResolvedExecutionRef, Revision, Job kind/read/control request/result types where genuinely generic;
- routing traits/interfaces for heterogeneous Job owners;
- typed Text/ExactRead/Bytes/Media representation;
- bounded cursor/result helpers.

Do not add AW Session/provider/Actor dependencies. Share only small syntax/typed-handle, semantic operation/result/footprint and Job request/result primitives that have concrete Process/Graph/Agent consumers. **ExecutionStrategy/OperationRouter, guardian/routing inference, delegate Agent identity and model/provider selection are not aw-tools concepts.** Implement d/i/p/a defaults, coordination-parent `^`, current Agent, cross-principal authorization and alias-free routing only in AW. Prefer a small tagged owner dispatch over an extensible generic Job-kind plugin framework until a fourth real Job kind exists.

### aw-tools-adapter

Own AW host projection/integration:
- ToolSpec flat provider projection;
- PreparedOperation mapping;
- the host execution-dispatch seam wrapping a PreparedOperation with selected Local/DelegateAgent strategy and captured execution footprint;
- local Process/native executor adapters;
- Graph Job host integration so strategy is frozen before conflict planning;
- unified Job registry/router glue;
- SDK adapter bindings where appropriate.

It must not own provider/model policy or a second Agent runner.

### AW Agent/runtime/store

Own:
- principal/project/Agent namespace and current-Agent relative-ref resolution;
- stable Agent a:<handle> identity, reserved a:0 root/default handle, immutable origin and mutable authorized parent/coordinator relation;
- Agent Job Session/branch binding;
- Turn generation/writer;
- Step/model invocation;
- ToolCall journal;
- host OperationRouter/guardian/decision policy that selects Local versus DelegateAgent;
- delegate target/factory selection, independent inference/payer/tool/resource authorization and causal-work reservation;
- delegated child Agent/Turn ownership, parent wait/resume/cancellation and semantic result adaptation;
- child Job/Agent ownership relations;
- jobs.send Agent input/steering;
- jobs.cancel Turn/Agent;
- Agent history JobReadPage projection;
- disclosure/redaction;
- wake recipient admission.

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

Standalone dotlink does not create Agent Jobs and does not implement AW ExecutionStrategy/OperationRouter delegation. It executes its standalone Process/Graph semantics locally. With an authenticated AW bridge, AW remains the routing/delegation authority; the same wire jobs_* calls resolve/canonicalize Agent refs and route them back to AW ownership.

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
3. model/tool rounds;
4. result tokens;
5. recovery/cancellation correctness.

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
- lost first Agent creation response recovery.

Any material schema/token regression must be justified by measured reliability/capability rather than assumed from tool count.

## 24. Correctness acceptance matrix

### Typed handles, contextual resolution and revisions
- segment types are d/i/p/a/t/j/n; all have immutable canonical identity. Graph n is **exactly its 1-based decimal Graph coordinate** (n:0 and leading zeros invalid), not another owner-allocated opaque handle;
- allocator format is implementation-defined, but canonical spelling is one 1..64-byte delimiter-safe ASCII token with no normalization/escape aliases;
- d:0/i:0/p:0/a:0 are fixed provisioned canonical default objects, never caller-relative aliases; /d:<h>/i:0 has one identity for every caller, while contextual inheritance represents the current principal; t/j value 0 is ordinary, n:0 invalid;
- slash-prefixed canonical refs are fully resolved and include explicit d/i/p/a, including :0 defaults;
- partial slash refs such as /a:0 or /i:pvl are invalid;
- contextual refs resolve exactly once from current Agent context before authority/replay/effect;
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

### Authority/effects
- Control is a first-class exhaustive portable effect across kernel/core/shared-tool/adapter/policy/Envelope/journal/store projections;
- Control never inherits filesystem Read/Write authority and never joins parallel Read waves;
- default Off/ReadOnly remains fail-closed except for explicitly granted Control operations;
- Graph outer Control admission never substitutes for exact leaf Read/Write/Process authorization.

### ExecutionStrategy / transparent delegation
- ExecutionStrategy is host-owned AW policy only: Local or DelegateAgent; no model/provider schema field, OperationId, ToolEffect, Job kind or pure aw-tools Agent dependency;
- original ToolCall identity, canonical operation, admitted args, semantic ToolEffect, semantic ceilings and factual result/effect contract remain unchanged by strategy;
- guardian/routing inference can recommend a route but cannot grant route/inference/payer/tool/resource authority;
- DelegateAgent requires independent exact Agent/provider/account/model/payer/causal-work/tool/resource authority; a semantic Read grant cannot implicitly authorize paid inference;
- additional execution mechanisms may consume separately authorized inference/private-scratch/process resources, but delegated user-visible/persistent semantic side effects stay within the trusted operation implementation contract; semantic Read never becomes filesystem Write merely because it is delegated;
- strategy and exact delegate admission are frozen before scheduling/effects and replay; retries reuse committed guardian/router decision evidence, reattach the same child Agent/Turn, later router changes do not retarget, and ambiguous started delegation never silently falls back to another strategy;
- scheduler uses a separate captured execution footprint; semantic Read through opaque delegation does not join a Read wave;
- parent model remains idle while delegated Turn executes; child may use a different authorized model/provider/tool set without mutating parent Agent/Turn/model identity;
- delegate receives bounded explicit context only, not parent transcript/provider-private context/credentials by default;
- child result passes through a trusted semantic result adapter; free-form Agent text never fabricates exact native effect success and visible-result transformation never changes factual receipt;
- completion-only is the transparent default; a delegate cannot block waiting on its synchronously blocked parent model without an independent controller/explicit Agent-Job handoff path;
- parent cancellation propagates only while exact delegated ownership remains; committed/unknown child effects stay factual;
- DelegateAgent consumes the same finite authenticated causal-work/payer allowance as explicit child Agent/Graph work.

### Steps/tool calls
- complete tool batch receipt belongs to Step;
- started external effects retain factual outcome;
- child Job correlation exact;
- no Job explosion for atomic fs calls.

### Waits
- waiting consumes no model invocation;
- cancellation remains serviceable;
- no held global runtime lock;
- condition completion resumes only if owning Turn is still current.

### Events
- advertised event enum is result|settled|attention and resolved kind validates support; AW Graph exposes attention iff admitted topology contains an attention-capable Agent node. Completion-only is the default; parent-blocking attention exists only when immutable captured host delegation explicitly permits that request/response path;
- constructor wake registration is atomic with Job admission before first effect;
- subscription rules persist until replace/clear/source terminality/authority revocation and are non-retroactive; EventRule is a unique semantic set, duplicate event names reject and replay hashing uses canonical enum order result,settled,attention;
- subscription delivery uses only auto/queue/steer/interrupt;
- exact source-ref/event/event-rev/recipient dedupe prevents duplicate wake Inputs, while bounded causal lineage/recipient queues/automatic paid-Turn budgets also prevent **new-event cross-Agent cycles**;
- finite result+settled duplicate wake coalescing, with conflicting coalescible modes rejected during subscription validation;
- Agent per-Turn result does not settle whole Agent Job;
- steer/interrupt wake binds receiver's exact current Turn ref/rev at delivery;
- queued wake acceptance never substitutes for current inference/payer/tool authority at Turn claim;
- no stdout/tool text forges attention; an AW-hosted Graph may project authenticated Agent-child attention with its n:<coordinate> and authorized child/Turn reference; standalone dotlink never fabricates it.

### Read
- jobs.list scope is exactly canonical d/i/p or standalone runtime generation; its owner-verifiable cursor binds scope/runtime, listing authority epoch, exposure position/tie-breaker and projection version, so cross-context replay/tampering rejects;
- live Agent roots are discoverable from authoritative SessionStore's existing immutable creation/commit ordering without a second Agent exposure database or migration-only field;
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
- Agent footprint conservatively covers complete delegated authority (native FS, Process reach, external/network/extension effects, child delegation);
- Graph preflight is two-phase and root-free: all leaves first pass semantic/schema/ToolPolicy/topology/when/ceiling validation; then all strategies/route authorities/execution footprints freeze; only then conflict planning/root start. Any guardian/router inference in phase 2 is separately journaled preparation usage, not a leaf effect/result;
- DelegateAgent keeps original coordinate/semantic operation/outcome/when/more/incomplete/ceilings while scheduling uses its captured delegated footprint;
- opaque Process/network/provider/external effects **conflict with interacting Graph nodes** by default; explicit Graph dependencies give additional ordering but no outside transaction guarantees;
- the same captured attention-capable Agent predicate drives proactive handoff: if such a child would otherwise depend on the synchronously blocked sole controller, the same Graph Job is handed off before roots run;
- all leaves authorized before roots;
- outer presentation cannot change node status/more/incomplete/when;
- long-lived Agent Job does not keep its initial Graph node nonterminal after the admitted Turn settles.

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

    ExecutionStrategy (AW host only):
      Local | DelegateAgent
      selected after semantic/revision/ToolPolicy admission, before scheduling/effects
      never changes ToolCall/OperationId/ToolEffect/admitted args/result contract
      separate captured execution footprint controls scheduling/conflicts
      DelegateAgent = ordinary Agent/Turn with independent provider/model/payer/tools
      parent model waits without inference; settled semantic result returns as original ToolCall receipt
      completion-only transparent default; replay reattaches same delegate; same causal-work budget
      not a model field, Job kind, OperationId or aw-tools Agent concept

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
      contextual refs canonicalize once before authority/replay/effect
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
