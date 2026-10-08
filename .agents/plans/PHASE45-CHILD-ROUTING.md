# Phase 45 — governed child target routing

**Status:** authorized 2026-10-05 by the user after Phase 44 review.

## Boundary

Phase 45 lets bounded subagents use an explicitly host-selected immutable routing strategy instead of always inheriting the direct parent's exact account/model.

It reuses the existing Phase-40 routing kernel, strategy revisions, strict classifier, target destination pins, provider-account authority and no-replay receipts. It does not create a second router.

Phase 45 supports one optional routing policy for an entire delegation root:

    --subagent-route ROUTE
    --subagent-route-revision REVISION

If no child route is configured, Phase-44 exact parent account/model inheritance remains unchanged.

A child route strategy must:
- be owned by the initiating stable user;
- have role `worker`;
- be an immutable current routing revision;
- reference only saved provider profiles/models whose destination fingerprints still match;
- satisfy the existing Phase-40 routing strategy validation.

The model-facing `subagent` tool does **not** gain route, target, account, model, candidate, classifier, revision, provider URL or credential arguments.

## Routing modes

Phase 45 reuses the three Phase-40 modes.

### Fixed

Every child under the root resolves the immutable fixed worker target.

No classifier model call occurs.

### Route once

The first child spawned by one direct parent Session/branch classifies its delegated task and establishes the existing Phase-40 affinity for:

- actor = stable initiating user;
- Session/branch = direct parent Agent Session/branch;
- role = `worker`;
- lineage = `subagents`;
- exact root-pinned strategy revision.

Later siblings from that same direct parent reuse the target after normal current account/model/destination revalidation.

A recursive descendant has a different direct-parent Session and therefore its own route-once affinity. It still uses the same root-pinned strategy revision and shared root limits.

### Route each prompt

Every child allocation classifies its own delegated task.

The classifier sees only the normal bounded Phase-40 routing evidence:
- child task text;
- role/lineage;
- candidate IDs/descriptions.

It never sees provider endpoints, credentials, billing metadata or arbitrary new targets.

## Root pinning

Schema v29 adds the exact optional child routing revision to `delegation_roots`.

The first top-level child allocation either:
- stores no child routing revision (inherit mode), or
- stores the exact actor-owned worker strategy revision.

Later top-level sibling allocations and every recursive descendant must supply the exact same routing revision choice.

The root routing revision is immutable. Changing `--subagent-route` or its revision during an active parent turn conflicts instead of mutating the root.

A descendant cannot create a new route policy or widen the candidate set.

## Child allocation selection

Schema v29 adds child-selection evidence to `delegation_child_allocations`:

- `routing_operation_id` nullable FK to the existing Phase-40 routing operation;
- `routing_target_id`;
- `routing_selection_source` = inherit | fixed | affinity | classifier;
- `selected_identity_json`;
- `routing_bound_at`.

For inherit mode, child Session creation records the exact direct-parent/root identity as `inherit`.

For routed mode, the child allocation must receive one consumed selected routing operation before:
- managed-worktree creation;
- child Session creation;
- child provider entry.

The selected identity is the normal freshly revalidated ProviderSelection identity. The allocation cannot store or grant credentials/endpoints.

Child Session creation requires its InvocationIdentity to exactly equal the allocation's selected identity when routing is configured.

## Child route receipt

A selected Phase-40 routing operation is consumed by the child allocation through a dedicated store transition.

Consumption requires:
- actor owns the route operation and delegation root;
- operation state is `selected`;
- operation Session/branch equal the direct parent allocation Session/branch;
- operation role = `worker`;
- operation strategy revision equals the root-pinned child routing revision;
- operation selected target exists in that immutable revision;
- freshly revalidated working identity account/model match the selected target;
- allocation is still `reserved` and has no route selection yet.

The transaction:
- records `working_identity_json` / `consumed_at` on the existing routing operation;
- links that routing operation/target/source/identity to the child allocation.

A route operation cannot be consumed by two child allocations.

## Classifier budget accounting

Routing classifier calls are paid/model attempts and must consume the same Phase-43 root model-attempt budget as child working model calls.

Schema v29 adds `delegation_routing_attempts`.

Before the existing Phase-40 RoutingJournal enters provider dispatch:

1. verify the allocation is reserved in the same active root;
2. verify routing operation is the root-pinned worker revision on the direct parent Session/branch;
3. recheck root deadline and `model_attempts_started < max_model_attempts`;
4. reserve one delegation routing attempt;
5. increment the same root `model_attempts_started`;
6. only then let the ordinary Phase-40 RoutingJournal persist classifier intent/provider entry.

The ordinary Phase-40 routing journal remains authoritative for the paid routing result.

On finish:
1. ordinary Phase-40 routing result is committed first;
2. delegation routing attempt settles second;
3. root `model_attempts_finished` and provider-reported usage evidence are updated.

Missing provider usage remains incomplete; no cost/currency is invented.

If delegation budget settlement fails after the routing result was stored, the route result remains inspectable and no classifier replay is allowed.

Owner restart marks started delegation routing-attempt reservations `unknown`; their root started count remains consumed.

## Provider/account authority

Phase 45 may select a different already-authorized personal provider account/model from the parent.

It never changes:
- stable actor;
- account ownership rules;
- credential custody;
- inference-executor validation;
- billing owner encoded by the account descriptor;
- destination fingerprint pin;
- SecretStore/local credential resolution.

The trusted host registers only the strategy-referenced configured profiles into the existing ProviderAccounts pool. Registration itself is no inference and does not expose credentials to the model.

Every selected target is resolved with current `ProviderAccounts::select(actor, account, model)` and revalidated before working child dispatch.

No fallback to another target occurs if the chosen target becomes unavailable.

## Recursive routing

Phase-44 recursion remains bounded by the same root max depth.

Each descendant:
- inherits the same root child routing revision;
- routes the new delegated task according to that revision;
- uses its direct parent Session/branch for route-once affinity;
- consumes the same root child/model-attempt/deadline counters;
- cannot change the route policy.

A child selected onto account/model B may route its child onto another permitted candidate C, but only because the root host selected the immutable worker strategy containing B/C and the initiating user currently has authority for C.

The direct parent tool caller's own InvocationIdentity is still checked against its stored child relation before descendant allocation. The descendant's routed working identity is separately stored on the new allocation/relation.

## Phase-42 worktree interaction

Routing selection happens **before** any descendant managed-worktree Git mutation.

Sequence:

1. reserve delegation child allocation;
2. select/inherit child target and durably bind allocation identity;
3. if needed, plan/create/verify managed worktree;
4. create child Session using selected identity;
5. start child and enter provider.

A failed route consumes the created-child allocation and any classifier attempt but creates no worktree/child provider call.

A selected route followed by failed worktree preparation remains durably inspectable; there is no reroute/fallback.

## Retry/no replay

Phase 45 does not add automatic child retry.

Routing classifier unknown effects remain Phase-40 unknown/no-replay.

Child working calls remain Phase-41/43 no-replay.

A later explicit child retry phase must pin the original selected routing operation/working identity rather than reclassifying silently.

## UI / CLI

Existing route resource commands continue to define/show immutable strategies.

Phase 45 adds run flags only:

    aw --subagents --subagent-route worker-route ...
    aw --subagents --subagent-route worker-route --subagent-route-revision 3 ...

`aw subagent budget` exposes the root's pinned child routing revision when present.

`aw subagent list/show` exposes each child allocation's routing source/target/operation and the child relation's actual account/model identity.

The Agent sidecar may show a compact routed marker/model for a child, but routing metadata is never injected into model context automatically.

## Schema v29

Additive changes:

`delegation_roots`:
- `child_routing_revision_id` nullable FK to routing strategy revisions.

`delegation_child_allocations`:
- `routing_operation_id`;
- `routing_target_id`;
- `routing_selection_source`;
- `selected_identity_json`;
- `routing_bound_at`.

New table:
- `delegation_routing_attempts`.

Existing v28 roots/children migrate with null child route and preserve inheritance behavior.

## Deferred after Phase 45

Still deferred:
- model-selected route strategy or candidate set;
- parent free-form target assignment outside the root strategy;
- per-child narrowed strategy revisions;
- child-specific resettable budgets;
- monetary budgets/pricing reconciliation;
- automatic reviewer/synthesis role topology;
- automatic join/reducer semantics;
- schedules/cron/autonomy/event triggers;
- trusted/bash child tools;
- automatic Git stage/commit/merge/import;
- cross-machine child placement;
- browser/shared participant spawning.

Those require explicit later phases.

## Validation requirements

At minimum:
- v28 -> v29 identity-preserving migration;
- existing v28 roots remain inherit/no-route;
- root child routing revision must be actor-owned role worker and immutable;
- default no-route behavior preserves Phase-44 exact parent identity;
- fixed child route uses alternate allowed account/model with zero classifier calls;
- route-once classifies once per direct parent and siblings reuse affinity;
- recursive descendant route-once has independent direct-parent affinity under the same root strategy;
- route-each classifies every child task;
- classifier evidence omits account/model/endpoint/billing metadata;
- classifier attempt reserves shared root model-attempt budget before provider entry;
- classifier usage settles into shared root counters after ordinary routing result;
- classifier limit/deadline blocks before provider;
- invalid/unknown proposal creates no worktree/child provider call;
- revoked/unavailable selected target blocks with no fallback;
- destination-fingerprint change blocks before disclosure;
- child allocation route receipt cannot be consumed twice;
- routed selection bound before worktree planning;
- child Session identity equals allocation selected identity;
- recursive routed child can change permitted model while retaining same actor/root/budgets;
- max-depth/no-recursion semantics from Phase 44 remain;
- Phase-44/43/42/41/40/39/38 checkers remain green;
- tiny/no-default builds remain free of routing unless routing feature is compiled;
- exact whole-workspace/all-targets, doctests, rustdoc, Clippy, reference/static gates;
- actual compiled fixed + classified child-routing smokes;
- pinned Nix/release claims only when the Nix store is writable.
