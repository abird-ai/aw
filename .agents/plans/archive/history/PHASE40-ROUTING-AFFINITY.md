# Phase 40 — bounded main-target routing and sticky affinity

**Status:** implementation and aggregate validation complete; checkpoint ready for user review.
**Base:** `2ae69da` (unnumbered post-Phase-39 correctness/UI checkpoint).
**Schema target:** SQLite v24.
**Product boundary:** the next bounded original-v0.7 Phase-7 routing slice only: main-target selection and per-user branch-lineage affinity. Dynamic child workers, schedules/autonomy, dashboards/cost UX, multi-model teams and advanced governance remain later phases.

## Goal

Let an explicitly selected routing strategy choose **which already-authorized provider account/model handles the main Agent prompt** without creating a second Agent loop or expanding authority.

Phase 40 supports three main-target modes:

- **Fixed** — use one configured target; no classifier call.
- **Route once** — classify the first admitted prompt for one actor/session/branch/role/lineage key, then reuse the durable affinity after revalidating current account/model authority.
- **Route each prompt** — run one bounded classifier operation for each admitted prompt and record the selected target.

The working prompt still executes through `Agent::prompt_as`. Routing never owns tools, filesystem access, provider credentials, endpoints, browser authority or Session history.

## Non-negotiable authority rules

1. A routing target is a secret-free configured reference: `target_id + account_id + model + destination_fingerprint`. `route define` derives that fingerprint from the saved profile using the same endpoint/custody-reference hash as durable Session binding; a later profile destination change blocks routing until a new immutable strategy revision is explicitly defined.
2. Strategy definitions cannot contain provider URLs, credentials, environment-variable names, tool grants, shell commands, executable code, arbitrary system prompts or user identities.
3. The classifier is itself one explicitly configured `account_id + model`; it is selected/revalidated through the caller's existing `ProviderAccounts` authority.
4. The classifier sees only one bounded routing evidence envelope: the admitted user prompt, role/lineage metadata and the strategy's candidate IDs/labels. Candidate endpoints, secrets and billing metadata are not classifier input.
5. Classifier output is strict typed JSON containing exactly one candidate ID. Free-form output, unknown IDs, tool calls, incomplete output or malformed JSON fails closed. There is no default fallback.
6. A candidate proposal never grants authority. The selected candidate is resolved through `ProviderAccounts::select(actor, account, model)` immediately before working-model dispatch and revalidated by the normal Agent loop.
7. Fixed/affinity routes short-circuit classifier inference only when the target still revalidates. An unavailable/revoked target blocks visibly; Phase 40 does not silently pick a different vendor/account.
8. Route-once affinity belongs to the stable caller and exact Session/branch/role/routing-lineage key. It is not shared merely because users share a Session. Forks have fresh affinity unless an explicit future policy says otherwise.
9. Affinity is a preference projection, not an authorization receipt. It is updated only by expected-revision CAS after an admitted route operation.
10. Paid classifier intent is durable before provider entry. A process crash after classifier dispatch begins but before settlement becomes `unknown_external_effect`; restart never automatically replays that attempt.
    A cancellation/authorization failure before classifier intent reaches the provider boundary settles the admitted route as cancelled/failed with no attempt; it is never left indefinitely `admitted`.
11. A routing operation/attempt is distinct from the working Agent turn. It never appends a fake Chat turn.
12. Bare `aw` remains the direct fixed configured Agent path unless a routing strategy is explicitly selected/bound. No hidden classifier lookup occurs in the tiny/stateless path.

## Transport-neutral routing model

Add `aw-routing`, with no provider/store/filesystem/network/auth dependencies. It owns:

- `RoutingMode::{Fixed, RouteOnce, RouteEachPrompt}`;
- `RoutingTargetRef { id, account_id, model, destination_fingerprint }`;
- `RoutingStrategyDefinition { name, role, mode, fixed_target?, classifier?, candidates }`;
- strict validation and canonical SHA-256 strategy digest;
- bounded classifier evidence envelope creation;
- strict `RoutingProposal { candidate_id }` parsing/validation.

Bounds:

- at most 16 candidates;
- IDs/role/name are bounded portable labels;
- prompt/evidence maximum 128 KiB;
- classifier output maximum 4 KiB;
- candidate IDs unique;
- fixed mode requires a valid `fixed_target` and no classifier;
- classify modes require a classifier and at least two candidates;
- classifier itself is not implicitly a candidate.

## Agent-core routing evaluator

Add a distinct `InvocationPurpose::Routing` plus a tool-free `RoutingExecutor`, following the proven summary-executor discipline:

- host journal authorizes exact strategy/evidence/destination;
- `begin` durably commits classifier intent before provider invocation;
- provider/account selection is revalidated before and after awaited gates;
- no tools;
- bounded complete output;
- strict proposal parsing;
- provider-private continuation is not persisted as routing evidence;
- settlement happens even after cancellation/failure, and a settlement failure is not permission to regenerate.

No raw classifier stream is rendered as Chat output.

## SQLite v24

Add immutable routing strategy revisions and durable selection state:

- `routing_strategies`
- `routing_strategy_revisions`
- `routing_bindings`
- `routing_affinities`
- `routing_operations`
- `routing_attempts`

Properties:

- immutable strategy revisions;
- user/self and Session routing bindings, role-scoped;
- route operations tied to stable user + Session + branch + role + lineage + exact input digest;
- route-once affinity unique for that exact key and CAS-revisioned; process-local provider lease revisions are **not** persisted as durable affinity authority; reuse always performs a fresh current account/model selection plus destination-pin validation;
- routing attempts contain classifier invocation identity, start/settlement state, provider usage metadata when available, and process/store-generation ownership;
- no raw provider secrets/endpoints or raw prompt text in routing tables;
- current-schema trusted-local direct remains exact-schema/no-migration;
- owner-mode v23→v24 migration only.

Recovery marks a `started` routing attempt owned by a dead/fenced prior process as `unknown_external_effect` and its operation as `unknown_external_effect`. No new classifier invocation is created automatically.

## Typed CLI / explicit runtime

Add a `route` resource:

- `aw route define FILE`
- `aw route list`
- `aw route show ROUTE [--revision ...]`
- `aw route bind ROUTE --scope user|session --scope-id ... [--role main]`
- `aw route bindings`
- `aw route binding-set BINDING --expected-revision N --enable|--disable [--revision ...]`
- `aw route affinity --session ID --branch NAME [--role main] [--lineage main]`
- `aw route affinity-clear ... --expected-revision N`
- `aw route status OPERATION`

Explicit routed prompt execution is enabled with `--route ROUTE` plus optional `--route-revision`, `--route-role` and `--route-lineage`. Phase 40 requires durable SQLite Session state for classified routing so the paid route intent is admitted before dispatch. A fixed strategy can be resolved without classifier inference but still records the selected route when used durably.

The runtime registers only strategy-referenced configured profiles needed for this caller. It does not turn every profile into a shared account or bypass OAuth/SecretStore custody.

## Required behaviors

### Fixed

- resolve exact strategy revision;
- select fixed target through current caller account/model authority;
- record resolved target when a durable routed prompt is used;
- zero classifier provider calls.

### Route once

- resolve strategy and exact affinity key;
- if affinity exists, require same strategy revision and revalidate its target; no classifier call;
- if no affinity, admit route operation and classifier intent, execute one classifier attempt, validate proposal, resolve working target, CAS-create affinity;
- concurrent first selectors cannot overwrite each other; losing operation remains inspectable and returns conflict rather than silently adopting/overwriting.

### Route each prompt

- every routed prompt gets one admitted classifier operation;
- proposal is strictly validated and working target revalidated;
- target changes are visible in operation receipts;
- no affinity is created.

## Explicit non-goals

Phase 40 does **not** add:

- child/worker Sessions or fan-out;
- child routing;
- schedulers, cron, overlap/missed-run policy or background autonomy;
- event/webhook triggers;
- multi-model teams, reviewer/synthesis topologies or joins;
- Jev/Laya-specific network adapters;
- generative planners or arbitrary DAG instantiation;
- automatic budget/cost optimization;
- dashboard/observability UI;
- guardian recursion/advanced group policy;
- classifier-selected URLs/accounts/models outside the prevalidated candidate set;
- routing-driven credential/tool grants;
- automatic fallback after classifier/target failure;
- native shared-draft participation.

## Validation gates

Before Phase-40 checkpoint/tag:

- `aw-routing` unit tests for validation/digest/evidence/proposal parsing;
- v23→v24 migration regression preserving store identity/generation;
- routing store tests for ownership/binding, user isolation, fork freshness, affinity CAS and recovery/no-replay;
- executor tests for tool-free strict proposal behavior and invocation-purpose binding;
- fixed route proves zero classifier calls;
- route-once proves one classifier call then affinity reuse;
- route-each proves one classifier operation per prompt;
- invalid/unknown classifier proposal fails without working-model dispatch;
- revoked/unavailable affinity target blocks without fallback;
- actual compiled fake-provider routed-prompt smoke;
- warning-denied tiny/direct path proves no routing initialization or lookup;
- normal full product/workspace/all-targets/doctests;
- pinned Clippy 1.98.1 and rustdoc warnings-as-errors;
- source-bound Phase-40 checker + complete reference profile;
- final static validation, secret scan and `git diff --check`.

## Review boundary

Phase 40 ends when main-target routing and sticky affinity are durable, explicit, crash-safe and validated. Stop there for review. Dynamic child workers and scheduling/autonomy require another explicit numbered phase.

Historical tags remain immutable. Do not push unless explicitly requested.

## Final measured checkpoint evidence — 2026-10-04

The final Phase-40 implementation bytes are green for:

- `aw-routing` kernel: **5/5**;
- tool-free routing evaluator plus inherited `aw-agent-core` suites;
- routing store/affinity/recovery: **6** focused routing tests plus same-context retry destination regression;
- v23→v24 identity-preserving migration and trusted-local direct stale-schema refusal;
- Phase-40 source-bound checker: **158 checks**;
- actual compiled loopback-provider smoke: fixed zero-classifier, route-once one classifier then affinity reuse across a fresh `aw` process, route-each one classifier per prompt, and destination-pin change blocked before provider entry;
- full product: **101 library/web + 110 CLI/workspace + 6 provider fixtures** under `-D warnings`;
- tiny/no-default and isolated `routing` feature cuts under `-D warnings`;
- exact whole-workspace/all-targets tests;
- whole-workspace doctests;
- pinned Rust/Clippy **1.98.1** across whole workspace/all targets;
- rustdoc warnings-as-errors;
- complete source-bound reference profile: **44/44**;
- static validation: **897 active files / 35 workspace manifests / 101 Python / 40 TOML / 104 JSON / 18 JavaScript / 796 authored Rust test attributes / 706 local links / 0 errors / 0 warnings**.

Validation-plan sizes are **44 reference / 123 Rust / 126 full**. The plan sizes are metadata, not a claim that the monolithic Rust/full orchestrator was executed as one command; the required Phase-40 native gates above were executed directly and are green. The five-target Nix `release-all` proof remains environment-blocked because this connector's `/nix/store` is read-only. No live external provider/model was contacted; the actual routing smoke used only loopback fixtures.

This is the Phase-40 review boundary. Do not begin child workers, scheduling/autonomy, or Phase 41 until explicitly requested.
