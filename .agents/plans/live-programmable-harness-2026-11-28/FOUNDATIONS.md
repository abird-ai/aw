# Foundations — shared contracts, composition and integrated live runtime

Date: 2026-11-28 · Base: `a47492f45813f1e85f85e706e7f33292f0c3e6f0`
Status: **implementation plan; no product implementation claimed**
Scope: HP-1–HP-3 are identifiers local to this plan, not new global AW/DL-200 phases.
Parent: [README](README.md) · [Design](DESIGN.md) · [Queue](TODO.md)
Contracts: [API and protocols](API_AND_PROTOCOLS.md) · Later work: [Authoring](AUTHORING.md)

## 0. Implementation invariants and ownership

- One original Agent/TurnMachine/TurnJournal, Actor Input owner, Store, JobManager,
  Process lifetime and output/ACK owner. A Service composes them.
- Preserve accepted history, exact authority generations, paid receipts and unknown
  effects. A behavior revision never restores a revoked permission.
- Preserve `no_std/alloc`, stateless, JSONL minimal and durable feature boundaries.
  A durable harness feature may be unavailable in a smaller host; report that limitation.
- Use L5 only; leave ZeroGravity, reserved UX and frozen physical Sandbox outside
  these changes. SourceDriver/Vault replacement retains its reviewed boundary.
- Existing R6 work remains authoritative. Reconcile overlap with its owner before
  implementation; do not create parallel registry, State or dynamic-host efforts.
- Implement coherent slices below as useful deliverables, not tiny ceremonial
  checkpoints. Verify behavior at existing owner boundaries; no evidence dumps
  or separate report artifact for every helper or accessor.
- All new type/method/schema names below are proposed. No schema version number
  is reserved; determine the actual migration parent at implementation HEAD.

| Existing owner/path | Extraction or extension |
|---|---|
| `src/main_full.rs::Host` | Move domain composition; retain settings presentation, CLI parsing and renderer state |
| `src/actor_owner.rs::Service` | Reuse Input admission, writer/ACL fences, cancellation and original run settlement |
| `src/agent_lifecycle.rs`, `src/agent_jobs.rs` | Reuse create/close/activation, exact Router and Job ownership |
| `src/headless.rs` | Decode/encode typed operations; remove extracted business logic |
| `crates/aw-host` | Public `service::Service`, domain facets and trusted composition |
| `crates/aw-client` | Typed attached operations; existing owner IPC and uncertainty rules |
| `aw-agent-core/{agent,registry,publication,hooks,runtime}.rs` | Resolve/retain behavior and install at original admission boundaries |
| `aw-store/{pipelines,routing}.rs` and current Store APIs | Retain component revisions, authority, effects and recovery owners |
| `src/context_flow.rs`, Store context/history APIs | Canonical authorized context projections; no history edits |
| `aw-tools-adapter::JobRuntime`, `aw-process` | Existing finite work and physical execution; no new scheduler |

### Foundation coverage and independent implementation

HP-1–3 establish the shared substrate for the whole declared product. This includes
the public API families, component integration points and protocol semantics now
known from HP-4–9. Their contracts cannot be postponed until the first component
happens to need them.

Distinguish three outcomes:

| Outcome | Required by foundation exit |
|---|---|
| Shared contract | Compiling native values/signatures, bounded wire schemas, owner/capability mapping and compatibility rules for every already-planned family |
| Common runtime | Working registration, preparation, activation, current authority, receipt/recovery and observation paths through existing owners |
| Consumer proof | Real default/custom composition through embedded Rust, attached Rust and minimal MCP; independent component consumer through public boundaries |

Later phases implement richer clients, context/strategies, dynamic execution,
experiments, improvement and inference adapters. A schema alone is not an
implemented capability. Advertise only executable supported operations; a known
but unimplemented family is refused before effects and does not start a service.
Use the existing dispatch/capability path rather than an empty manager per family.

The contract set covers declared work, not hypothetical future plug-ins. Do not add
a universal component trait, arbitrary JSON execution interface or `Any` service
locator. Reuse existing typed owner contracts; add a narrow missing interface only
where one of the planned consumers needs it. Private algorithms, layouts and
component schemas remain independently evolvable under the compatibility rules.

### Implementation style and performance

- Prefer concrete structs/enums and borrowed typed inputs; use generics where
  useful for ordinary reusable Rust code. Runtime dispatch belongs at actual
  provider/plugin/transport substitution boundaries, not every internal method.
- Reuse semantic validation and owner checks across SDK/transports. Avoid
  parallel validators, generic meta-frameworks, per-feature managers and new
  proof-wrapper types when existing domain types and guards already express it.
- Resolve names, normalize/hash definitions and prepare dependency closures once
  outside the execution loop. Retain immutable handles; do not clone/serialize a
  whole harness, hash definitions or query Store on every token/tool event.
- Current live authority checks remain at their existing effect boundaries.
  Do not cache away revocation or add a new global lock to simplify composition.
- Prefer the clean simple implementation; optimize a measured or evident hot
  path when it remains maintainable. Reuse focused existing tests and a handful
  of end-to-end scenarios below; no fixture matrix for its own sake.

## HP-1 — Host and public contracts

### Entry and exit

Entry: owner map and source baseline revalidated; extraction paths claimed.
Exit: the complete currently planned public contract set is expressed through shared
typed requests/results, bounded schemas and documented owner mapping. An independent
Rust host and attached client can create/inspect/send/cancel through the same
original owners. Other planned families have usable contract types and explicit
capability behavior; their future algorithms need not exist yet. The shipped host
delegates the implemented slice to Service without a second compatibility path.

### HP-1A — Complete shared contract set

1. Add `aw_host::service::Service` around owned/shared handles to existing
   authorities. Retain the exact `JobRuntime` returned by
   `LocalHost::configure_with_runtime`; do not rediscover or construct another.
2. Establish every already-planned public family from API_AND_PROTOCOLS.md:
   discovery, Agents, Inputs, Jobs, harnesses, extensions, observation, Events,
   context, qualified environments, experiments, operation lookup and optional
   inference. Keep their native/wire representations, mutation identity, authority
   owner and compatibility behavior consistent. Implement the baseline owners
   first; later engines need no empty manager, background service or success stub.
3. Introduce `CallContext` for authenticated invocation. Its constructor belongs
   to the trusted embedding/transport adapter; it is not deserializable.
   Caller, Project scope, executor authority and idempotency namespace come from
   the host. Wire parameters cannot mint them.
4. Preserve existing `aw_policy::OperationContext` as its policy query descriptor.
   It is not the authenticated `CallContext`; do not change its public meaning.
5. Define typed requests/results, `CapabilitySet`, `ExecutionRef` and
   `OperationReceipt<T>` using [API_AND_PROTOCOLS](API_AND_PROTOCOLS.md).
   Include owner-local observation, cancellation, deadlines/causal-budget context,
   bounded pagination, receipt projection and refusal/conflict/unavailable/unknown
   semantics now. Later components reuse these contracts instead of adding another
   envelope, error model, retry namespace or execution owner.
6. Share pure contract types only when two real consumers require them.
   A small `aw-control` crate is justified if needed to keep `aw-client`
   independent of `aw-host` and the Agent/Store implementation. It contains
   data/validation/schema types, never authentication, executor or storage code.

Dependency direction: clients/transport adapters → contracts; aw-host → contracts
and original implementation crates; original core/kernel never → Service.
No HTTP/MCP/SQLite/TS dependencies enter the portable kernel. Keep the lower
`AgentBuilder` API usable without the full host Service. Re-export the embedding
Service and shared request types from `aw_host`; re-export the same pure request
types with Client from `aw_client`. SDK examples do not depend on the CLI root
library, and these conveniences do not enable unrelated product features.

### HP-1B — One end-to-end Agent path

- Extract exact current-Project Agent lookup/create from existing lifecycle
  composition, preserving canonical Agent identity and existing immutable
  Agent→Sandbox binding. Trusted execution configuration is supplied separately
  from caller fields.
- Submit text through the existing Input owner; acceptance means accepted/queued,
  not successful inference. Return the original Input/execution references.
- Observe through current authorized projections; cancellation reaches the
  original active Turn token/control owner. Closing composes existing finite-Job
  quiescence and Store lifecycle settlement; it is not synonymous with cancel.
- Keep blocking database sections bounded. Do not hold Store transactions,
  publication guards or per-Agent admission locks across network/model awaits.
- Use provider selections authorized for the caller. Do not expose trusted
  `Agent::set_model` as a remote grant mutation: its local convenience semantics
  can extend a default account's allowed-model set.

### HP-1C — Cut over adapters and lifecycle composition

| Surface | Required adapter behavior |
|---|---|
| Embedded Rust | Explicit Service construction; no listener, model call or resource scan merely on import |
| Existing local owner IPC | Preserve peer/instance checks; map typed requests to Service |
| Existing aw.rpc stdio | Translate legacy method names/JSON at the edge; preserve existing wire compatibility |
| CLI/full host | Call Service for extracted behavior; keep unrelated domains unchanged until their slice |
| Minimal/stateless | Expose supported operations; never initialize durable services to imitate parity |

A wire request ID correlates a connection response. An explicit semantic
`operation_key` identifies a mutation in a host-bound caller/scope namespace.
The same key and canonical input finds the original receipt; different input
conflicts. A disconnect never causes an automatic new effectful request.
Read-only status lookup uses the original owner; no global retry ledger.

### HP-1D — Establish adapters and protocol contracts early

- Put shared native/wire DTOs and semantic validation in the actual common module
  or dependency-light crate. Compile these contracts for the declared optional
  families without loading their runtime implementations.
- Establish the control-v2 envelope, method/result mapping, bounded limits,
  operation-key semantics, capability discovery and version/legacy adapters.
  The baseline attached path is real here; harness methods become executable
  as HP-2/3 provide their owners.
- Establish MCP tool/resource/observation mappings and selected-version rules
  against the same values. HP-3 delivers the minimal live adapter; HP-4 expands
  profiles, client ergonomics and optional transports/Tasks.
- Establish Process Extension ABI-3 request/result, scoped invocation, deadline,
  cancellation, poison and compatibility contracts. Existing ABI-2 behavior is
  retained; expanded dynamic execution and its author SDK remain HP-6.
- Define how optional evaluator, promotion-policy and inference capability
  requests reuse the same caller, budget, receipt and observation semantics.
  HP-7–9 implement these domain behaviors; they do not design their host identity
  or transport model for the first time.
- Keep a single source of canonical names/schemas. Reuse normal type/schema
  generation where helpful, with ordinary explicit method matching; no new
  protocol-description language or code-generation framework.

### HP-1 verification

- One embedded/attached parity scenario: create/send/observe/cancel/close refer
  to the same Agent, original receipts and retained JobRuntime; legacy adapters
  preserve their wire behavior.
- One lost-reply/admission-race scenario: lookup preserves the original outcome,
  changed-input key reuse conflicts, and current access revocation prevents a
  new effect. Reuse the existing Input/authority test seams.
- Run relevant existing lifecycle tests and changed-package checks. Inspect
  feature dependencies for accidental Store/network/UI inclusion.
- DTO round trips may support these tests; they do not replace owner behavior.

## HP-2 — Composition and component contracts

### Entry and exit

Entry: HP-1 operation context/identity contract and reusable owner access exist.
Exit: typed composition and extension boundaries cover the already-planned
component families, including their configuration, state, effect and lifetime
contracts. One default harness and an independently authored variant can be
prepared, inspected and durably selected for new work; exact composition is
recoverable without copying existing pipeline/routing definitions. Later phases
supply implementations behind these boundaries rather than inventing their
host integration, identity or state model.
HP-2 may create new Agents with an exact revision against a static, already
installed registration set frozen for that instance's lifetime. Online changes
to that set and switching a running Agent remain HP-3 until atomic composition,
admission and implementation-lifetime gates pass.

### HP-2A — Typed composition and references

```rust
// Proposed semantic shape; exact field types follow existing domain owners.
struct HarnessDefinition {
    name: String,
    instructions: InstructionComposition,
    context: ContextProgramRef,
    strategy: StrategyRef,
    route: ModelRouteRef,
    tools: ToolsetRef,
    limits: HarnessLimits,
    components: Vec<ComponentRef>,
}
struct HarnessRevisionRef(RevisionId); // opaque outside its issuing owner
// Digest and provenance belong to authorized revision metadata, not wire ref shape.
struct HarnessBinding {
    agent: AgentRef,
    desired_revision: HarnessRevisionRef,
    binding_revision: BindingRevision,
}
```

- Distinguish logical definition identity/name from immutable revision identity.
  Normalize bounded typed fields and explicit composition order before hashing.
- Reject unknown required contracts, duplicate slots, cycles, missing references,
  ambiguous replacement and incompatible source/effect assumptions.
- Exact component references include their owner namespace, registration/revision
  identity and schema/implementation contract. A root publication generation
  provides snapshot coherence; it is not every component's version.
- Reuse pipeline/routing immutable records through references. Initial strategy
  variants are default loop, existing workflow, registered controller/action.
  Inner round-policy replacement is a later narrow port, not a generic callback
  that acquires `&mut Agent`.
- ContextProgram initially selects registered projections/compaction and owned
  instruction contributions. Prompt hooks remain bounded system-prefix edits;
  they cannot reorder or replace accepted messages.
- Installation, binding and grants remain separate. Importing a shared definition
  cannot install executable code, choose credentials, widen tool grants or alter
  an environment merely because its metadata asks for those capabilities.
- Persist owner/provenance and necessary disclosure references. A definition that
  embeds private instructions needs the same scoped read/export discipline as
  other protected content; a digest is not permission to retrieve it.

### HP-2B — Prepare, explain and realize

`harnesses.prepare` validates bounded data against one coherent registry snapshot
(or the frozen installed set before HP-3) and records the immutable candidate
using its semantic operation key. It is a
metadata mutation; executable realization remains a separate operation.
Resolve names to exact refs; report composition, effect requirements, inherited
constraints, unsupported capabilities and selected implementations.

Separate two operations:

1. **Metadata preparation:** no executable discovery, external process launch,
   network probing, paid inference or source mutation.
2. **Explicit realization when needed:** existing Process/source/operation owner
   performs admitted effects and records its own receipt; the preparation links
   that evidence. A failed realization leaves the active binding unchanged.

The native receipt contains `Preparation`: its `PreparationSummary` and an
optional `PreparedHarness` retaining the resolved immutable closure and handles.
A valid-but-unavailable candidate still has its factual summary. Wire projection
returns that same receipt identity with `PreparationSummary`, including any
optional `PreparationRef`; pointers, native trait objects and authority do not
serialize. There is one prepare operation, not a mandatory second lifecycle.
Preparation scope, expiration and caller access are checked when subsequently
used. PreparationRef is only an optional acceleration hint. Expiry permits
effect-free resolution of the same exact immutable revision from installed
components; missing code or necessary startup returns unavailable. Neither expiry
nor a prior successful preparation permits fallback or bypasses admission checks.

### HP-2C — Minimal Store representation

Conceptual records, to refine against current Store transactions before coding:

| Record | Required content/relationship |
|---|---|
| Harness identity | Scoped owner, logical ID/name and lifecycle metadata |
| Harness revision | Immutable normalized composition/digest, format version, exact component refs and provenance |
| Agent harness binding | Canonical Agent FK, desired revision FK, monotonic BindingRevision |
| Execution association | Exact admitted revision on the existing Turn/run receipt or owner-local FK association |
| Preparation metadata, only if recovery needs it | Reproducible exact dependencies and linked realization receipts; never native handles |

Use unique identity/digest and foreign-key constraints where appropriate. Existing
pipeline/routing rows remain authoritative. Do not introduce a universal component
revision table, a second operation log or a second run state machine.
The execution association is written with its original admission/intent; an
eventual observer backfill cannot establish what executed.

Canonical Agent scope is the public boundary. Existing Session-oriented
pipeline/routing records are resolved through verified Agent→Session/branch
ownership; possession of a backing Session ID is not authority over every Agent.
A pipeline/controller receipt pins its exact composition for all admitted work.
Templates influence future creation; editing a template does not retarget Agents.

### HP-2D — Migration and default adoption

1. Read actual schema at implementation HEAD and design one additive migration
   under the existing exclusive owner/migration path.
2. Create/validate new records and constraints atomically; bump schema only after
   completeness checks. Do not delete owner locks or bypass restore quarantine.
3. Existing Agents without a harness binding keep their existing behavior.
   Represent that absence explicitly; do not fabricate exact historical revisions.
4. Capture a default composition from the current trusted host configuration on
   explicit adoption/new creation. Unsupported ambient configuration is reported,
   not silently converted to a misleading reproducible definition.
5. Keep historical execution receipts unchanged. Record harness identity on new
   admissions only; compatibility readers distinguish legacy unknown from exact.
6. Restart loads definitions inertly. Resolve availability and current authority
   before admitting model/tool work; no automatic package execution at Store open.
7. Older binaries obey normal schema compatibility refusal. JSONL does not gain a
   silent new record format; capability-gate unsupported durable harness control.

### HP-2E — Complete the known component boundaries

Use existing typed leaves and host ports, including R6 Action/View/EventHandler
and StateKey<T>/SettingKey<T>/EventKey<T> where applicable. Define the missing
integration shape before independent component implementation begins. These are
contract groups, not a required trait/crate/registry per row; canonical signatures
and portable representations remain in API_AND_PROTOCOLS.md.

| Component group | Foundation establishes | Later implementation can vary |
|---|---|---|
| Instructions/context | Ordered contributions, authorized input views, bounded projection/output, disclosure/provenance and separately admitted paid work | Selection, summarization recipes and context heuristics |
| Strategies/actions | Original loop/pipeline/controller references, scoped calls to original work owners, bounded stop/delegate/join and cancellation | Planning, review, repair and specialist orchestration |
| Routing/inference | Existing Provider/executor selection, exact-or-alias provenance, optional capability resolution and invocation receipt association | Selection algorithms and richer provider/ZeroGravity adapters |
| Tools/hooks/resources | Existing typed signatures, schema/implementation identity, allowed stages, effect and continuation ownership | New useful implementations within those contracts |
| Events/observation | Existing owner-local order, bounded projections/cursors, subscription versus watch and current disclosure checks | Derived views and component-specific signals |
| State/settings | Logical owner/key/scope, schema and semantic compatibility, authorized read/write/CAS and explicit migration boundary | Component state representation and algorithms |
| Views/semantic UI | Registered typed projection/action contracts, bounded data and explicit owner-authorized actions | Presentation content and interactions |
| Evaluation | Exact candidate/baseline/case/evaluator refs, authorized scorer inputs, typed measurements, coverage and immutable report relationships | Scorers, case sets and comparison algorithms |
| Promotion | Activation evidence reference, policy ownership, permitted edit/authority boundary and short commit-time fences | User policies and ordinary improvement controllers |
| Dynamic extension bridge | Selected manifest-to-leaf mapping, scoped typed ctx ports, invocation lifetime/budgets and native-only exclusions | Process SDK/runtime conveniences and additional admitted dynamic families |
| Environment/source access | Existing qualified source/checkpoint/driver handles, effect receipts, disclosure and maintenance constraints | Separately qualified providers; no foundation expansion of physical containment |

For each actual typed interface, establish its input/output meaning, lifetime,
allowed effects, current-authority boundary, cancellation and state ownership.
Keep optional requirements discoverable and versioned with their component.
Native interfaces use existing concrete types, borrowing and retained handles;
portable definitions reference exact implementations rather than serializing code.

The root composes the selected contribution families atomically. An absent optional
family contributes no running service or hook to the default path. Foundation
tests exercise one real native context/strategy variation through this composition;
they do not demand a runtime implementation of every future scorer, provider or
UI contributor. The existing native-only registration exclusions remain intact.

Persist only metadata needed for executable foundation behavior. Experiment
dispatch/report storage and other later domain migrations remain local to their
implementation phase; establishing their shared IDs/schema does not preallocate
tables or reserve migration numbers.

### HP-2 verification

- One authoring/adoption scenario covers default + independent variant,
  normalized revision identity, invalid references, no-execution preparation and exact
  first-execution association; it also denies cross-Agent backing-ID access.
- One populated migration/restart scenario preserves legacy Agents/history,
  restores exact definitions inertly, and reports unavailable dependencies
  without substituting newer code or replaying effects.
- Reuse existing disclosure checks for protected definition read/export and
  existing intent tests to verify the association precedes effects.
- Keep canonicalization/schema unit tests only where they guard real ambiguity
  or compatibility; do not generate a parallel checklist of every field.

## HP-3 — Live runtime and integrated foundation

### Entry and exit

Entry: HP-2 definitions and bindings exist; required R6 leaf registry contracts
and ownership are reconciled. Scope atomic-root work with the R6 registry owner.
Exit: the common foundation is executable through embedded Rust, attached Rust
and a minimal explicitly started MCP server. R1 finishes coherently while a
committed R2 becomes the next eligible Turn's behavior; current revocation still
wins; restart retains exact intent and unavailable implementations are explicit.
The shared contracts and integration points for later components are usable from
public libraries without root-private access or changes to core ownership.

### HP-3A — One ExtensionRegistry root

- Complete the planned extension-scoped staging over existing typed leaves.
  Build/validate a full successor before publishing any member.
- Reuse `registry::StagedRegistry` and `publication::Publisher`; do not add
  another revision clock or generic manager.
- A failed multi-leaf registration publishes nothing. CAS against the root used
  to prepare prevents mixed-generation validation/publication.
- Registration-local identities and native implementation handles survive
  unrelated root publication. Changed dependencies invalidate affected prepared
  closures; broad root-generation equality is not an authorization predicate.
- Preserve exact implementation custody through outstanding work/recovery.
  Native declarations stay trusted code. The dynamic manifest's typed mapping,
  bounds and publication/lifetime contract are established in HP-2; the expanded
  process execution and TS/JS implementation remains HP-6.

### HP-3B — Activation and admission linearization

Activation changes `HarnessBinding.desired_revision` using mandatory expected
`BindingRevision`, stable operation key, authorized canonical Agent and exact
candidate revision/preparation. It does not mutate the current Turn.

1. Resolve candidate handles and validate compatibility outside Store transactions.
2. Enter the existing per-Agent admission/writer serialization boundary.
3. Recheck current caller permission, Agent lifecycle, expected binding and
   required dependency custody; commit binding CAS plus original operation result.
4. Release the boundary. Return committed desired revision and binding revision;
   readiness is an explicitly reconstructible observation, not another owner.

At actual Turn admission:

1. Resolve the run's explicit pinned constraint, otherwise the current Agent
   binding. Ordinary queued Inputs choose here; pinned Inputs must match or fail.
2. Prepare the exact immutable closure before entering the short final boundary.
3. In the original admission transaction, recheck binding/run constraint and
   existing writer/ACL/source conditions, and associate the exact harness with
   the original admitted Turn/intent. Refuse stale preparation.
4. Install that exact closure under the original Agent owner before any effect.
   A proposed internal `Agent::install_prepared_harness` may exchange validated
   options/extensions/tool snapshots; preserve Agent, conversation, journal,
   execution identity and the same ToolBindingAuthority producer.
5. Run the original loop. No provider/tool effect starts before installation and
   original durable intent both succeed. Failure settles or holds the original
   admitted work according to factual effect evidence; never silently requeue.

No transaction spans SQLite, remote services, Process startup and runtime memory.
After a committed binding, runtime memory is reconstructed from that binding.
No invocation may pair a new binding with old handles. No silent fallback to R1.
An admitted pipeline/controller constraint takes precedence over mutable defaults.

### HP-3C — Tool and continuation boundaries

- Initial same-name tool replacement waits for the affected Agent's Turn and
  dependent Job/continuation custody to quiesce. Idle prompt UI alone is not proof.
- Stage a complete registry/schema successor and republish through the existing
  ToolBindingAuthority. Never rewrite a schema after the model has observed it
  while dispatching its call to a different implementation.
- Continue current authority/source/account checks before their effects/commits.
  A retained old code revision cannot retain a revoked grant.
- Current Runtime Defer pins the shared HookPipeline version; a registry change
  invalidates its one-shot continuation. Preserve that behavior initially.
- Either drain affected deferred work before shared-chain mutation, or implement
  retained per-revision chains with explicit continuation custody and tests.
  Do not claim old continuations survive until the latter contract exists.

### HP-3D — State compatibility, retirement and recovery

State declares owner/namespace, schema and semantic contract plus sharing scope:
per Agent, per run/revision, or an explicitly named shared service.
Concurrent revisions may share state only under a compatible read/write contract;
schema shape alone is insufficient. Otherwise drain or use isolated state.
Experiments receive isolated state unless shared-state effects are explicitly
part of the admitted comparison. State CAS remains with its existing owner.

State migration is separately admitted and journaled by its owner. Binding
rollback selects earlier behavior for future work; it does not undo a migration.
Retirement reports exact blockers: active Turns/Jobs, continuations, retained
state/recovery/checkpoints, or implementation users. Release only when the
respective original owner proves dependencies gone; no time-only forced unload.

SourceDriver/Vault maintenance stays exclusive and semantic: retained binding,
anchor/materialization/recovery checks, no live users, currently compatible
replacement of the same concrete native type. Harness activation cannot widen
this into general concurrent source replacement or rebind an Agent's Sandbox.

| Failure point | Required outcome |
|---|---|
| Validation/realization failure | Prior binding remains selected; factual realization receipt retained |
| Stale binding CAS | Conflict; no partial activation |
| Binding committed; reply lost | Lookup original operation/binding; do not create another mutation |
| Binding committed; process exits | Load exact desired ref; hold if implementation unavailable |
| Turn admitted; installation fails | Original run settles/holds before effect according to evidence |
| Extension/process disappears after dispatch | Preserve factual result or Unknown; original recovery owner |
| Revocation after behavior pin | Existing required authority fence refuses/stops further effects |
| Previous revision selected again | New future binding revision; history/effects remain unchanged |

### HP-3E — Integrate public consumers before releasing the foundation

Move the baseline SDK/MCP implementation into this phase. Using public crates,
demonstrate the same create/send/inspect/cancel, prepare/diff/binding/activate and
original-operation lookup path through:

1. Embedded Rust with typed handles and a borrowed host-minted CallContext.
2. Attached Rust over the explicit local control adapter.
3. A minimal explicitly started MCP server with discovery, the supported baseline
   operations and a small harness-authoring profile.

The adapters use the same domain validation and owners. Thin transport-specific
framing/authentication code is expected; duplicated activation, state, scheduling
or retry logic is not. No new listener starts during ordinary embedded/product
construction.

Exercise an independently authored native component from a public consumer,
rather than importing root-private modules. It must compose with the default
harness, use the declared state/context/owner boundaries and participate in the
same next-admission activation. This demonstrates that later component authors
can work without another core extraction.

Known optional-family contracts and ABI mapping must compile and validate their
declared shapes at this point. Runtime discovery exposes only installed,
implemented support. The minimal MCP surface need not implement remote HTTP
authentication, every export profile or optional Tasks execution; those expand in
HP-4. HP-6 still owns expanded process execution and the TS/JS authoring SDK.

### HP-3 verification

- **Live activation:** R1 settles coherently, R2 serves the next eligible Turn,
  queued Inputs follow the documented boundary, pinned runs retain their pin,
  and concurrent CAS/close/revocation obey the original authority owner.
- **Interrupted activation:** lose replies/crash at binding commit and Turn intent;
  reconstruct exact identities or hold, with no duplicate effects or fallback.
- **Registry/state lifetime:** one composed fixture covers partial publication
  refusal, unaffected handles, a parked Defer, old writers and retained recovery
  dependencies. Exercise real replacement/retirement behavior.
- **Minimality/performance:** relevant existing feature builds remain clean;
  inspect the changed admission/hook hot path for new per-token work. Benchmark
  only a concrete performance uncertainty, using an existing workload if possible.
- Use existing authority/journal/Job/Process seams and ordinary readable assertions.
  Add targeted regressions for uncovered risks, not a combinatorial fixture matrix.
- These checks supply V1/V2/V7 plus the existing native composition scenario;
  they are not a new verification matrix. Component-specific runtime checks
  remain with the implementation that introduces the corresponding behavior.

### Foundation exit gate and subsequent independence

HP-1–3 finish together when all of the following are true:

1. Every already-planned public family has a shared compiled contract, bounded
   schema, original owner/authority mapping and capability/version behavior.
   Existing names and semantics are reusable by a later component without
   redesigning request identity, receipts or cancellation.
2. The actual default/custom harness works through public embedded Rust, attached
   Rust and minimal MCP, including preparation, next-admission activation,
   observation and original-owner cancellation.
3. One independently authored native component composes through public typed
   ports. State/provenance/dependency lifetime and exact implementation custody
   participate in the same activation and recovery rules.
4. The integrated V1/V2/V7 checks and native composition scenario pass at the
   changed boundaries. Absent future capabilities refuse clearly, while ordinary
   startup and portable feature cuts remain intact.

After this gate, HP-4/5/6/7/9 can evolve independently against the established
foundation. HP-8 needs HP-7 comparison/report behavior; a component selecting
another optional capability declares that concrete dependency. HP-10 qualifies
the chosen shipping set incrementally rather than blocking every lane on all
future features.

Independent evolution permits additive capabilities, component-private schemas
and deliberate versioned changes. It does not promise a permanent Rust ABI.
A new requirement that changes shared ownership or protocol meaning is handled
as an explicit compatibility change, not silently embedded in one component.
