# AW live programmable harness — implementation queue

Base: `a47492f45813f1e85f85e706e7f33292f0c3e6f0`
Status: foundation-first planning complete; runtime implementation pending.
Map: [IMPLEMENTATION](IMPLEMENTATION.md) · Contracts: [API_AND_PROTOCOLS](API_AND_PROTOCOLS.md)

HP identifiers are local to this plan. Existing R6/DL-200 ownership and queue remain
authoritative. HP-1–3 establish the shared contracts and working programmable
substrate; later component phases consume those boundaries independently.
Each checkbox represents a coherent deliverable, not an accessor or evidence artifact.

## Committed

### This commit — live programmable harness design and documentation

- [x] Investigate current AW source and establish the complementary AW/ZeroGravity boundary.
- [x] Define canonical API/protocol contracts with simple reusable Rust primitives and focused verification.
- [x] Move the complete declared contract surface and baseline SDK/MCP integration into HP-1–3.
- [x] Separate independent component implementation from shared foundation decisions and versions.
- [x] Rewrite the root README for the requested mature product narrative, with two architecture SVGs and CLI/SDK/MCP examples.
- [x] Specify the bound SDK facade, lightweight public imports and explicit MCP server command behind those examples; runtime delivery remains pending.
- [x] Surface graph-native execution and AI-native tools in the mature README and architecture, with a source-audited compact batch example.

## Uncommitted

### Plan and coordination

- [ ] Reconcile implementation ownership and accepted work with the coordinator's DL-200 queue.
- [ ] Revalidate affected source/HEAD and actual migration baseline before implementation.

### HP-1 — Host and public contracts

See [FOUNDATIONS](FOUNDATIONS.md).

- [ ] Establish all declared public facet request/result/schema contracts, canonical references and capability/version rules.
- [ ] Implement shared CallContext, original mutation/receipt/error semantics, budgets, cancellation and owner-local observation.
- [ ] Extract a real create/send/inspect/cancel path and baseline typed embedded/attached operations through existing owners.
- [ ] Adapt the existing caller and protocol compatibility edges without duplicate admission/runtime ownership; preserve feature cuts.

### HP-2 — Composition and component contracts

- [ ] Implement normalized immutable composition, exact references, metadata prepare/get/fork/diff and native/wire preparation.
- [ ] Establish typed registration/host-port contracts for every planned component family using existing R6 roles and original owners.
- [ ] Define state, provenance, budget, lifecycle and version behavior, including evaluation/report/promotion and optional inference boundaries.
- [ ] Add minimal composition/binding/receipt persistence with honest legacy provenance; component-specific engine tables come with their implementations.
- [ ] Run the default and one independently authored native revision on new instances against the frozen installed set.

### HP-3 — Live runtime and integrated foundation

- [ ] Complete atomic registry publication, expected-binding CAS and next-eligible-admission pinning.
- [ ] Preserve tool/hook/continuation custody, current revocation, compatible state, retirement and original-owner recovery.
- [ ] Deliver the real common path through embedded Rust, attached Rust and minimal MCP: discover/create/send/observe/cancel/prepare/inspect/activate.
- [ ] Prove independent public component authoring without root-private imports, and accurate absence of optional engines.
- [ ] Close the foundation gate using compiled declared contracts plus V1/V2/V7 and the existing native composition scenario.

### Independent component work

HP-4/5/6/7/9 can proceed independently after the foundation. HP-8 requires HP-7.
A particular dynamic evaluator, environment or provider may add an explicit narrow
capability dependency; no lane depends on another merely because of phase numbering.

### HP-4 — Client and transport expansion

See [AUTHORING](AUTHORING.md).

- [ ] Improve attached/native client ergonomics and expand qualified export profiles over established operations.
- [ ] Add selected transport/authentication/Tasks capabilities with the same receipt, observation and cancellation behavior.
- [ ] Ship examples for each supported expansion; keep lightweight clients and kernel dependency cuts intact.

### HP-5 — Richer context and strategy implementations

- [ ] Implement useful projections and context recipes through established provenance, ordering and budget contracts.
- [ ] Add bounded plan/work/review and delegation using existing loop/pipeline/controller/routing interfaces.
- [ ] Demonstrate compatible state coexistence and original cancellation; add a deeper round policy only for a concrete unmet consumer.

### HP-6 — Dynamic Process runtime and TS/JS authoring

- [ ] Implement the established successor ABI/ctx contracts and existing ABI-2 compatibility in the original Process owner.
- [ ] Adapt the sealed dynamic subset into the existing atomic registry with configured reload/withdraw and native-only exclusions.
- [ ] Qualify bounded calls, poison, drain and recovery using a real process; ship the TS/JS example and SDK.

### HP-7 — Experiment implementation

See [LEARNING_AND_DELIVERY](LEARNING_AND_DELIVERY.md).

- [ ] Implement comparison/scoring/report operations over foundation-defined contracts and original Agents/Jobs/pipelines.
- [ ] Isolate mutable trial state, share budgets and preserve full coverage plus honest incomplete usage.
- [ ] Add bounded experiment projections and minimal component-owned storage; recover/cancel through original execution custody.

### HP-8 — Bounded improvement

- [ ] Implement an ordinary controller with explicit edit space, stopping rules and total budget using HP-7.
- [ ] Apply independent evaluator/case-set/promotion policy through the foundation's activation validation boundary.
- [ ] Keep expensive validation outside the lock; recheck current fences at commit and support compatible future rollback.

### HP-9 — Richer inference integration

- [ ] Implement optional capability/exact-revision/continuation support over the foundation's Provider and reference contracts.
- [ ] Preserve actual target identity, uncertainty, usage and original receipt custody.
- [ ] Demonstrate staged AW/inference composition; qualify a live ZeroGravity endpoint only when available and authorized.

### HP-10 — Incremental migration and delivery

- [ ] Migrate remaining callers and legacy data incrementally; preserve the working foundation default and normal startup.
- [ ] Apply actual component migrations/adapters through their existing owners and shared compatibility rules.
- [ ] Ship public examples/docs and qualify each declared product subset without waiting for all optional components.
- [ ] Remove proven duplicate paths and record actual implementation results/commits; publication is separately authorized.

## Tests and verification

Documentation only: no product tests were run. Runtime gates below remain open.
The foundation reuses V1/V2/V7 plus the existing native composition scenario in V3.
V4–V6 qualify later implementations when present; no separate evidence framework.

- [ ] **V1 Shared operations:** embedded/attached/MCP reach the same Agent/Job, receipt and cancellation semantics.
- [ ] **V2 Live revision lifecycle:** R1/R2, stale CAS, exact custody, revocation and restart/unavailable dependency.
- [ ] **V3 Composition and state:** public native component, provenance, bounded work, shared budget and isolated state.
- [ ] **V4 Extensions:** scoped nested calls, bounded framing, forbidden re-entry, poison/reload and unknown effects.
- [ ] **V5 Evaluation/promotion:** comparable cases, independent scorer, truthful coverage/usage, fresh policy/CAS and future rollback.
- [ ] **V6 Inference:** ordinary provider and optional revisions, factual cancellation/usage/unknown outcomes and staged integration.
- [ ] **V7 Product/efficiency:** compiled declared contracts, feature cuts, ordinary startup, public examples and targeted hot-path checks.

### Documentation verification

Passed on 2026-11-28: the root README and eight plan documents, 136 local Markdown
links, eight JSON examples, fences, whitespace and final newlines. The README's four
Rust excerpts and seven shell blocks passed syntax parsing; both SVGs passed XML
checks and were rendered and visually reviewed. The README intentionally describes
the user-requested mature product; these checks do not compile the target SDK or
qualify runtime behavior. Review reconciled facade imports, original receipt/wait
semantics, MCP startup, component dependencies and native/dynamic port scope.
The graph-native follow-up adds AI-native tools and Graph execution to the README
and overview SVG. Its batch example and `jobs_*` vocabulary were checked against
the current shared schemas, Graph parser and AW adapter; no graph execution was
claimed. The final copy uses the approved “harness you can program live” headline,
links the sealed DL-150 first-principles tool design, and adds the verified
ZeroGravity/dotlink Related projects links. The first FAQ explains live authoring
by people, SDK applications and MCP agents, including iteration between turns and
retained in-flight revisions. The design link explains rationale; current schemas
and the Graph parser remain the authority for exact API spelling.
The original shell-containment note moved to the tools guide. This checkpoint
contains design, documentation and SVG assets only; runtime implementation remains
pending. The Base above records the source revision used for this design.
