# Learning, inference integration and delivery — HP-7 through HP-10

Author: Prasanna Venkateswaran Loganathar
Co-authors: GPT-6 Astra Pro
Owner: @chatgpt-aw-harness-design (plan coordination)
Date: 2026-11-28
Base: `a47492f45813f1e85f85e706e7f33292f0c3e6f0`
Status: implementation specification; new contracts below are proposed, not shipped.
Parent: [design](DESIGN.md) · [queue](TODO.md) · [canonical API/protocol](API_AND_PROTOCOLS.md)

## Scope and shared rules

These are plan-local work packages, not additional numbered AW product phases. HP-1–3
establish the full shared contract map, typed component ports, original lifecycle/state
semantics and an executable baseline through Rust and MCP. That foundation includes basic
observation and ordinary Provider execution, plus the declared experiment, evaluator,
promotion and richer-inference data/extension contracts.

HP-7 implements comparable experiments and evaluation through those contracts. HP-8 adds
the improvement controller and deterministic promotion-policy implementation. HP-9 supplies
richer AW-side inference adapters. HP-10 qualifies each selected capability incrementally.
HP-4/5/6/7/9 can evolve independently after the foundation; a chosen component may add a
specific capability dependency. HP-8 genuinely requires HP-7 reports. Basic programmability
and SDK/MCP use do not wait for these optional lanes.

Wire adapters follow API_AND_PROTOCOLS.md; native callers use the same typed requests
and results with a borrowed host-minted `CallContext`, without a wire envelope or JSON.
Wire callers never supply an actor, execution site or grant. `operation_key` is stable
semantic mutation identity; `id` is transport correlation. Native `PreparedHarness`
retains handles; portable clients receive `PreparationRef`. Existing
Agent/Turn/Job/pipeline receipts remain the execution and recovery truth. Experiment
metadata and report records do not introduce a scheduler or effect ledger.

The field semantics below feed the foundation's canonical contract types; they are not
first designed when each later engine is implemented. Compile/schema availability does
not advertise runtime support. Until a selected component is installed, capability dispatch
returns unsupported before effects; no empty experiment manager or dummy successful handler
is needed. Components add implementation and advertise its exact supported capabilities.

Prefer small reusable Rust modules and existing value types. The contract names below
identify data needs, not a requirement for a trait, crate or subsystem per concept. Retain
typed handles outside hot paths; use borrowed data and static dispatch where practical. Add
optional evaluation only when requested, with no baseline startup or per-token overhead.
Optimize measured hot areas; keep cold control paths simple.

| Source foundation inspected at Base | Consequence for implementation |
|---|---|
| [Provider::invoke and InvocationContext](../../../crates/aw-agent-core/src/provider.rs) | Keep actual journaled invocation identity and cancellation limitations. |
| [Usage and CompletionMeta](../../../crates/aw-agent-core/src/completion.rs), [UsageSummary](../../../crates/aw-agent-core/src/conversation.rs) | Missing token totals remain optional; preserve calls_without_complete_usage. |
| [pipeline revisions/runs/directives/receipts](../../../crates/aw-store/src/pipelines.rs) | Reuse durable step attempts and UnknownExternal; do not replace them with batch Graph replay. |
| [pipeline execution adapter](../../../src/main_full.rs) | Extract existing pipeline_next/settlement composition through HP-1's service. |
| [JobRuntime](../../../crates/aw-tools-adapter/src/job_runtime.rs) | Finite cancellation and close use original hosts/tokens and factual quiescence. |
| [event source](../../../crates/aw-store/src/event_source.rs), [Agent Job events](../../../crates/aw-store/src/agent_job_events.rs) | Reuse source-owned revisions, deliveries and current authority checks. |
| [aw-inference](../../../crates/aw-inference/src/lib.rs) | Keep provider credentials with their configured executor and existing private grant/receipt ownership. |
| [workspace features](../../../Cargo.toml), [host features](../../../crates/aw-host/Cargo.toml) | Preserve explicit full/minimal/stateless and optional durable/source boundaries. |

The frozen physical Sandbox implementation and reserved UX plan remain outside these work
packages. A separate process alone does not establish an untrusted-code boundary.
Evaluations use qualified executors or trusted/native/declarative code.

## HP-7 — Experiment execution and evaluation

### Prerequisites and owned areas

Required for integrated delivery: the completed HP-1–3 foundation, including exact
revision/provenance, owner-local observation, evaluator/report contracts, causal budget and
state-isolation semantics. Engine development may proceed against those settled interfaces
while foundation implementation finishes.

HP-4 transport expansion, HP-5 advanced algorithms, HP-6 dynamic runtime and HP-9 richer
inference are not blanket dependencies. The initial comparison can use foundation-native
harnesses, a deterministic native scorer and an ordinary provider. A selected Process
Extension evaluator or richer candidate declares only its actual capability dependency.

Primary areas: `aw-host` experiment implementations; `aw-store` bounded experiment
metadata; existing pipeline, Agent and Job adapters; client projections and examples.
Consume the foundation's shared semantic-boundary observations and receipt fields; keep
experiment-specific aggregation in this component. Reuse `src/events.rs`, Store Events and
history/disclosure projections, without moving dispatch or scoring into presentation code.

### Foundation-established domain contracts

HP-1 owns operation/observation DTOs and HP-2 establishes the experiment, scoring and
provenance shapes. HP-7 implements dispatch, scoring, aggregation and storage adapters
against them. Reuse the canonical definitions rather than creating phase-local copies.

- `ObservationQuery`: authorized subject refs within one original owner/order domain,
  bounded field selection, time/receipt bounds and optional opaque cursor. Default
  fields are factual metadata.
- `ObservationPage`: snapshots/facts, owner-local source position, next cursor and
  explicit completeness/gap/expiry information. Cursor scope includes the original
  owner, query and current authority dependency; restoration cannot reinterpret an
  old cursor. A combined overview labels separate owner snapshots and coverage; it
  cannot imply an atomic snapshot or global order. A future multi-owner watch would
  need explicit per-owner positions, not a new universal cursor.
- `ExperimentSpec`: baseline/candidate HarnessRevision refs, case-set revision,
  evaluator revision, environment plan, initial state references, model constraints,
  budget, bounded parallelism, repetition/order policy and output-retention policy.
- `CaseSetRef`: immutable dataset identity/version plus authorized access policy;
  development, validation and held-out partitions are explicit.
- `EvaluationContract`: metric names/units, score ranges, handling of refusals,
  failures and incomplete cases, deterministic checks, optional judge configuration,
  aggregation method and evidence required for a comparison conclusion.
- `TrialRef`: experiment/case/arm/repetition identity linked to the actual
  Agent/Turn/pipeline execution and its original operation receipt.
- `TrialMeasurement`: observed outcome, evidence refs, elapsed timing, usage,
  evaluator result and coverage. It includes exact harness and available inference
  identities; it never stores raw private reasoning as general diagnostic content.
- `ExperimentReport`: immutable spec digest, trial coverage and receipt refs,
  baseline/candidate aggregates, comparison method, uncertainty and limitations.
  An amended report supersedes a prior ref; it never edits a report used for promotion.

Foundation operations: `observations.read/watch`; declared experiment contracts:
`experiments.compare/get/list/cancel/report`. HP-7 activates the experiment handlers and
adds their authorized projections to the existing observation path. The compare operation returns `OperationReceipt<ExperimentStart>`: `value` contains
`experiment_ref` and any initial domain summary; the existing receipt's `execution_ref`
identifies admitted work when available. There is no extra execution-status wrapper.
Report reads are authorized reads, not implicit requests to rerun missing trials. A view
may expose a provisional snapshot; only a sealed report can support promotion.

### Preparation and dispatch algorithm

1. Authorize the complete experiment specification and all input/disclosure scopes.
   Resolve exact baseline/candidate/evaluator/case-set revisions before scheduling.
2. Explain which requested environmental properties can actually be reproduced.
   Distinguish history fork, source snapshot, source derivation and external services.
3. Require an explicit policy for external effects: pure recorded analysis, fresh
   inference, isolated qualified environment work, or specifically authorized effects.
4. Reserve the experiment's ordinary causal budget and prepare the original
   dispatch identity. Publish spec plus its existing execution reference coherently.
   Retry with the same operation key observes that publication rather than spawning again.
5. Derive an environment only for trials requiring isolated mutable execution,
   using the same declared baseline and qualified SourceDriver/checkpoint receipts.
   Pure analysis and read-only/inference trials reuse authorized immutable evidence
   references and separate their mutable component state without a filesystem fork.
6. Admit trials through ordinary Agent/Job/pipeline operations, with exact retained
   harness constraints. Mutable Agent defaults cannot retarget a later trial.
7. Record outcomes through original receipts; score authorized outputs with the pinned evaluator.
8. Seal from committed trial/scoring facts; failed publication preserves paid output custody.

The initial runner uses existing bounded pipeline steps plus original Agent/Job work. A
controller may admit multiple independent trials under one shared budget. It must not create
a second task queue or bypass the current nested-batch restriction. A trial's success means
the original owner settled successfully; an evaluator score is a separate fact. A failed
process cannot be relabeled successful by a judge.

### Baseline/candidate isolation and comparability

- Use separate mutable state namespaces/generations for each arm and trial unless
  the spec explicitly calls for persistent-learning trials with their own boundaries.
- Reuse immutable authorized history and read-only evidence by reference; never
  reuse a writable baseline directory as a candidate's supposedly isolated environment.
- Bind each arm to the same source snapshot and initial state contract where supported.
  Unsupported environmental cloning is an explicit limitation or preparation refusal.
- Keep source grants, executor identity, model constraints and budgets comparable.
  Record intentional differences instead of attributing them to the harness alone.
- Separate candidate artifacts from scorer inputs/labels and promotion state.
  The candidate receives its task input, not dataset enumeration, labels or scorer-editing rights.
- Isolate experiment state from the live production Agent. No experimental memory,
  routing affinity, learned instructions or cache-owned mutable state attaches to
  production unless a separately defined, authorized operation imports it.
- Randomize or alternate arm order when time/load matters; retain actual order.
  Repeat stochastic cases as configured. A seed is an input, not proof of determinism.

### Measurement and budget semantics

The foundation fixes shared usage/timing units, missing-value semantics and causal budget
ownership. HP-7 adds experiment-specific accumulation and scoring over those facts; it
cannot create a separate budget allowance or reinterpret an unknown result.

Keep existing `Option<u64>` token totals and missing-usage counts. Use a small typed reason
only where unknown/unavailable/not-applicable needs distinguishing; no generic metrics
framework. Partial totals carry coverage. Check overflow; never wrap or clamp.

Measure queue delay, admitted execution duration, first visible output delay and settlement
time separately, with named start/end events and local monotonic timing. Remote timestamps
cannot establish local elapsed time without a clock contract. Token throughput uses
provider-reported token counts only; missing token counts do not become character-count
estimates. Cost requires actual billing or an explicitly labelled estimate with
rate/version/currency; local/subscription inference is not assigned an invented price.
Admission refusals and preparation costs remain visible.

Every requested case stays in the denominator. Report scheduled, admitted, scored, failed,
refused, cancelled, incomplete and unknown counts. Missing paired results must not disappear
into a favorable average. The evaluation contract determines whether an incomplete pair
receives a conservative score or makes comparison inconclusive.

Record sample count, chosen comparison method, meaningful threshold and coverage. Tiny
examples prove mechanism, not general quality; predetermined repetition/stopping avoids
cherry-picking. Add statistical analysis only when the claimed conclusion needs it. Model
judges retain rubric/revision and known limitations.

Budgets cover candidate generation when included, all arms, environment preparation, scoring
and report-producing model work. Children consume the same parent's remaining budget, not
copied allowances. Existing effect admission owns reservations and settlement. A strict
monetary limit requires a qualified conservative upper bound before admission. If
pricing/usage cannot support that bound, refuse that strict-limit mode or use an explicit
call/token/time limit; never promise a hard spend cap based on unknown usage. Stop new
admissions when the bound is exhausted; drain/cancel already admitted work through original
owners and retain the possibility of late remote billing.

### Storage, observation and recovery

Persist only missing experiment composition/metadata and sealed report refs in the existing
Store under an explicitly designed migration; allocate no schema number here. Trial
execution/attempt state stays with Agent/Turn/Job/pipeline owners. Content uses existing
protected artifact/history custody and source/disclosure dependencies. Reports contain
authorized references or bounded projections, not a second transcript.

Reuse the foundation's authoritative observation, cursor and cancellation semantics.
Optional experiment counters reconcile or advertise reset/coverage after restart. Emit
experiment facts at lifecycle/decision boundaries without per-token harness records.
Existing bounded watches retain gap/resync behavior; detaching one never cancels work.
Durable subscriptions remain separate `events.*` operations with their existing owner.

`experiments.cancel` stops future dispatch and signals original active work. It acknowledges
cancellation intent; settlement waits for factual child outcomes. Process loss resumes the
existing pipeline/operation recovery path. Completed trials are not repeated because the
aggregator lost its local cache. An uncertain trial is inspected through its original
receipt; absent acknowledgment is not a fresh budget. A report may honestly contain
unresolved outcomes; such evidence cannot satisfy a promotion policy requiring complete
trials. Later reconciliation creates a new report.

### HP-7 acceptance scenarios

1. Run one paired comparison through original Agent/pipeline owners. Prove isolated
   arm/production state, exact revision receipts and shared budget while exercising
   one successful case, one failed/refused case and incomplete usage. The resulting
   report keeps all denominators and distinguishes unknown usage from actual zero.
2. Stop the coordinator after a paid result and before report publication; resume
   from existing custody without another effect. Cancel another blocked trial through
   its original owner and retain factual cancellation/remote-usage uncertainty.
3. Use the same comparison to verify protected scorer/held-out access, observer
   revocation and bounded slow-consumer behavior. A small deterministic scorer proves
   aggregation; a bounded real-provider sample qualifies the adapter, not general gains.

## HP-8 — controlled improvement and evidence-bound promotion

### Prerequisites, ownership and contracts

Requires the HP-1–3 activation/policy-validation boundary and HP-7 sealed comparable
reports. HP-5/HP-6 are required only for a chosen component beyond the foundation baseline.
Initial scope: an ordinary trusted controller changes existing declarative context/routing
settings within an established ceiling. Client expansion and richer providers are optional.

Owned areas: `aw-host` experiment/activation composition; Store policy records and existing
binding transaction; ordinary controller/pipeline code; attached client projection and
examples. Promotion remains `harnesses.activate`; no promotion daemon, scheduler, ledger or
additional Agent runtime is introduced.

HP-1/2 establish the following data and policy-validation interface; HP-3 connects the
activation path to its configured policy requirement. HP-8 supplies the bounded controller
and supported promotion evaluator. An unavailable required policy capability refuses
activation; absence never becomes implicit permission.

- `ImprovementSpec`: target Agent/binding, stable baseline revision, editable
  component paths and allowed value ranges, evaluator/case refs, maximum generations,
  total budget, concurrency, cadence, stop condition and retention.
- `PromotionPolicy`: revisioned target/scope, permitted edit classes, authority
  ceiling, evaluator/case-set constraints, coverage and regression requirements,
  budget limits, evidence freshness and future-selection rollback conditions.
- A candidate is the existing HarnessRevision plus its semantic diff and source
  execution refs. An optional rationale explains the proposal; it is not evidence.
- Promotion evidence is the sealed report reference already accepted by
  `harnesses.activate`, binding the baseline/candidate, evaluator/case-set and
  policy revision needed to check the result. Reuse the existing report/revision types.

Implement policy validation through the foundation's original activation boundary. Retain
its fresh authority, identity and generation fences; policy algorithms do not add another
commit path. No custom token protocol, proof framework, promotion owner or evidence archive
is required. A client-supplied score or report ID alone never grants activation rights.

### Controller and promotion algorithm

The controller is an ordinary Agent or registered bounded workflow: observe → propose →
prepare → compare → inspect → activate or stop. Every generation uses original operation
identities, independent candidate state and the parent causal budget. Reaching a configured
stop creates an ordinary completed or partial result; it does not schedule another
improvement loop automatically.

1. Freeze the iteration baseline, objective, editable search space and scoring policy.
2. Generate a candidate within that space; validate its semantic diff before preparing.
   Reject changes to authority, evaluator, data partitions or forbidden executor classes.
3. Run the HP-7 comparison and obtain a sealed report with sufficient coverage.
4. Independently validate eligibility against the current promotion policy. Compute
   report eligibility, semantic diff and state compatibility over immutable inputs
   outside the activation transaction. The gate is deterministic policy code; scoring,
   model calls and unbounded report aggregation never run under the admission lock.
5. Call `harnesses.activate` with exact candidate, `expected_binding`, next-turn
   boundary and `evidence_ref` through the same typed operation used by SDK/MCP.
6. Inside the existing short binding transaction recheck current caller/target authority,
   lifecycle, expected binding, policy revision, retained report/candidate/base/target
   identities, expiry and relevant state/dependency generations. Hold required guards
   to commit. A changed fence returns conflict or requires bounded revalidation outside
   the lock; no generic proof object or second validation engine is required.
7. Commit the binding/activation receipt once. Observe actual usability separately.
   An unavailable committed closure holds future work rather than silently selecting
   the old revision or minting permission to install a missing executor.
8. Evaluate post-activation monitoring only within its declared observation window.
   A permitted rollback selects a compatible prior revision through the same CAS path.

A competing activation invalidates the expected binding; do not silently rebase the
comparison onto its new baseline. Recheck current promoter authority, policy, evidence
freshness and exact report/candidate/base/target correspondence at commit. Immutable report
references prevent changing the comparison after it was accepted.

### Separation, storage and uncertainty

Candidate authors have write access only to their proposal/candidate space. Evaluators have
scoped read access to required outputs, inputs and protected labels; they cannot promote
solely because they can score. Promoters independently hold the target's activation
authority. A single user may explicitly grant several roles; the runtime still checks the
separate operation scopes. Executable untrusted candidates require the qualified boundary
from DESIGN.md, not role labels alone.

Controller progress stays in ordinary Agent/pipeline state. The Store owns a small
revisioned promotion policy and activation's evidence references. Retain the minimal
immutable comparison summary, revision references and factual receipts needed to explain
promotion. Historical scorer references alone do not keep executable processes alive or
block code retirement. Retain implementations only for active work, state compatibility,
continuations or unresolved recovery that actually requires them.

A lost activation reply is resolved by its original operation receipt and binding; never
issue a different operation key to repeat an uncertain activation. Controller cancellation
stops new proposals/trials; a committed activation remains a factual change and is not
automatically undone. Post-commit correction is a new authorized CAS activation. A state
migration, external effect or paid inference is not reversed by selecting an earlier harness
revision. If state compatibility prevents rollback, hold/refuse and report the reason rather
than force old code onto changed data.

### HP-8 acceptance scenarios

1. An ordinary bounded controller changes one permitted context setting, evaluates
   it and activates through the same API as a human client. Its configured authority
   permits this without another dialog; a wider edit returns a concrete policy refusal.
2. Compete two activations from one binding and include wrong/stale evidence plus
   a changed policy or revoked caller. Only the valid current CAS commits; no trial
   repeats and no candidate can alter its scorer, held-out data or authority ceiling.
3. Lose the activation reply, restart the controller and inspect original custody.
   Recovery reports the committed result; compatible rollback affects future work
   only, while incompatible state and retained invocations keep their factual limits.

## HP-9 — AW-side inference and optional ZeroGravity capabilities

### Prerequisites and ownership

Ordinary Provider execution, capability/result shapes, target-reference identity and
usage/cancellation semantics belong to the HP-1–3 foundation. HP-9 independently adds
richer adapters and provider-specific resolution through those interfaces.

Integrated delivery requires that foundation. HP-4/5/6/7 are not required for an adapter;
a paired experiment specifically uses HP-7. This phase never edits ZeroGravity and does
not block on its new SDK/MCP. Exact-pin support is advertised only by a provider that can
actually honor the foundation's exact-reference contract.

Owned areas: `aw-providers` explicit provider/configuration adapters, `aw-host` resolution
and current routing/invocation projections, consuming the foundation's Provider contracts.
Extend `aw-inference` only for an independently versioned optional executor capability.
A genuinely new capability may justify a narrow compatible contract extension, not another
Provider owner or a redesign of shared receipts. Richer remote support cannot become an
unconditional dependency of the kernel, minimal build, ordinary providers or embedded users.

### Implement the established inference contract

Continue actual execution through `Provider::invoke(InvocationContext, ...)`. The foundation
already uses a configured OpenAI-compatible/provider endpoint, with existing account, payer,
credential custody, model allow-list and executor selection. Do not expose trusted
`Agent::set_model` as a remote grant-expansion operation.

The foundation defines optional typed facets `inference.capabilities` and
`inference.resolve`. HP-9 supplies richer implementations that report:
- supported modalities/tool protocol/limits for the actual selected target;
- immutable inference-revision identity when the provider supplies one;
- continuation/cancellation/usage/timing capabilities and their precise limitations;
- compatibility/version evidence and the configured account/executor binding;
- exact, alias-only, unsupported, unavailable or unknown resolution quality.

Capability discovery is an explicitly requested bounded read if it uses network I/O. Pure
harness preparation consumes recorded capability facts; it does not silently probe
endpoints, download models, start services or incur inference. Admission revalidates current
configured authority and required capability/revision availability.

Resolve an optional `InferenceTargetRef` to a `ResolvedInferenceTarget`; retain the actual
identity in the original invocation receipt before the effect. If the provider accepts only
a mutable model alias, record alias-only provenance and make exact-pin requests fail
clearly. No fabricated inference revision or reproducibility. Return provider-operation refs
as protected correlation metadata, not authority. Credentials, private provider
continuations and provider-private reasoning remain with their existing custodian; general
observation sees only authorized projections.

Continuation requests must bind the original executor, compatible model/revision, scope and
original receipt. Unsupported continuation is explicit. A routing change must not reuse an
incompatible hidden continuation or silently replay a paid request.

### Cross-product ordering, cancellation and restart

AW activation and inference activation are separate owner transactions. An explicitly
authorized upper-level controller may prepare inference first, obtain its durable reference,
prepare AW against it and finally activate the AW binding. Each step has its own receipt;
intermediate failures leave inspectable prepared artifacts. AW may need only reference an
already prepared target. Creating or retiring remote models requires a separately granted
operation; it is never a side effect of local harness activation. No shared database
transaction or automatic compensating rollback.

Dropping a local Provider future stops local observation according to the current contract;
it does not prove remote computation or billing stopped. Send remote cancel only through
supported original operation identity and record factual acknowledgment. A lost reply
remains unknown until the configured provider supports reconciliation; do not fall back to
another executor or model to conceal uncertainty. Restart reconstructs exact
target/continuation custody or blocks the invocation. Usage and timing join HP-7 reports
without asserting that AW wall time equals model kernel time. Future richer adapters are
optional capabilities with explicit versions.

### HP-9 acceptance scenarios

1. Exercise the ordinary configured provider unchanged, then a richer fixture that
   reports exact revisions and alias-only/unsupported pinning. Both use original
   invocation receipts, account/executor authority and honest usage.
2. Change a capability or revoke model access between preparation and admission;
   then lose an invocation/cancellation acknowledgment. Refuse or preserve unknown
   custody without a second effect, silent fallback or invented zero usage.
3. Exercise failure before and after AW binding commit in the staged integration.
   Show actual intermediate state and unavailable inference. A live ZeroGravity
   smoke test is optional until an authorized endpoint exists; otherwise retain
   that gate as unexecuted and make no cross-product qualification claim.

## HP-10 — migration, qualification, examples and documentation

### Prerequisites and migration strategy

The useful default programmable harness, embedded/attached Rust API and minimal MCP path
are HP-1–3 deliverables. HP-10 is an ongoing delivery lane: qualify each selected component
and migration as it becomes ready, rather than waiting for every later phase to finish.
HP-4/5/6/7/9 additions have independent feature/support boundaries; self-improvement claims
require HP-7 and HP-8. Do not advertise a capability because its contract compiles or its
schema/example exists.

Owned areas: existing CLI/headless adapters, `aw-host`/client compatibility glue, Store
migration code for actual accepted schema changes, tests/examples, README and live
SDK/CLI/protocol docs. The reserved UX plan stays opaque; future UI work must coordinate
ownership rather than editing that plan as part of this delivery.

The foundation represents existing behavior as the built-in default harness and qualifies
its normal startup before independent expansion. HP-10 preserves that baseline as each
component ships: no implicit daemon, listener, package scan, evaluation, source migration,
model download or new model call. Explicit embedded construction opens no service endpoint.

Migrate one existing product path at a time to the shared typed service. Keep adapters thin
and delete duplicate domain decisions only after the original path's behavior is preserved.
Never maintain two active Agent/Job/Process ownership implementations. Legacy
Session-oriented routing/pipeline bindings map through canonical Agent authority; no
migration assumes Session participation grants Agent control or history disclosure.

For new Store data: design the minimum additive representation, migration ordering,
backfill/default interpretation and read compatibility before assigning a schema version.
Use the existing exclusive owner/writer migration path; do not delete locks, copy raw WAL or
bypass quarantine. Existing historical runs may honestly have no harness revision; label
them legacy rather than inventing an exact revision. Only bind future work to the new
default after reconstruction is qualified.

Protocol adapters negotiate supported versions/capabilities and preserve original operation
identity across upgrades. Reject unavailable facets explicitly. Legacy RPC remains
compatible through its adapter or has a documented version transition. MCP tasks/resources
are projections; a transport upgrade cannot retire retained original operations or turn a
disconnected watch into a cancelled Agent.

### Compatibility cuts and focused qualification

| Existing cut | Boundary to preserve |
|---|---|
| Kernel / alloc / embedded | Existing no_std/alloc features and host-supplied effects; no host/network dependency. |
| Stateless / minimal / full | Tiny stateless mode, JSONL minimal custody, SQLite/full owners; advertise actual capabilities. |
| Embedded / attached SDK | Same semantics; embedded starts no listener, attached uses explicit authentication and endpoint. |
| MCP control | Explicit transport/profile, bounded framing and original operation identity/cancellation. |

The foundation qualifies the public-consumer and feature/dependency boundaries once.
For each later component, select affected builds/owner tests and strict lint/format gates.
Run broader integration only for a changed dependency boundary or an actual release gate;
do not rerun a platform matrix after every edit. Optional MCP/HTTP/SQLite/TS/evaluation
implementations stay out of portable cuts. A bounded startup check observes actual
network/listener/effect behavior when startup changes.

Reuse the foundation's decisive scenario: admit R1, prepare/inspect/activate R2 through
SDK/MCP, settle R1 unchanged and admit R2, including current revocation and recovery.
When HP-7 ships, insert its real comparison before activation; when another component ships,
exercise only the new behavior and remaining failure risks. Reuse existing owner suites
for established invariants. This elaborates the seven V1–V7 scenarios; it does not create a
parallel evidence harness or a separate fixture for every contract sentence.

### Examples, documentation and release boundary

Examples 1–4 are part of the foundation's public-consumer gate, and later phases extend
that same collection. Every example uses public crates, not root-private modules:

1. Small native AgentBuilder use and a simple embedded host run.
2. Attached create/send/inspect/cancel with exact operation receipt recovery.
3. Build/fork, prepare, diff and activate a harness for the next admitted Turn.
4. MCP inspect/control of that same Agent and revision, using a supported profile.
5. One bounded baseline/candidate comparison with honest incomplete-usage reporting.
6. An improvement controller with an explicit edit space and promotion policy.
7. Optional inference integration showing ordinary provider and richer revision facts.

Examples use fixture credentials/configuration placeholders and explicit opt-in for real
paid/network work. They do not grant arbitrary filesystem/network access as a convenience or
require hidden services. Compile/run the shipped examples relevant to the feature claim;
label illustrative pseudocode where no executable example exists.

Update README with the concise product boundary and simple run/customization path; put full
contracts in SDK/protocol docs. Document defaults, activation boundaries, authority versus
visibility, operation retry/recovery, legacy provenance, feature availability, unknown
metrics, state migration and limited environment reproducibility. Show what changes for
users/developers, with factual examples and actual commands.

Record each component's implementation commits and concise executed results as it qualifies;
HP-10 completion applies to the declared shipping subset, not every optional future lane.
No generated evidence archive or exhaustive new test scaffold is required.
Remaining platform, remote-authentication, dynamic-isolation and live-provider gates stay
open by name. Commit only owned paths through the coordinator. Release, publish, tag, push
and deployment are separate explicit decisions; this plan authorizes none of them.
