# Database design refinement v2.2 — durable LLM retry/recovery

**Status:** design-only amendment on top of database architecture v2.1 and Phase 11 / source 0.12.0. No runtime schema or application version is advanced by this document.

## Why this refinement exists

The current durable journal already records a model-start intent before provider entry and conservatively recovers unfinished calls as interrupted/unknown. That protects against blind replay, but it does not yet express the desired product behavior after a process crash:

- interactive work should visibly retain the uncertain call and let the user choose whether/how to retry;
- autonomous pipeline work should be able to resume itself using a bounded, persisted retry policy;
- every provider dispatch, including retries, must remain separately attributable and billable/uncertain;
- a retry must not erase the previous attempt, change accounts silently, rebase to unseen context, or repeat an unknown tool effect.

The detailed contract is [LLM-RETRY-RECOVERY](LLM-RETRY-RECOVERY.md).

## Adopted storage model

```text
LlmOperation (logical semantic inference)
  attempt 1 → Unknown after crash
  attempt 2 → Completed
```

The operation owns the exact original context/head/revision/purpose and a retry-policy snapshot. Attempts are immutable facts describing actual provider dispatches. A small operation/retry-schedule row is a mutable current projection backed by semantic events.

This extends—not replaces—the current `runs`, `run_events`, `invocation_receipts`, request receipts and future `commit_log` design. Physical tables may reuse those records as long as operation and attempt identities remain explicit.

## Interactive versus autonomous

| Origin | Crash recovery |
|---|---|
| Interactive/user | No automatic new provider call when an attempt is unknown. Offer reconcile, retry same context, retry current context as a new operation, or leave unresolved. |
| Autonomous pipeline | Reconcile first; if policy permits, create a new attempt after durable backoff, current authorization and budget checks. Retry state survives restart. |
| Tool/external effect | Existing hazard rules win. Unknown side effects pause; never auto-retry them from the model retry policy. |

A durable result is recovered instead of regenerated. A completed result that loses branch CAS remains durable and unattached/conflicted; it is not a reason for another model call.

## Accounting and credential rules

Each attempt independently records actor, provider account, payer, model, executor/custody, provider request identity when available and known/unknown usage. Operation-level totals expose known usage plus unknown-attempt count rather than interpreting unknown as zero.

Crash recovery never silently changes the selected account/model. A personal executor that is offline or revoked causes waiting/blocked behavior; it never falls back to a shared account. Explicit pipeline routing/fallback is a separate recorded decision, not retry magic.

## Durable scheduling

Autonomous retry needs no external queue. Store a bounded due/backoff/claim projection in SQLite and emit semantic `commit_log` events. Startup scans due work. Lost wakeups recover from stored `due_at`.

Before local multi-process writers are enabled, retry claims must become generation-fenced so two processes cannot dispatch the same next attempt.

## Pipeline relation

Autonomous sessions remain sessions produced by pipeline runs. Retry policy attaches to the exact model step/operation inside one durable occurrence. Retrying model step M1 in occurrence #42 does not create occurrence #43.

The pipeline definition can supply defaults such as max attempts/backoff/deadline/budget. Admission snapshots the effective policy so later pipeline edits do not silently alter an in-progress retry chain.

## Migration relation

Introduce operation/attempt semantics additively:

1. add operation/attempt/retry projection or equivalent fields;
2. map legacy single invocation receipts to one-operation/one-attempt only where unambiguous;
3. do not fabricate retries for historical unknown calls;
4. activate autonomous retry writes only after all supported writers understand the new semantics;
5. keep incompatible old writers read-only/refused through the normal migration capability protocol.

## Roadmap impact

- **DB-A:** unchanged — compile/correct Phase 11 first.
- **DB-B:** add operation/attempt lineage together with CAS, durable conflict result, ref events and semantic commit log.
- **DB-C/D:** bind retries to stable users, security audit, provider account/custody and budget metadata.
- **Pipeline/autonomy implementation:** persist policy snapshot and restart-safe retry schedule.
- **DB-F/G:** generation-fenced scheduler claims before supported direct multi-process mode.

## Non-negotiable recovery invariants

1. A persisted dispatch intent with no settled result means **possibly called**, never "definitely not called".
2. Retry is a new attempt ID.
3. Unknown earlier usage remains unknown.
4. Status polling/reconciliation never dispatches inference.
5. Restart/`SQLITE_BUSY`/migration/lost wakeup cannot cause another provider call by themselves.
6. Durable completed output is recovered, not regenerated.
7. Branch conflicts preserve paid output.
8. Retry uses the pinned original context unless a user/pipeline explicitly creates a new logical operation.
9. Unknown tool effects block automated continuation.
10. Personal-account failure never falls back to a shared credential.
