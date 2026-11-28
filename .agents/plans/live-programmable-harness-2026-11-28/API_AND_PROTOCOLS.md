# API and protocols — proposed canonical contracts

Date: 2026-11-28
Base: a47492f45813f1e85f85e706e7f33292f0c3e6f0
Status: implementation specification; these target APIs are not shipped.
Related: [Design](DESIGN.md), [Foundations](FOUNDATIONS.md),
[Authoring](AUTHORING.md), [Learning and delivery](LEARNING_AND_DELIVERY.md).

This file owns proposed public names, request semantics and adapter rules.
Phase documents reference it; they must not define competing wire contracts.
Implementation may refine Rust placement without changing the semantic contract.
HP-1–3 establish the complete shared contract surface and executable common
foundation. HP-4 onward expand independent component implementations over it.

## 1. Current surfaces versus target

| Surface | Current source | Proposed transition |
|---|---|---|
| Embedded Rust | AgentBuilder, aw-host LocalHost, individual host modules | Service with typed domain facets over the same owners |
| Attached IPC | aw-client protocol v1, same-UID Linux socket | Explicit v2 adapter using the shared domain contract |
| Headless stdio | aw.rpc v1, underscore method names | Explicit aw.rpc v2 with dot-separated methods |
| MCP client | aw-mcp consumes external servers | Preserve its role and exact tool authorization |
| MCP server | No general AW control server | Feature-gated adapter over Service |
| Process Extension | aw-extensions ABI 2 tools/hooks/prompts | Separate successor ABI for scoped full-duplex callbacks |

The three version spaces are independent: AW control API, MCP protocol, and
Process Extension ABI. A matching integer never implies compatibility.
AW control v2 and successor Extension ABI 3 are adopted design values for this
plan, not claims that those versions are implemented. Legacy decoders retain
their actual validation and replay behavior.

### Foundation delivery contract

The foundation establishes every public family already named in this plan,
including the boundaries needed by later optional implementations. A list of
future method names alone does not satisfy this requirement.

| Contract family | Established during HP-1–3 | Later component work |
|---|---|---|
| Public operations | Typed request/result definitions and bounded schemas for every §7 method; native/wire projection; read/mutation identity; original owner mapping | HP-4 broadens client conveniences and transport/profile coverage; each component supplies its own implementation |
| Invocation and effects | Host-minted CallContext, role scope, original operation/cancellation identity, deadline/budget narrowing, receipts and typed errors | Components consume the same admission/cancellation owners |
| Composition | HarnessDefinition, exact component references, typed registration handles, preparation, dependency/state compatibility, binding CAS and retirement | HP-5 adds context/strategy algorithms; HP-6 adds dynamic implementations |
| Observation and events | Owner-local query/page/cursor types, bounded read/watch, typed Event contracts, disclosure and current authorization | HP-7 adds experiment projections; new views reuse the same observation contract |
| Experiments and evaluation | ExperimentSpec/Start/Report, CaseSetRef, EvaluationContract, TrialRef/Measurement and evaluator invocation/result contracts | HP-7 implements trial dispatch, scoring and reporting through existing execution owners |
| Improvement and policy | ImprovementSpec, PromotionPolicy and the activation eligibility input/result contract; no separate promotion operation | HP-8 implements the controller and policy module; it depends on HP-7 reports |
| Inference and environments | Configured Provider/source identities, InferenceTargetRef/ResolvedInferenceTarget, capability/continuation/usage shapes and qualified resource-operation schemas | HP-9 adds richer adapters; qualified environment implementations evolve in their existing lane |
| Transport and extension wire | Control v2 framing/compatibility, MCP method/result mapping, ABI-3 manifest/callback/ctx schemas and version/refusal rules | HP-4 adds optional transports/profiles/Tasks; HP-6 implements expanded process callbacks and TS/JS authoring |

HP-1 supplies the shared public operation types and cross-cutting semantics.
HP-2 supplies the typed contributor/data contracts and composition relationships.
HP-3 completes runtime composition and exercises the baseline through native
Rust, attached Rust and a minimal real MCP adapter. Its implemented common path
includes discovery, create/send/observe/cancel and harness prepare/inspect/activate.

All declared optional request/result and component contracts must type-check
and produce their boundary schemas where encoding is required. The foundation
also compiles an external consumer against public crates, demonstrating a native
contributor without private root-application imports. Existing definition owners
and public role contracts are reused rather than wrapped in a universal plugin
object. Future algorithms, evaluator engines and every optional transport do not
need to be implemented to establish those boundaries.

A declared schema is not a runtime capability. Missing optional implementations
are absent from supported capabilities and receive a shared `unsupported` result
before new effects. Do not create empty managers, fake successful handlers or
parallel placeholder registries. Original receipt lookup remains available:
capability withdrawal never erases an earlier outcome or permits re-execution.
Known configured behavior whose required dependency is temporarily missing uses
`unavailable`, preserving its exact identity.

The [learning phase contracts](LEARNING_AND_DELIVERY.md) spell out experiment,
measurement, policy and inference fields; those data contracts are delivered
during the foundation, not discovered when those engines are implemented.
[FOUNDATIONS](FOUNDATIONS.md) owns the integrated foundation exit criteria.

## 2. Native contract and authority

Proposed shared value types belong in a dependency-light aw-control crate when
HP-1 needs both aw-client and aw-host to consume them. It has bounded DTOs,
schema/validation and serde support, not an Agent, Store, transport or scheduler.
Pure local embedding remains possible without that transport-facing dependency.

```rust
// Proposed API shape; not compilable against the current repository.
pub struct Service { /* handles to existing host/Store/runtime owners */ }

pub struct CallContext {
    // Private: authenticated principal, owner scope, admission authority,
    // execution binding, original invocation namespace, disclosure context.
}
// No Deserialize or public constructor from arbitrary caller fields.

pub struct RequestMeta {
    pub operation_key: Option<OperationKey>,
    pub deadline: Option<Deadline>, // requested ceiling; host may narrow
}

impl Service {
    pub fn system(&self) -> System<'_>;
    pub fn agents(&self) -> Agents<'_>;
    pub fn inputs(&self) -> Inputs<'_>;
    pub fn jobs(&self) -> Jobs<'_>;
    pub fn harnesses(&self) -> Harnesses<'_>;
    pub fn extensions(&self) -> Extensions<'_>;
    pub fn observations(&self) -> Observations<'_>;
    pub fn events(&self) -> Events<'_>;
    pub fn context(&self) -> Context<'_>;
    pub fn sandboxes(&self) -> Sandboxes<'_>;
    pub fn checkpoints(&self) -> Checkpoints<'_>;
    pub fn experiments(&self) -> Experiments<'_>;
    pub fn operations(&self) -> Operations<'_>;
    pub fn inference(&self) -> Inference<'_>;
}
```

CallContext is the authenticated service context. The existing
aw_policy::OperationContext remains the pure policy-query descriptor; neither
its public fields nor deserialization confer authority.

Native operations take a borrowed CallContext separately from their request.
Authenticated adapters mint it from their actual connection/owner binding.
An external client cannot select its actor, writer epoch, source grant, payer,
execution site or credential custody by adding fields to a request.

A request can select an authorized resource or configured account where the
existing operation supports that choice. Selection is independently resolved
and checked; it is never an authority assertion.

Service construction starts no listener, package installation, model call,
extension process or scheduler. Optional capabilities become available only
after their existing owners are explicitly configured. Facets are cheap borrowed
views of that service; each name does not require an allocated manager, object-safe
trait or mandatory implementation dependency. Optional domain contract modules
must remain usable by clients without pulling their execution engines into them.

### Bound SDK facade and waiting helpers

The public embedding entry point is `aw_host::Service`, re-exporting
`aw_host::service::Service`. The crate root also re-exports shared AgentSpec,
HarnessDefinition, ActivationRequest and ExperimentSpec values. Attached callers
use `aw_client::Client` and the same pure request types re-exported by aw-client;
they never need aw-host or the CLI root library just to build a request.
Re-exports do not enable CLI, UI, HTTP/MCP server or durable-storage features.
Native PreparedHarness custody remains on the host side of the contract.

The mature product facade binds the host-authenticated caller once, while every
operation still revalidates current authority. It is an ergonomic projection of
the typed Service contract, not a second API owner or wire protocol.

```rust
let service = Service::embedded(config).await?;
let aw = service.client();
let agent = aw.agents().create(AgentSpec::new("researcher")).await?;
let result = agent.run("Map the architecture of this repository").await?;
service.shutdown().await?;
```

An attached program substitutes `Client::connect(endpoint).await?` for the
embedded construction and uses the same facet semantics. The native client calls
typed Service operations directly; it does not encode JSON or start a listener.
The attached `aw-client` remains independent of aw-host, Agent and Store code.
Share small typed request/observation helpers where useful; matching ergonomics
does not require a universal backend trait or another dependency-heavy SDK.

| Facade operation | Exact lowering and result |
|---|---|
| Service::embedded(config) | Construct the configured original host/runtime/resource owners in-process; no listener, package discovery, implicit inference or second scheduler |
| Service::client() | Borrow a client bound to the trusted principal/scope selected by host configuration; mint fresh CallContext for calls, not caller-supplied grants |
| Client::connect(endpoint) | Attach to an explicitly configured owner and authenticate through its transport; no local Store ownership |
| agents().create(AgentSpec) | Submit one agents.create mutation and observe its original construction receipt; return a bound Agent handle after factual creation, retaining that receipt/key |
| AgentSpec::new(name) | Use already-authorized configured harness/environment defaults; resolve their exact identities under the original creation intent before effects |
| AgentSpec::harness(revision) | Pure local request construction selecting that exact revision; no installation, grant or activation mutation |
| agent.run(input) | Submit one agents.send with one retained operation key; observe original Input admission and resulting execution until settlement; return its authorized result/receipts |
| agent.harness() | Authorized harnesses.binding read; revision() is the desired binding and binding_revision() is its CAS revision |
| harnesses().prepare(definition) | Submit the sole prepare mutation; return a Candidate retaining the original receipt and summary, with optional native PreparedHarness custody |
| experiments().compare(spec) | Submit one experiments.compare; return ExperimentRun retaining original operation/experiment/execution references as available |
| ExperimentRun::wait() | Observe that original execution and read its sealed report after factual completion; no additional trial admission |
| harnesses().activate(request) | One canonical activation mutation and its existing receipt; no automatic compare, rebase or policy bypass |
| Service::shutdown() | Stop new admission and shut down/drain owned runtime work through original cancellation/settlement owners; report unresolved work honestly and release owned resources through normal lifecycle |

`AgentSpec::new` does not reinterpret defaults on a retry. An explicit harness
overrides only selection within current authority. A created Agent may still
report unavailable dependencies through normal readiness; creation never performs
its first run implicitly. Shutdown does not delete durable Agent resources or
claim remote effects/billing stopped merely because the local host exited.
Dropping an attached client detaches observation and does not shut down the host.

The low-level Service/wire APIs retain their `OperationReceipt<T>` results.
SDK Agent/Candidate/ExperimentRun values retain the original receipt and key;
they are client references, not additional execution or authority objects.
Candidate.revision() exposes the exact durable revision, and readiness remains
inspectable even when its optional native implementation closure is absent.

A mutation helper creates one semantic key before its first send and retains it
through waiting and transport retries. Pending helpers allow the caller to
capture that key before awaiting when recovery must outlive the client task.
Errors preserve the key, available original refs and uncertainty. Retrying an
uncertain helper never generates a replacement key automatically; recovery uses
operations.get or the original run handle. Dropping a wait detaches that wait;
explicit cancellation still targets the original owner. No client-side durable
execution ledger is introduced.

`agent.run` waits using bounded original-owner observation rather than repeatedly
submitting input. It does not assume an immediate execution_ref or infer that a
Job snapshot is terminal. The returned result is the authorized completed output
projection and references to retained output; it need not inline unbounded text.

`ExperimentRun::wait` returns a sealed report only after the original experiment
reaches factual completion. Cancelled, unresolved or incomplete execution returns
a typed outcome/error retaining the experiment and any report reference, rather
than presenting a partial snapshot as completed. A completed report can still
contain failed/refused trials, incomplete usage or an unfavorable conclusion;
coverage and promotion policy retain their normal meaning.

All builder methods below construct typed values locally:

```rust
let definition = HarnessDefinition::new("reviewer")
    .context(repo_context)
    .strategy(plan_work_review)
    .route(model_route)
    .toolset(workspace_tools);

let candidate = aw.harnesses().prepare(definition).await?;
let agent = aw.agents().create(
    AgentSpec::new("reviewer").harness(candidate.revision())
).await?;
```

The context/strategy/route/toolset arguments are registered typed references.
HarnessDefinition::new starts a local definition; the shown builders supply its
required component slots. Optional settings use schema-defined local defaults.
Preparation resolves selections to exact references under the original candidate
intent. Builders do not discover host configuration, register code or run effects.
`ActivationRequest::next_turn(agent, revision, expected_binding)` constructs the
canonical request; `.evidence(report_ref)` supplies its optional evidence field.
These are convenience constructors, not new wire operations.

```rust
let current = agent.harness().await?;
let mut draft = aw.harnesses().fork(current.revision()).await?;
draft.context = focused_context;
let candidate = aw.harnesses().prepare(draft).await?;

let report = aw.experiments().compare(ExperimentSpec {
    baseline: current.revision().clone(),
    candidate: candidate.revision().clone(),
    ..comparison
}).await?.wait().await?;

aw.harnesses().activate(
    ActivationRequest::next_turn(
        agent.id(), candidate.revision(), current.binding_revision()
    ).evidence(report.id())
).await?;
```

`comparison` is an explicitly configured ExperimentSpec with authorized cases,
evaluator, effect/environment policy, initial state, bounded parallelism and
budget. The struct update changes only its two harness references. Each displayed
mutation has its own original semantic key; the helpers do not hide a chain of
additional mutations. Competing activation remains conflict; report quality and
current policy decide eligibility.

### Typed contributor and host-port boundaries

HP-2 establishes the public authoring boundaries below using the existing R6
role contracts. Each registration retains its actual Rust input/output types.
Pure native registrations need no serde bound or wire schema; a codec and explicit
schema version are required only for an encoded/storage/model boundary.

| Contribution | Foundation contract | Existing runtime boundary |
|---|---|---|
| Context and instructions | ContextProgramRef and typed ContextProjection; ordered authorized inputs, provenance, truncation and bounded output | Original ContextFlow/summary acceptance; prompt hooks retain their narrower edit contract |
| Strategy and routing | StrategyRef for original loop/pipeline/registered controller; exact route references; typed work requests and receipts | Original Agent/TurnMachine, pipeline and Provider selection; no new inner loop |
| Tools, Hooks and Actions | Existing Tool/Hook contracts, typed Action input/output and local handles; role-specific invocation context | Exact ToolBindingAuthority, allowed Hook stage/Next/Defer, original effect admission |
| Resources and Views | Typed inert content; View resource/query/output types and bounded failure | Existing resource authorization and disclosure; views stay read-only |
| Events and handlers | EventKey<T>, schema/provenance, typed handler input and existing acknowledgment/cursor semantics | Original Store Event log, subscription and Input delivery |
| State and settings | StateKey<T>, StateSnapshot<T>/expected sequence and SettingKey<T>; explicit schema/semantic scope | Existing state/configuration owners; secret settings retain Vault custody |
| Semantic UI | Existing typed presentation/action bindings and role-scoped interaction results | Surface host and Action owner; no renderer or product view owns Agent execution |
| Evaluation | Exact evaluator reference, authorized case/output inputs, bounded score/measurement result and coverage | Ordinary admitted Agent/Job/pipeline work; scoring has no activation grant |
| Inference and native drivers | Existing Provider/SourceDriver/SandboxDriver/VaultProvider contracts plus typed optional capability/target facts | Their original executor/maintenance owners and native-only limits |

Native ExtensionContext may expose role-scoped Agent/input operations as well as
tools, state, events, settings, models, process and UI, with original cancellation,
deadline and causal budget. These ports call existing typed operations and never
grant mutable registry access. A View receives the narrower read context.
The initial Process Extension ABI exposes only ctx.tools/state/events/settings/
models/process/ui under its host-minted invocation scope; it does not inherit
every native port. Agent/input submission remains the canonical Service/Input
operation. Direct dynamic ctx.agent/ctx.inputs would require an explicit supported,
versioned schema and authority contract, not implicit inclusion from native context.

Use the R6 typed registration pattern: register a concrete role, receive a
typed handle, resolve/invoke that handle through its owner. Metadata descriptors
support discovery; native callers do not downcast them or serialize through
`serde_json::Value` to reach their implementation. Heterogeneous storage may
use private erasure at the actual runtime dispatch boundary.

The shared registration lifecycle covers stage/validate/publish, retained exact
use, withdrawal and retirement. A component's state schema, role, authority
requirements and compatibility declare how that lifecycle applies; no new
generic lifecycle engine replaces its existing owner. Registration identity,
component schema, implementation revision and root snapshot generation remain
distinct. Root publication gives one coherent composition and does not stale
unrelated typed handles.

The foundation makes these seams usable and compile-checked. Concrete algorithms
and additional dynamic role adapters can then evolve without changing the service
context, receipt semantics, registry ownership or transport envelope.

## 3. Capabilities and versions

CapabilitySet describes supported operations, bounded modes and schema versions.
It is not a permission token. Discovery must filter protected object metadata;
a supported method may still refuse a particular caller, resource or effect.

Proposed discovery response:

```json
{
  "api_version": 2,
  "capabilities": {
    "harnesses": ["read", "prepare", "activate.next_turn"],
    "agents": ["read", "create", "send", "close"],
    "observations": ["read", "watch"],
    "experiments": [],
    "durability": "sqlite"
  },
  "limits": {
    "request_bytes": 196608,
    "request_timeout_ms": 30000,
    "page_items": 200,
    "watch_wait_ms": 30000
  }
}
```

These are proposed v2 ceilings, not copied claims about current transport limits.
The host may advertise lower effective bounds. Actual operation schemas impose
additional collection, text, recursion, result and resource limits.

Initial v2 control request ceiling: 192 KiB; response frame ceiling: 640 KiB;
observation event ceiling: 64 KiB; page count: at most 200; bounded watch wait:
at most 30 seconds. Large results use existing authorized retained-output or
artifact references, never an unbounded inline result.

Current limits remain distinct:

| Existing protocol | Verified limits |
|---|---|
| aw.rpc v1 | 192 KiB request; 64 KiB event; 640 KiB frame; request IDs up to 64 bytes |
| aw-client IPC v1 | 1 MiB request; 32 MiB response; 128 KiB prompt |
| Process Extension ABI 2 | 256 KiB frame; 128 KiB manifest; 8 processes; 32 tools; 3 hook names |

V2 must not silently reinterpret v1 numeric or method formats. New unbounded
counter/revision values use decimal strings on the wire to preserve precision
in JavaScript; native types remain checked integers. Legacy adapters retain
their old encodings and reject values they cannot represent safely.

Requests remain closed: unknown fields are rejected, including new authority
fields. Clients may ignore additive response metadata, but must refuse unknown
required capabilities, behavior discriminators or semantics before acting.
A new behavior requires an explicit supported capability/version; it cannot be
smuggled through an ignored field. Optional metadata never changes authority.

Component evolution is local. Each encoded component contract has its own schema
version and compatibility rule, and executable registrations retain their own
identity/revision. Adding a compatible implementation or optional capability must
not advance one global revision clock or require coordinated upgrades of unrelated
components. A real shared-contract change uses an explicit compatible addition
or version transition; it is never hidden as a private implementation change.
Native Rust updates may require rebuilding their consumers. The contract does not
promise an eternal Rust source ABI or hot-loadable native binary ABI.

## 4. AW control envelope

The proposed aw.rpc v2 envelope is bounded JSONL. It is not JSON-RPC and is not
the Process Extension wire. IPC v2 can reuse its domain request/result types
while retaining authenticated framing appropriate to the socket.

```json
{
  "v": 2,
  "id": "request-17",
  "method": "harnesses.activate",
  "operation_key": "activate-repo-helper-9",
  "timeout_ms": 10000,
  "params": {
    "agent_ref": "/d:personal/i:alice/p:repo/a:helper",
    "revision": "hr:repo-helper:9",
    "expected_binding": "12",
    "boundary": "next_turn",
    "evidence_ref": "experiment:comparison-4"
  }
}
```

- id correlates a response on this transport; maximum 64 ASCII characters.
- operation_key is the stable semantic mutation key; maximum 64 characters.
- Method and parameter schemas are closed and reject unknown fields.
- The host authenticates the operation namespace; it is not caller-selected.
- Plain references are bounded identifiers, not bearer capabilities.
- User-supplied deadlines only narrow an existing authorized budget.
- No fields for raw Process IDs, ActorKey, grants or authority epochs.
- A duplicate in-flight transport ID is a protocol error, not a new operation.
- Absent operation_key is allowed only for documented reads or explicitly
  non-replayable operations whose error semantics require fresh user intent.

Optional timeout_ms is an integer from 1 to the advertised request ceiling
(initially 30000). It bounds this request's wait and admission attempt, not the
lifetime of accepted queued work or an already admitted execution. The adapter
maps it to a monotonic RequestMeta.deadline, narrowed by the original caller's
remaining deadline; nested calls cannot restart that budget. If it expires
before acceptance, no new work is admitted. After acceptance, expiry only ends
the response wait: inspect original custody and use an explicit cancellation
operation if cancellation is intended. A timeout racing acceptance is uncertain.
Retries may change their response-wait ceiling but never extend an original
execution deadline, reset a budget or repeat an accepted mutation. Execution
limits remain explicit semantic operation parameters owned by that executor.

Mutation fingerprints cover the canonical method, resolved target and semantic
arguments, including expected revision and exact component references. They
exclude transport ID, connection, timeout_ms and presentation options. Resolution must not
turn a retry into a request against newly changed defaults.

The semantic namespace comes from the authenticated original caller and owner.
Transport reconnects cannot mint a new namespace for the same durable intent.
Same key and same fingerprint returns original custody; changed fingerprint
conflicts. A timeout never authorizes an automatic new key.

Existing aw.rpc v1 constructors that use their RPC ID as the original identity
keep that mapping exactly. The v2 adapter adds operation_key explicitly; it must
not re-key old Prepared/Applied/Unknown records during migration.

## 5. Receipts, errors and uncertainty

OperationReceipt is a projection of an original operation owner, not a new
universal journal. The implementation reuses the relevant Store/input/Job/
constructor receipt or introduces the minimal missing record in that owner.

```rust
pub struct OperationReceipt<T> {
    pub operation_ref: OperationRef,
    pub disposition: Disposition,
    pub execution_ref: Option<ExecutionRef>,
    pub value: Option<T>,
    pub observation: Option<ObservationCursor>,
}

pub enum Disposition {
    NotStarted, Accepted, Running, Applied, Failed, Cancelled, Unknown, Compacted,
}
```

Dispositions describe the original operation. Accepted means its original owner
accepted or queued the work; it does not imply an admitted Turn or successful
effect. The domain payload explains the actual owner state. Applied is a
committed factual operation result. Failed or Cancelled does not
prove that no external effect occurred. Unknown retains custody and blocks
automatic repetition. Compacted keeps the non-retryable identity even when its
payload is no longer available.

```json
{
  "v": 2,
  "id": "request-17",
  "result": {
    "operation_ref": "operation:harness-activation:71",
    "disposition": "applied",
    "value": {
      "binding": {
        "agent_ref": "/d:personal/i:alice/p:repo/a:helper",
        "desired_revision": "hr:repo-helper:9",
        "binding_revision": "13"
      },
      "readiness": "prepared"
    }
  }
}
```

Mutation result is the serialized OperationReceipt<T>; optional absent fields
are omitted. Read-only results return their domain value without a new receipt.
Readiness is a reconstructible observation, not an additional durable owner.
The currently active Turn still reports the revision captured at its admission.
The response must not call a desired future binding the effective active Turn.

Error response shape:

```json
{
  "v": 2,
  "id": "request-18",
  "error": {
    "code": "conflict",
    "message": "Harness binding changed",
    "effect_state": "not_started",
    "details": { "current_binding": "13" }
  }
}
```

| Proposed code | Meaning / client behavior |
|---|---|
| invalid_argument | Request failed bounded schema/semantic validation |
| conflict | Expected revision or same-key fingerprint differs; inspect current authorized state |
| permission_denied | Current authority cannot admit the request |
| not_found_or_inaccessible | Do not disclose existence through unauthorized lookup |
| unsupported | This host/version has no such capability |
| unavailable | Known capability cannot currently be realized |
| limit_exceeded | Explicit capacity, size, time or budget refusal |
| unknown_outcome | Possible effect or commit; inspect original custody |
| outcome_compacted | Original identity retained; payload unavailable; do not repeat |
| internal | Bounded public error; diagnostic correlation retained without secrets |

Transport-local timeout/disconnect is a client error with possible uncertainty,
not a fabricated server Failed receipt. operations.get performs authorized
lookup of original custody by operation_ref or original operation_key; a
missing record does not dispatch work. No general "mark successful" repair API.

Typed details may include an authorized current revision, a missing capability
or an original receipt reference. Never expose another caller's existence,
source locator, hidden grant or secret through conflicts and diagnostics.

## 6. Definitions, preparation and activation

HarnessDefinition is typed composition. The initial serializable fields are:
name, definition_schema, exact context/strategy/route/toolset references,
bounded settings/limits and dependency declarations. Human aliases are allowed
only during authoring; normalization resolves immutable component references.

HarnessRevisionRef identifies the immutable normalized composition and its
provenance. A reference is server-issued and opaque to external clients;
digest and dependency metadata are inspected through authorized operations.

ContextProgramRef and StrategyRef are proposed typed references. A context
reference resolves registered projection behavior; a strategy reference resolves
the original loop, a bounded pipeline, or a registered action/controller.
Neither is arbitrary embedded source code or a replacement TurnMachine.

PreparedHarness is native-only: retained exact implementation handles plus
validated metadata. Its pointers, closures and authority do not serialize.
The wire returns PreparationRef, revision, dependency summary, validation issues,
required capabilities and readiness. PreparationRef is an optional host-affine,
expiring acceleration hint, never authority or the durable revision identity.
An expired hint may be replaced by effect-free resolution of the same exact
revision from installed definitions. Missing code or required external startup
returns unavailable; never choose a different revision or silently run effects.
A durable revision remains inspectable independently.

The one native prepare operation returns OperationReceipt<Preparation>:

```rust
pub struct Preparation {
    pub summary: PreparationSummary,
    pub prepared: Option<PreparedHarness>,
}

impl Harnesses<'_> {
    pub async fn prepare(
        &self,
        caller: &CallContext,
        meta: RequestMeta,
        definition: HarnessDefinition,
    ) -> Result<OperationReceipt<Preparation>>;
}
```

PreparationSummary carries the immutable revision, optional preparation_ref,
validation/dependency summary and readiness. PreparedHarness exposes its exact
revision and summary and is not Serialize. The optional retained native closure
is present only when installed dependencies resolve without further effects.
The adapter projects the same receipt to OperationReceipt<PreparationSummary>,
preserving operation_ref, disposition and all factual metadata. No runtime
handles or authority serialize, and there is no second mandatory prepare call.
A lost runtime closure never erases the committed candidate's summary/result.

```json
{
  "v": 2,
  "id": "prepare-4",
  "method": "harnesses.prepare",
  "operation_key": "prepare-repo-helper-9",
  "params": {
    "definition": {
      "definition_schema": 1,
      "name": "repo-helper",
      "context": "context:repo-focus:3",
      "strategy": "strategy:default-agent:1",
      "route": "route:review:5",
      "toolset": "toolset:workspace:2",
      "limits": { "max_turns": 8 }
    }
  }
}
```

prepare persists the candidate in the host-bound existing Project/owner scope
and checks disclosure/use of every referenced component through canonical owner
policy. Read/fork visibility is separate from target Agent activation rights;
there is no new harness ACL engine.

prepare validates metadata and records an immutable candidate through its
existing Store mutation boundary. It does not run child discovery, install
packages, probe a model or acquire source effects. Optional executable
realization uses the explicitly configured extension/process/provider owner,
such as extensions.reload; it is not a mandatory extra harness lifecycle.
The candidate may remain valid-but-unavailable until realization succeeds.

activate requires agent_ref, exact revision, expected_binding and next_turn.
Optional preparation_ref accelerates exact closure resolution; it cannot change
the selected revision or any authorization check. Optional evidence_ref is
checked against independently configured promotion
policy; possession of a report ID is not permission. The authenticated caller,
target, candidate/baseline, expected binding, evaluator/policy revision and
evidence validity are rechecked at the binding commit.

HP-3 implements baseline current-authority/CAS activation and the typed eligibility
integration boundary. HP-7/8 supply optional report and promotion-policy behavior.
If the request or configured policy requires an absent validator, refuse with
unsupported (or unavailable for a configured but unusable dependency) before
commit. Never ignore evidence_ref or a policy requirement because its engine is
not installed. Baseline activation under a policy that requires no experiment
remains usable without those components.

Desired binding is durable. Applied/effective revision is recorded per admitted
Turn or pinned pipeline run. Admission rereads/rechecks the binding in the
original writer/admission transaction while retaining the prepared dependency
custody. A stale effect-free preparation may restart; an effect may not.

Same-name tool replacement cannot rely on retained Arc handles alone:
ToolBindingAuthority checks the exact live registration. Initial activation
defers per-Agent tool rebinding until every affected Turn, Job and parked
continuation releases its exact binding custody. Coexisting versions need
distinct still-live registration identities within the original authority.
Current revocation remains effective against both.

Rollback is activate of an older compatible revision using a new semantic key
and current expected_binding. It changes future selection only.

### New harness request/result contracts

| Method | Required request / optional fields | Result and mutation behavior |
|---|---|---|
| harnesses.get | revision | Authorized immutable normalized definition, component references and provenance; read |
| harnesses.fork | revision; optional proposed_name | Editable definition value returned to caller; authorized read, no persistent draft owner |
| harnesses.prepare | definition; operation_key in envelope | Native OperationReceipt<Preparation>, projected to wire OperationReceipt<PreparationSummary> as above; sole candidate-creation mutation |
| harnesses.diff | before, after exact revision refs | Bounded semantic component/settings/requirements difference; read |
| harnesses.binding | agent_ref | Desired HarnessBinding plus authorized active-execution/readiness projection; read |
| harnesses.activate | agent_ref, revision, expected_binding, boundary; optional preparation_ref, evidence_ref; operation_key | OperationReceipt<ActivationResult> with new binding and readiness; same-owner expected-binding CAS |

harnesses.list uses the ordinary bounded selector/page contract. There is no
additional define/save-draft operation: authors edit returned values locally,
then prepare an immutable candidate. Validation failures create no active
binding; candidate metadata is retained only through the original candidate
mutation contract. The plural harnesses facet is canonical across adapters.

## 7. Method inventory

Names below are target v2 names. Their request/result schemas and owner mappings
are foundation deliverables; implementation capabilities may arrive independently.
Existing v1 methods remain compatibility adapters until their removal is separately
authorized and verified.

| Facet | Proposed methods | Existing authority |
|---|---|---|
| Discovery | system.describe | Configured host capabilities |
| Agents | agents.list/get/create/edit/send/close | Store Agent state, ACL, original Input/creator/close composition |
| Inputs | inputs.list/add/edit/delete/send | Existing staged-input owner and actual Input admission |
| Jobs | jobs.list/get/read/send/ack/cancel | Original Agent router / JobManager / Process owner |
| Harnesses | harnesses.list/get/fork/prepare/diff/binding/activate | Composition records plus existing Agent admission |
| Extensions, qualified | extensions.list/get/reload/withdraw | Host-configured Process Extension and original registry/process owners |
| Observation | observations.read/watch | Authorized projections and original Store/Job observation |
| Events | events.read/subscribe/publish | Existing Event/Input/subscription owners |
| Context | context.inspect/prepare/apply | Existing ContextFlow and disclosure/provenance owners |
| Environments | sandboxes.list/get; checkpoints.get/create/restore | Only qualified existing resource constructors |
| Experiments | experiments.compare/list/get/report/cancel | Ordinary Agents/Jobs/pipelines and immutable experiment facts |
| Operations | operations.get | Read projection of original owner custody |
| Inference, optional | inference.capabilities/resolve | Configured Provider/executor selection; no execution |

This does not expand the compact ordinary model-facing tool vocabulary.
MCP export profiles expose selected typed tools; internal facet completeness
does not imply every operation is model-granted.

Agent creation chooses an authorized exact HarnessRevisionRef and configured
environment reference. Trusted provider/site/Sandbox construction context stays
outside caller parameters. Existing canonical execution references are retained.

extensions.reload/withdraw accept an already configured extension_ref and
expected_revision under their semantic operation key. Reload stages the selected
implementation through the original Process owner and publishes only validated
definitions; it is not an arbitrary executable/path upload API. Withdrawal
preserves live users and retained recovery dependencies or returns conflict.
Native privileged registry maintenance retains its separate quiescent boundary.

agents.send accepts agent_ref and text under one operation_key and returns
OperationReceipt<InputSubmission>. The SDK convenience agent.send maps directly
to this original Input submission owner, never a hidden inputs.add then
inputs.send sequence. Its domain payload identifies the original input and
current acceptance/queue state; execution_ref remains absent until the original
owner has actually allocated/admitted the corresponding execution. Inspect the
original operation or observe changes to obtain that reference. inputs.send
retains its distinct existing role of sending explicitly staged drafts.

No universal jobs.start bypass: an Agent turn starts through Input admission,
a pipeline through its owner, and finite tool work through original tool/Graph
admission. jobs.send keeps the existing directed-input/stdin semantics.

experiments.compare returns OperationReceipt<ExperimentStart>. Its value
contains experiment_ref; the standard receipt execution_ref is optional and
appears only when original execution custody exists. Acceptance does not imply
completion. experiments.get/report inspect the factual experiment state and
report, which binds exact baseline/candidate, initial state/environment selection,
evaluator and results. Promotion uses harnesses.activate; there is no separate
promotion scheduler or mutable success ledger.

## 8. Observation and cancellation

observations.read returns a bounded page and opaque continuation cursor.
observations.watch is a bounded wait for changes after a cursor and returns a
page, timeout, stale, lagged or unavailable result. It is not a new Event bus.

Cursors bind caller authorization generation, Store identity/restore generation,
selector, placement/projection generation and progress as required by their
original owner. Cursors carry no authority and do not create cross-Store order.
Revoke/regrant may stale a cursor even if the visible Agent revision is unchanged.

A lagged cursor reports the authorized resnapshot/rebaseline action; never
silently skip retained work or claim a gap-free stream after retention loss.
Finite Job byte offsets remain byte offsets, not Store positions.

Dropping/cancelling a watch detaches observation only. Cancelling execution
requires jobs.cancel, agents.close or the original operation-specific control.
A cancellation request is not a terminal fact. Preserve actual settlement,
partial output and unknown external effects.

Durable events.subscribe is distinct from ephemeral observation watching:
it uses the original subscription activation/generation and Input delivery
owner. A transport disconnect cannot silently delete durable subscriptions.

## 9. MCP adapter and version matrix

MCP uses its own JSON-RPC envelope. Map a selected tool's typed arguments to
the same service method; do not nest an aw.rpc envelope inside every tool.
Resource reads and task/control requests get fresh authenticated CallContext.

| Wire profile | Adapter obligation |
|---|---|
| MCP 2026-07-28 | Stateless core; per-request version/capabilities; version-correct result types |
| 2026 Tasks extension | io.modelcontextprotocol/tasks; server-directed task creation after client support |
| Earlier supported MCP | Explicit version decoder/encoder and capability matrix; no mixed task/session assumptions |
| No Tasks capability | Return a normal bounded submission receipt where the tool contract permits, or a version-correct capability refusal |

The 2026 Tasks extension supports tools/call through tasks/get, tasks/update
and tasks/cancel. Publish a task handle only after its durable lookup is
available. AW task mapping references original operation custody; it does not
invent another execution state machine. Task cancellation routes to the
original AW cancellation owner; uncertain underlying effects stay explicit.
Input responses route to the actual pending owner-bound request.

Application refusal is an appropriate typed tool error/result; malformed
protocol uses the selected MCP version's protocol errors. Do not allocate AW
codes in MCP-reserved ranges. Preserve complete structured results and bounded
human-readable summaries without serializing secrets.

An MCP task ID is neither a capability nor an idempotency key. Authentication
and current object authorization apply on every get/update/cancel. Task result
retention may expire independently of original replay tombstones; expiry does
not make an earlier effect executable again.

HP-3 includes the minimal working stdio/local owner adapter for the common
foundation operations. Its mature CLI entry point is:

```sh
aw mcp serve --stdio --mcp-profile authoring
```

This explicit server subcommand is additive to existing MCP-client inspection
commands and distinct from aw.rpc. It attaches to the configured original owner
through the supported owner path; ordinary aw startup does not start the server.
The authoring profile selects the supported programming tools while current
object authority still determines which calls can execute.

Shared schema/result/error/authorization mapping is complete in the foundation. HP-4 adds optional profiles, richer client ergonomics, negotiated
Tasks support and remote adapters without reopening that domain contract.
Remote HTTP export requires verified principal mapping; same-UID socket trust
is not Internet authentication. Ordinary aw startup opens no server.

Primary protocol sources, checked 2026-11-28:
[2026 base](https://modelcontextprotocol.io/specification/2026-07-28/basic),
[release](https://blog.modelcontextprotocol.io/posts/2026-07-28/),
[Tasks](https://tasks.extensions.modelcontextprotocol.io/specification/2026-07-28/tasks).
Protocol-specific tests must use the pinned official schema selected by the
implementation, not assumptions about the currently installed MCP SDK.

## 10. Process Extension successor protocol

Current aw-extensions ABI is 2. Do not silently extend its strict response or
manifest grammar and call that compatible. Target a separately selected
successor ABI (proposed 3); keep an ABI-2 adapter for existing tools/hooks/prompts.
Host configuration selects the ABI before launching; no effectful version probes.
Its complete declared manifest/callback/ctx schemas, scope and lifecycle rules
are foundation contracts. HP-6 implements and qualifies the expanded process
dispatcher and language SDK against them; HP-3 does not advertise that dynamic
support merely because the codec types exist.

Preserve bounded one-JSON-message-per-line framing and the existing
v/id/method/params request and v/id/ok/result response model. Add explicit typed
error and invocation fields only in the successor version. Use disjoint host
and child request-ID namespaces.

```json
{
  "v": 3,
  "id": "h:41",
  "method": "invoke",
  "params": {
    "definition": "extension:repo-tools/action:review@3",
    "invocation": "opaque-host-scope-7",
    "budget": { "remaining_ms": 12000, "nested_calls": 256 },
    "input": { "change_ref": "artifact:change-9" }
  }
}
```

```json
{
  "v": 3,
  "id": "c:8",
  "method": "ctx.state.get",
  "params": {
    "invocation": "opaque-host-scope-7",
    "key": "review-preferences"
  }
}
```

The host scope table binds connection generation, selected Extension,
Agent/Turn, exact contribution, permitted ports and budget. The child cannot
supply replacement identities or extend the scope's lifetime.
Host monotonic deadlines govern; reported remaining_ms is advisory to the child.

At most one top-level callback runs per process connection. During it, nested
ctx calls may occur under its scope: at most 8 cross-extension callback levels
and 256 nested calls per root deadline. Synchronous re-entry into the same
process is rejected before dispatch. Unsolicited or expired-scope calls fail.

Keep frame 256 KiB and manifest 128 KiB ceilings plus native role-specific
bounds. HP-1/2 establish count, size and recursion limits for every declared
manifest/callback family in its schema/capability contract; HP-6 implements and
qualifies enforcement. Do not defer these wire boundaries to feature promotion
or assume current 32-tool/3-hook limits describe the whole expanded registry.

describe is executable work under explicitly selected process custody.
A manifest names the host-selected Extension exactly, declares schema/code
metadata and registers definitions atomically. It never grants activation.

Cancellation or malformed/mismatched/incomplete responses poison the connection,
expire invocation scopes, suspend affected handlers and retain uncertain effect
custody. Process cleanup uses aw-process; it cannot report proven termination
without facts. Restart performs fresh describe/validation/publication and does
not replay an uncertain callback.

Hook Defer uses the existing continuation owner and bounded deferral contract.
A process-generated string cannot reconstruct a parked continuation after
process loss. Unsupported durable continuation recovery returns unavailable.

Initial dynamic contributions: tools, safe Agent hooks, actions, resources,
views, event types/handlers, semantic UI, settings and typed State.
Provider/ProviderHook/ProcessHook/Compactor/SourceDriver/SandboxDriver/
VaultProvider/TrustPolicy stay native-only. A separate process is not isolation;
untrusted executable authoring needs a qualified executor, not a manifest claim.

## 11. Source anchors and verification

- [Current IPC](../../../crates/aw-client/src/protocol.rs) and
  [client uncertainty](../../../crates/aw-client/src/lib.rs).
- [Headless v1](../../../src/headless.rs).
- [Process Extension ABI 2](../../../crates/aw-extensions/src/lib.rs).
- [Tool binding authority](../../../crates/aw-agent-core/src/tool.rs).
- [R6 dynamic contract](../dl200-unified-jobs-agent-events-2026-11-20/SPEC.md),
  especially the Process Extensions section.

This document changes no executable protocol. HP-1–3 must qualify the shared
contracts and real baseline SDK/MCP path. Each later component qualifies only
its added capability and affected boundaries. Reuse the selected V1/V2/V7
foundation scenarios and one real native composition example; compile/schema
checks support them rather than becoming a new fixture or evidence matrix.
Documentation checks do not qualify runtime behavior.
