# Phase 12 — first-class durable sessions, LLM attempts and recovery

**Source candidate 0.13.0. Rust/Cargo/Nix execution is still pending.** This phase implements schema3 storage/runtime source for durable LLM operation lineage, crash recovery hints, semantic commit events, branch-ref events, explicit interactive retries and generation-fenced autonomous retry claims. `owner.lock` remains in place; direct multi-process DB writers are **not** enabled by this phase.

## Core invariant

A potentially paid provider call is never forgotten and an ambiguous attempt is never silently replayed.

```text
logical operation op_...
  attempt 1 / run r_... : provider_started -> unknown after crash
  attempt 2 / run r_... : explicit user or persisted autonomous retry -> completed
```

Attempts are immutable lineage. Retry creates another attempt under the logical operation; it does not rewrite attempt 1 or pretend its usage was zero.

## Schema 3

Migration `003_durable_runtime.sql` adds:

- `store_meta` with permanent `store_id`, schema epoch and restore generation;
- append-only `commit_log` semantic events;
- append-only `branch_ref_events`;
- `llm_operations` for logical user/autonomous operations;
- `llm_attempts` for each admitted real execution;
- retry lineage columns on `runs`, including original head/revision and attachment status;
- durable `retry_schedules` with generation-fenced claims;
- durable `recovery_hints` consumed by CLI/web/native surfaces.

Legacy schema2 runs are migrated as one-attempt operations without inventing retry authority. A legacy running invocation with a started receipt is classified `unknown`; migration does not call a provider.

## Admission and settlement

New durable prompts record the original branch head/revision and a logical operation before execution. Provider intent still precedes provider polling. Settlement persists the completed/failed turn first and then conditionally attaches it to the branch. A lost branch CAS results in:

```text
execution outcome = completed/failed/...
attachment       = conflict
```

The result node and billing/provenance remain durable. The user can later fork/compare/summarize it. The model is never automatically rerun just because attachment lost a race.

## Crash recovery

Exclusive-owner startup reconstructs running work as before, but now also:

- marks unfinished provider attempts `unknown` when dispatch may have occurred;
- retains every invocation receipt;
- persists a recovery hint with the exact logical operation and original head/revision;
- interactive sessions expose `retry_available` rather than dispatching;
- autonomous policies may persist a future `due_at` retry when there are no unresolved external effects;
- possible tool/process effects remain blocking hazards and prevent automatic retry.

## Interactive retry

The web and CLI can choose a durable interrupted operation. A same-context retry must run from the operation's original pinned head. The browser therefore forks from that exact head when necessary before arming the retry. The next prompt is bound to the existing logical operation and becomes attempt N+1.

Retrying from the *current* branch is intentionally a new logical operation because the model sees different context.

## Autonomous retry

`RetryPolicy` can be configured by an autonomous host before a run. Durable policy stores bounded attempts, whether unknown completion may be retried, and exponential-backoff bounds. Recovery can schedule a retry and `claim_due_autonomous_retry` uses a generation-fenced SQLite claim so one future worker owns the dispatch opportunity. Claiming itself is not inference; the claimed operation must still be bound to the next admission.

This phase provides the durable scheduler/claim primitives. The generalized pipeline/autonomous runner that drives them remains a separate application layer; no hidden background model worker starts in ordinary chat mode.

## Semantic UI hints

`Snapshot.recovery_hints` returns bounded, authorized durable hints. The web UI renders:

- Retry same context;
- Retry from current branch;
- inspect effects when effects are uncertain;
- attachment-conflict guidance;
- autonomous retry scheduled status.

Hints contain operation IDs and bounded action metadata, never provider keys.

## Semantic commit log

Durable lifecycle mutations append bounded application events in the same transaction as their state changes. Initial event types include session/branch creation and advancement, attempt admission/settlement, membership changes and retry scheduling/claiming. Wake-up transport remains optional; cursor replay from `commit_log` is the durable truth.

Raw SQLite WAL is not an application event protocol.

## Limits and deferred work

- `owner.lock` still excludes a second database owner.
- schema3 does not implement OIDC, SecretStore, cross-store sync or direct local multi-writer mode.
- automatic retry is blocked when tool/external effects may be unknown.
- provider-side status reconciliation remains provider-specific future work; unknown means unknown.
- retry policies never authorize account fallback. Current account/device permission is rechecked at every new attempt.

## Acceptance on a compiler host

Run the complete source-bound gate and additionally exercise:

1. kill after `InvocationStarted`, then restart;
2. verify `unknown` provider attempt + nonzero/unknown usage semantics;
3. click/issue same-context retry and verify attempt 2 under the same operation;
4. retry from current branch and verify a new operation;
5. configure an autonomous policy, crash, verify durable backoff survives restart and only one claim wins;
6. start a tool effect, crash, verify no automatic retry is scheduled;
7. force branch movement before settlement, verify result persists with `attach_status=conflict` and no new provider call;
8. verify recovery hints survive browser refresh/restart and contain no credential material;
9. verify `commit_log` and `branch_ref_events` are immutable and cursor-readable.
