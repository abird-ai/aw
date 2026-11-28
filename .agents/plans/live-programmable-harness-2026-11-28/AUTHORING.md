# Component evolution — client surfaces, authoring and extensions

Date: 2026-11-28
Base: a47492f45813f1e85f85e706e7f33292f0c3e6f0
Status: approved direction; detailed implementation proposal, no runtime change.
[Canonical API](API_AND_PROTOCOLS.md) · [Foundations](FOUNDATIONS.md) ·
[Learning and delivery](LEARNING_AND_DELIVERY.md) · [Queue](TODO.md).

HP-1–3 establish the public operation contracts, component ports, lifecycle/state
semantics and executable baseline embedded/attached Rust and MCP paths. These
phases independently extend that foundation; they do not first discover its API
or introduce another Agent loop, state service, executor or extension authority.

HP-4, HP-5 and HP-6 can progress independently after the foundation. A specific
consumer may require a named capability from another component; that is an
explicit optional dependency, not a dependency on the whole phase. Contract
availability never advertises an unimplemented runtime capability. Each component
advances its own implementation/schema revision under the foundation's compatibility
rules; unrelated components keep their valid handles and supported protocol versions.

## HP-4 — Client ergonomics and transport expansion

### Outcome and dependencies

The foundation already lets an external Rust program, attached client and minimal
MCP client inspect/control the same Agent and harness revision. HP-4 expands the
client experience, selected export profiles and qualified transports.

Entry: completed HP-1–3 common contracts and baseline adapters. HP-5, HP-6, HP-7
and HP-9 are not blanket prerequisites; expose their implementations when the
selected host actually supports them. Conversely, those components do not need
HP-4 before using the foundation's typed operations or baseline MCP adapter.

### Implementation cuts

1. Improve client builders, selection helpers, bounded observation iteration and
   result handling over the established service. Convenience methods retain the
   original mutation identity and do not hide additional effects.
2. Extend attached/RPC/MCP coverage to newly qualified operation families through
   the same contract types and domain handlers. Keep legacy decoder, bounds and
   uncertainty behavior until their transition is separately completed.
3. Add useful MCP profiles and resource projections; implement optional Tasks
   mapping through original operation custody and the foundation's version matrix.
4. Add remote transport/authentication only for an explicit supported deployment.
   Map verified principals into the same CallContext; local same-UID trust does
   not become a remote authentication design.
5. Consume the foundation's shared schemas and DTO generation for client packages.
   Extend that generation only for a real new contract; do not add parallel
   handwritten validators or a schema framework.
6. Extend CLI/client examples and developer documentation for the supported
   additions. Product interfaces consume shared behavior without a renderer rewrite.

The initial Service extraction, canonical native DTOs, v2 adapter framing and
minimal MCP server are HP-1–3 deliverables. HP-4 does not postpone or redefine them.

### Mature facade over the same contracts

Public examples import Service and shared request types from `aw_host` for
embedding, or Client and those same pure request types from `aw_client` when
attached. The CLI-heavy root library is not an SDK dependency. These re-exports
preserve the foundation's feature and dependency boundaries.

The product README presents the mature API directly. Implementation readiness and
qualification stay in this plan and its queue. The exact convenience lowering is
defined once in [API_AND_PROTOCOLS](API_AND_PROTOCOLS.md), including:

- `Service::embedded(config)`, its bound native `client()` and explicit `shutdown()`.
- Attached `Client::connect(endpoint)` with the same facet semantics and a lightweight dependency boundary.
- `AgentSpec::new(name)` using authorized configured defaults; `.harness(exact_ref)` only changes selection.
- Receipt-carrying Agent and Candidate handles; `agent.run` submits once and observes its original execution.
- `experiments.compare` returning ExperimentRun; `wait` returns a sealed completed report or an honest non-complete outcome.
- Pure local HarnessDefinition and ActivationRequest constructors over registered typed references.
- Explicit `aw mcp serve --stdio --mcp-profile authoring` over the original owner.

The foundation supplies the baseline bound facade and common lifecycle helpers;
HP-4 extends ergonomics without changing their semantics. Helpers preserve one
semantic key per displayed mutation, expose original receipts/recovery references
and never turn a timeout or lost reply into fresh work. Native convenience calls
remain typed; no JSON round-trip is introduced to share the attached experience.
Embedded shutdown follows original runtime settlement and does not delete durable
Agents or equate local cancellation with proven remote termination.

### Baseline shape retained by this lane

This proposed shape belongs to the foundation; HP-4 preserves it while adding
conveniences and transport coverage.

```rust
// Proposed; connect to an explicitly configured existing owner.
let client = aw_client::Client::connect(endpoint).await?;
let agent = client.agents().get(agent_ref).await?;
let submitted = agent.send("Review this change", request_key).await?;
let status = client.operations().get(&submitted.operation_ref).await?;
if let Some(execution_ref) = status.execution_ref {
    let snapshot = client.jobs().get(&execution_ref).await?;
    // A factual snapshot, not an assertion that execution has completed.
}
```

agent.send maps to canonical agents.send with one original Input mutation
identity; it never hides an inputs.add plus inputs.send sequence. Accepted input
may still be queued, so execution_ref is optional. Use the original operation
status or bounded observation to await admission/settlement without resubmission.

Embedded code obtains the corresponding facet from Service and supplies its
host-minted CallContext. Lower AgentBuilder remains available for small embedders.
harnesses.fork returns an editable value without creating a persistent draft;
harnesses.prepare is the sole durable candidate-creation operation. Native code
receives OperationReceipt<Preparation> with summary and optional PreparedHarness;
the adapter returns the same receipt projected to PreparationSummary, without
serializing handles. An optional PreparationRef accelerates activation but
expiry never changes revision selection.
A simple read needs no mutation metadata; a simple send supplies its semantic
key once. Client helpers may generate a key once and return it to the caller;
a transport retry must reuse it.

Foundation MCP names map mechanically to canonical methods with one public
spelling. Its baseline supports inspect/send/observe/cancel and harness
prepare/diff/binding/activate. HP-4 adds profile composition and exports qualified
experiment/extension operations as their implementations arrive. Profiles select
exposure; permissions still decide use.

Server startup is explicit. If an owner already exists, attach to it through
the supported owner-client path; never race another writer by opening its DB.
Stateless/minimal hosts expose their actual supported subset.

### Source work and compatibility

Work in existing aw-host/aw-client facets, headless/MCP adapters and examples;
add remote transport at the edge only when selected. The existing aw-mcp client
remains an independent consumer capability. Reuse the foundation's contract
placement; transport additions cannot pull SQLite, HTTP, UI or MCP into the
portable kernel or force unrelated components to adopt a new client package.

### Acceptance

Reuse the foundation's V1 SDK/MCP activation/cancellation scenario. Extend it only
for the selected client/profile/transport behavior, including its actual auth,
reconnect and legacy identity risks; do not duplicate the baseline suite.

For Tasks, verify negotiated schema/result mapping, durable lookup before handle
publication and cancellation without fabricated terminal facts. An unsupported
version/capability fails before execution. For a remote adapter, test its real
principal mapping rather than inferring it from local IPC. Feature checks cover
the affected cuts. Compile checks alone do not qualify an added runtime path.

## HP-5 — Programmable context and strategy composition

### Outcome and dependencies

Users extend the baseline context, route and strategy composition established in
HP-1–3. Implement useful algorithms and reusable recipes against the already
defined ContextFlow/provenance, strategy, routing and state ports.

Depends on the completed foundation. Native implementations need neither HP-4
client expansion nor HP-6 dynamic runtime. A selected dynamic contributor adds
only that capability dependency. The baseline SDK/MCP can already inspect,
prepare and activate the resulting registered compositions.

### Implement the established context contract

HP-2 defines ContextProgramRef, ordered contributions, source selectors, limits
and ContextProjection with original provenance/disclosure dependencies. HP-3
exercises a default and one custom composition through that port. HP-5 adds
implementations and richer recipes without changing those ownership semantics.

Reuse source selection, inherited-history pins, manual-import restrictions,
summary execution and existing context acceptance. A projection cannot relabel
a fact, weaken its disclosure manifest, rewrite HistoryBase or invent a provider
release. Model-based compression is separately admitted paid work; a pure
prepare/inspect operation cannot invoke it.

Extend the foundation's useful default composition: fixed instructions + bounded
recent history + selected repository context, then optional approved summary and
richer selection/compression implementations. Resolve duplicates and ordering
deterministically. Explain each contributing block and budget truncation while
retaining provenance by reference.

### Implement the established strategy contract

HP-2 defines StrategyRef variants for the original Agent loop, an existing
immutable pipeline revision and a registered native action/controller. HP-5
implements useful controllers and recipes through those ports. Existing routing
revisions remain their authority for configured model/account candidates.

A controller schedules original Agent/Job/pipeline operations and consumes their
receipts. It cannot invoke providers/tools outside the original admission and
journal boundary. Keep pipeline sequencing, Actor collaboration and finite
batch Graph execution distinct, including the current nested-batch restriction.

First workflows: plan/work/review with a bounded repair count; specialist
delegation with an explicit join; route selection among authorized models.
Use existing planner/reviewer and foundation action contracts. A genuinely new
inner round-decision behavior may justify a narrow versioned capability at an
existing safe point; it must not force a redesign of common service, registry,
authority or lifecycle contracts. Do not prebuild a universal strategy framework.

Stop/continue decisions are bounded proposals at existing safe points.
Hook Next/Defer semantics and the original continuation owner remain unchanged.
An incompatible continuation after code loss is unavailable, never a newly
created Turn disguised as resumed work.

### State and live composition

Consume HP-2/3 State contracts: explicit logical owner, module/key identity,
schema and semantic compatibility. Compatible shared state uses existing CAS;
incompatible changes drain old users or select separate state. HP-5 supplies
component-specific state behavior/migrations, without another state owner.

Baseline and candidate evaluations start from explicit comparable state
selections. Use fresh/derived experiment state unless shared-state contamination
is deliberately part of the experiment. Candidates cannot mutate production
memory as an accidental consequence of evaluation. Source/environment copying
still uses the qualified original checkpoint/driver contracts.

### Source work and acceptance

Work in ContextFlow adapters, aw-agent-core narrow behavior ports, aw-routing,
aw-pipeline, host composition and actual examples. Store changes stay minimal
and use existing component owners rather than copied revision tables.

A single end-to-end scenario compares two context/strategy revisions on a
controlled case, preserving original source manifests and inference receipts;
it does not require the HP-7 experiment service. Include a revocation during
preparation, old/new state coexistence and a held Defer continuation. Verify bounded termination and that optional strategies add no
default model call. Reuse affected existing history/context/pipeline tests.

## HP-6 — Dynamic Process Extensions and TypeScript/JavaScript SDK

### Outcome and dependencies

An author runs a selected TS/JS module, registers bounded definitions, accesses
granted ctx ports and reloads future behavior through the established host.
Depends on the completed HP-1–3 foundation: atomic registry publication, typed
host ports, scoped lifetime rules and the successor ABI contract. It does not
depend on HP-4 client expansion or HP-5's algorithms. A specific extension may
choose either as an explicit capability dependency.

Converge with DL-200 Phases 5–6; keep their final leaf identities and native-only
exclusions. The foundation defines this boundary; HP-6 implements the expanded
process runtime and author SDK rather than inventing a parallel plugin registry.

### Implementation cuts

1. Implement the foundation-defined successor ABI and compatibility adapter for
   existing ABI-2 tools/hooks/prompts; preserve explicit host selection.
2. Implement its full-duplex connection adapter with disjoint request IDs,
   one top-level callback, host-minted invocation scopes and bounded nested
   ctx calls. Existing aw-process remains the lifetime/cleanup owner.
3. Add manifest-to-typed-registration adapters for the sealed dynamic subset.
   Validate all selected contributions before atomic root publication.
4. Bind ctx operations to the original CallContext/Agent/Turn/callback scope;
   no child-supplied authority, raw process handles or ambient host APIs.
5. Implement cancellation/poison/exit handling that expires scopes, suspends
   affected handlers and preserves uncertain effects and parked continuations.
6. Ship a small TS/JS author SDK for schemas, registration, invocation scopes,
   framing and typed errors. Plain JavaScript receives the same runtime checks.
7. Add a documented example that contributes a tool, safe hook, read-only view
   and typed State key, then changes its next-revision behavior.

The successor protocol retains 256 KiB frames and 128 KiB manifests, at most
8 cross-extension callback levels and 256 nested calls per root deadline.
Synchronous same-process re-entry fails before dispatch. Final exact wire and
ABI-2 migration are specified in API_AND_PROTOCOLS.md.

The foundation defines typed ctx.tools/state/events/settings/models/process/ui
ports and their authority/lifetime/result contracts. HP-6 supplies the process
transport adapters and selected dynamic contributors. Availability is per
invocation; schema or manifest presence cannot grant authority or advertise an
absent implementation. Dynamic Compactor and privileged provider/source hooks
remain excluded.

### Reload, packaging and acceptance

SDK/MCP extensions.reload selects an already configured Extension and expected
registry revision; its semantic operation key preserves original process effects.
The request cannot supply a new executable, credential or authority policy.
Reload stages a newly described/validated process generation. A same-name
replacement waits for all affected Turn, Job and continuation custody, not just
the foreground Turn. Incompatible active state also blocks replacement.
Fresh registration does not resurrect old grants. A manifest
revision is compatibility metadata, not executable integrity proof.

Npm/pip/cargo installation remains outside AW. No embedded Wasmtime requirement.
The currently selected process is trusted OS-user code; process separation alone
does not qualify untrusted AI-generated executable modules.

One real process fixture should exercise nested allowed ctx calls, forbidden
re-entry, a timeout after a possible effect, handler suspension and subsequent
validated reload. Confirm no automatic callback replay and no same-name tool
rebinding under an old Turn. Run the TS example against the actual binary
protocol; static TypeScript compilation alone is insufficient.

## Cross-phase simplicity and performance contract

Use one semantic definition, typed native calls and thin wire adapters.
Prefer static dispatch/borrowing on native paths; dynamic dispatch belongs at
extension/transport/provider boundaries where variability requires it.
Resolve immutable composition once per activation/admission and retain handles.
Avoid JSON round-trips, deep configuration copies, repeated dependency traversal
and lock-held user callbacks on hot execution paths.

Use bounded observation/queues from existing owners; no per-token harness record
or periodic whole-Store scan. Measure a real controlled Turn/Job before adding
caches, schedulers or generic engines. Take simple demonstrated wins; otherwise
keep the implementation readable. Tests target changed owner/failure boundaries,
not one fixture or evidence archive per setter.
