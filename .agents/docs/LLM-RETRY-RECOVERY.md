# Durable LLM call recovery and retry — design amendment

**Status:** adopted design amendment on top of Phase 11 / source 0.12.0 and the database-storage v2.1 architecture. This document defines recovery semantics; it does not claim the retry scheduler or schema changes are implemented in the current Rust source.

## 1. Requirement

If aw has durably admitted an LLM invocation and the process is restarted, killed, crashes, loses the provider connection, or loses the final response before durable settlement, aw must retain enough information to know that the call **may already have been made**.

Recovery depends on the origin of the work:

- **interactive/user work:** surface the unresolved attempt and offer an explicit retry; never charge another account merely because the process restarted;
- **autonomous work:** use a persisted, bounded retry policy to create a new attempt automatically when the policy permits it;
- **tool/external effects:** retain the existing stricter hazard rules; an unknown tool effect is never automatically retried simply because the surrounding model step is retryable.

The central rule is:

> **Retry is a new attempt, never replay of an ambiguous attempt.**

Every actual provider dispatch keeps its own durable attempt identity, provenance, accounting and outcome. Retrying never deletes, rewrites, or changes an older unknown attempt into a failure that is assumed to cost zero.

## 2. Existing foundation and required refinement

The current source already journals model-start intent before provider polling and reconstructs unfinished work conservatively after restart. A model start with no recorded completion is treated as unknown usage; the current runtime intentionally does not replay it automatically.

Retain that invariant, but add a durable distinction between:

```text
Logical LLM operation
    one semantic inference requested by a user/pipeline step

Provider attempt 1
    an actual provider dispatch, possibly interrupted/unknown

Provider attempt 2
    a later explicit or policy-driven retry of the same operation
```

A user-visible prompt may contain several logical LLM operations because a tool loop can call the model again after tool results. Retries are scoped to the individual logical model operation, not to the entire transcript or session.

For autonomous work, the operation belongs to the exact pipeline occurrence and step. A retry does **not** create a second schedule occurrence.

## 3. Durable operation and attempt model

Prefer extending the current run/invocation receipt model rather than inventing another conversation store. Logical types:

```text
LlmOperation {
    operation_id,
    origin: Interactive | Autonomous { pipeline_run_id, step_id, occurrence_id },
    session_id,
    branch_id,
    logical_turn_id,
    round_ordinal,
    purpose: Chat | Summary | Router | Guardian | ...,

    actor_user_id,
    requested_account_id,
    requested_model,
    payer,
    executor_id,

    source_head,
    source_revision,
    context_digest,
    tool_schema_digest,
    policy_revision,

    retry_policy_snapshot,
    state,
    created_at
}

LlmAttempt {
    attempt_id,
    operation_id,
    attempt_number,
    retry_of_attempt_id?,
    retry_reason?,

    account_id,
    model,
    executor_id,
    credential_version_ref?,
    provider_request_id?,
    provider_idempotency_key_ref?,

    admitted_at,
    dispatch_intent_at?,
    provider_acknowledged_at?,
    settled_at?,

    outcome,
    known_usage?,
    output_digest?,
    result_object_id?,
    uncertainty_reason?
}
```

The exact table names can reuse/extend `invocation_receipts`; the semantic distinction is mandatory even if implemented with fewer physical tables.

### 3.1 Immutable facts and mutable projection

Attempt history is append-only/immutable evidence. Current operation state may be a tiny mutable projection such as:

```text
Pending
Running(attempt_id)
NeedsUserDecision(attempt_id)
RetryScheduled(attempt_id, due_at)
WaitingForExecutor
Completed(result_id)
Failed
Blocked
RetryExhausted
```

Every projection transition has semantic commit-log/audit history. A mutable status row is never the only evidence that a provider dispatch occurred.

## 4. Dispatch ordering and the unavoidable crash window

Before entering provider code:

```text
Tx A:
  authorize current actor/account/context
  create/resolve logical operation
  append attempt + DispatchIntent
  reserve retry/budget/account usage envelope
COMMIT

-- no DB write transaction held --
provider invocation

Tx B:
  persist final immutable provider result or failure evidence
  settle attempt/accounting
  attach result to branch with expected-head + revision CAS if applicable
  write semantic commit-log events
COMMIT
```

`DispatchIntent` must be committed **before** the provider future is entered. Therefore after a crash:

- no dispatch intent → provider call was not admitted through this path;
- dispatch intent with no settled result → the call is **unknown**, because the crash may have happened immediately before or after provider entry;
- settled durable result → do not invoke the provider again merely because branch attachment or client delivery was incomplete.

No journaling order can remove the tiny external-system ambiguity window without provider-side idempotency/status support. Preserve the uncertainty instead of inventing certainty.

## 5. Recover provider status before retry when possible

A provider adapter may optionally expose a narrowly typed reconciliation capability:

```text
ProviderAttemptStatus::NotAccepted
ProviderAttemptStatus::Running
ProviderAttemptStatus::Completed(result/usage)
ProviderAttemptStatus::Failed
ProviderAttemptStatus::Unknown
```

Use a provider request ID/idempotency key only when the provider's documented contract supports it. Never assume OpenAI-compatible syntax implies identical idempotency or lookup semantics.

On recovery:

1. consult durable local attempt evidence;
2. if supported and currently authorized, reconcile using the exact provider/account/request identity;
3. if a completed result can be recovered, durably settle it **without another inference**;
4. if provider confirms not accepted, a new attempt may be safe according to normal policy;
5. if status remains unknown, use the interactive/autonomous policy below.

Status reconciliation is read-only. It must never become an implicit retry.

## 6. Interactive/user-session recovery

An unresolved interactive attempt appears in the session timeline as a real event, for example:

```text
Model attempt interrupted
Account: alice-openai
Model: model-X
Started: 21:03:14
Provider completion: unknown
Possible provider usage: unknown
```

Offer explicit choices:

- **Check provider status** — when adapter support exists; no new generation;
- **Retry same context** — create a new attempt for the same logical operation against the exact pinned context/account/model;
- **Retry from current branch** — create a **new logical operation** against the newly selected context; it is not another attempt of the old operation;
- **Leave unresolved / abandon** — retain history and possible usage without a new call.

The UI should warn when the previous dispatch may have incurred cost and when the same request may execute twice at the provider.

A user retry creates a new `attempt_id`, increments the attempt number, rechecks current account/model/source permissions and writes its own usage. It never overwrites the older unknown attempt or reuses its client-side receipt as though it had not happened.

## 7. Autonomous-session/task recovery

Autonomous sessions are ordinary sessions produced by a pipeline run. Retry policy belongs to the **pipeline/run step**, not to a magical session type.

Every admitted autonomous LLM operation stores an immutable snapshot of its retry policy so later pipeline edits do not silently change already-running work:

```text
RetryPolicySnapshot {
    mode: Automatic,
    max_attempts,
    retryable_outcomes,
    retry_unknown_completion,
    initial_backoff,
    max_backoff,
    jitter_policy,
    overall_deadline?,
    token_or_usage_budget?,
    monetary_budget_ref?,
    account_strategy,
    model_strategy
}
```

Recommended default behavior for autonomous model steps:

1. recover/reconcile an interrupted attempt first;
2. if still unresolved and `retry_unknown_completion` is enabled, schedule a **new attempt** after durable backoff;
3. preserve the same pinned context snapshot, purpose, selected account and model by default;
4. reserve remaining retry/budget allowance before provider entry;
5. stop when completed, blocked, policy is revoked, deadline/budget is exhausted, or max attempts is reached;
6. emit `NeedsAttention`/`RetryExhausted` rather than looping forever.

The automatic scheduler state is durable. Restarting again resumes the scheduled retry from SQLite/`CommitLog`; it does not reset attempt counters or backoff.

### 7.1 No hidden account fallback

Automatic retry does not change who pays.

- personal-device account offline → wait/backoff or fail/needs-attention according to its policy;
- personal grant expired/revoked → blocked, not shared-account fallback;
- common/server account may be used only if that exact account is the stored/authorized selection or an explicit pipeline routing rule chose it as a **new semantic selection** before the operation;
- retrying after a crash does not itself authorize rerouting to another provider/model/account.

If a pipeline intentionally permits model/account fallback, record that as a separate routing decision and operation attempt provenance; do not hide it inside crash recovery.

## 8. Retryability matrix

| Recovered state | Interactive | Autonomous |
|---|---|---|
| Intent never reached provider-dispatch boundary | Offer retry; no duplicate-dispatch warning required. | May dispatch when normal policy permits. |
| Provider explicitly confirms request was not accepted | Offer retry. | Auto-retry under policy. |
| Known transient provider failure with no external tool effect | Offer retry. | Auto-retry under policy/backoff. |
| Dispatch intent, completion unknown | Warn and require explicit user retry. | Auto-retry only if the persisted policy explicitly permits unknown-completion retries and budget remains. |
| Durable provider result exists but branch CAS lost | **Do not retry by default.** Offer attach/fork/compare/summarize. | Preserve result as conflicted/unattached; pipeline policy may resolve attachment but must not regenerate automatically. |
| Durable completed result exists, client never received it | Return the durable result. | Continue from it; no retry. |
| Account/source permission revoked | Block. | Block/needs-attention. |
| Personal executor offline | User may wait or retry later; no fallback. | Durable waiting/backoff; no fallback. |
| Unknown tool/external effect occurred | Inspection/reconciliation before any continuation. | **Never automatic side-effect retry.** Pause/needs-attention. |

## 9. Tool-loop behavior

Model-call retry and tool-effect retry are separate domains.

If a recovered model attempt had produced a complete durable tool proposal but **no `ToolStarted` fact exists**, aw may continue from the recovered proposal without another model call when all current permissions still pass.

If `ToolStarted` exists and its external outcome is unknown, the existing hazard blocks automatic continuation. An autonomous run pauses even if its model retry policy would otherwise permit another model attempt. The operator or a verified reconciliation adapter must settle the hazard first.

A new model retry must never cause a previously uncertain tool call to be silently executed again.

## 10. Branch/head behavior on retry

Every logical operation pins the model context it actually saw:

```text
source_head
source_revision
context_digest
source/dependency policy epochs
```

`Retry same context` and autonomous retries use that pinned context. They do not silently consume newer branch messages.

If another writer advances the branch while the retry is running, settlement uses expected-head + revision CAS:

- CAS succeeds → attach normally;
- CAS loses → persist the completed paid result with `attachment_status = Conflict/Unattached`;
- never discard the result;
- never silently rebase it onto unseen context;
- never auto-regenerate solely because attachment lost the race.

A user choosing **Retry from current branch** intentionally creates a fresh logical operation with a new context digest and billing attempt chain.

## 11. Durable retry scheduling

Do not require Redis/NATS/Kafka just to retry autonomous inference. Store a small current projection in SQLite, for example:

```text
RetrySchedule {
    operation_id,
    generation,
    state: Scheduled | Claimed | WaitingExecutor | Exhausted | Cancelled,
    due_at,
    last_attempt_id?,
    claimed_by_client_id?,
    claim_generation?,
    updated_at
}
```

Append semantic events for policy-relevant transitions. A scheduler wakes from `commit_log`/timer hints and also scans due work on startup. Claims must be generation-fenced before direct multi-process mode is enabled; while `owner.lock` remains, the same durable representation still makes crash recovery deterministic.

A lost wakeup is not a lost retry: `due_at` is durable.

## 12. Accounting and budgets

Each provider attempt is independently attributable:

```text
actor
provider_account
payer
credential_custody
executor
model
attempt_id
operation_id
provider request identity when known
known usage or Unknown
```

Operation-level accounting exposes:

```text
known_usage_total
unknown_attempt_count
attempt_count
completed_attempt_id?
```

Unknown prior usage is never converted to zero. An autonomous retry reserves its own allowance before dispatch. A budget must account conservatively for unknown attempts according to policy (for example, reserving the configured maximum or requiring a remaining uncertainty allowance).

Do not use an account that lacks sufficient current budget simply because the operation was originally admitted under a larger budget.

## 13. Interaction with schedules and autonomous occurrences

A scheduled automation occurrence has one durable occurrence ID. Retrying an LLM step after a crash remains inside that occurrence:

```text
schedule occurrence #42
  pipeline run R42
    model operation M1
      attempt 1 → unknown
      attempt 2 → completed
```

It does **not** create occurrence #43 or another conversation root.

Occurrence deduplication, model attempt deduplication and tool-effect receipts are separate identities. Pausing a schedule prevents future occurrences; it does not erase an already admitted retry chain. Stopping/cancelling a run prevents future automatic attempts but cannot recall a provider call already dispatched.

## 14. Commit-log and audit events

Representative semantic `commit_log` events:

```text
LlmOperationAdmitted
LlmAttemptDispatchIntent
LlmAttemptSettled
LlmAttemptRecoveredUnknown
LlmRetryOffered
LlmRetryScheduled
LlmRetryClaimed
LlmRetryExhausted
RunNeedsAttention
```

`commit_log` is for state refresh/sync. Security/audit records separately capture sensitive policy/account use such as manual retry approval, autonomous retry policy, shared credential use and administrative retry/cancel actions according to deployment audit policy. Neither log stores provider secrets.

## 15. Migration/compatibility

The retry model should be introduced additively under the rolling storage architecture:

1. expand tables/columns/event kinds while existing readers remain compatible;
2. new writers record logical-operation/attempt identities;
3. old receipts can be interpreted as one-operation/one-attempt legacy records where that mapping is unambiguous;
4. do **not** invent a retry chain for historical unknown entries;
5. activate automatic autonomous retry only after all active writers preserve the new semantics;
6. old write-incompatible clients become read-only/refuse before activation.

A model operation admitted under one DB epoch may settle under a later compatible epoch. Migration must not cause provider replay.

## 16. Recovery algorithm

On owner/scheduler startup:

```text
for each unfinished logical operation:
    load current immutable attempt chain
    determine last attempt state
    if durable result exists:
        finish/attach/reconcile without model dispatch
    else if no provider-dispatch intent exists:
        classify as not-dispatched
    else:
        classify as unknown/interrupted
        optionally reconcile provider request status

    if Interactive:
        expose durable recovery state; wait for user choice
    if Autonomous:
        evaluate persisted retry policy + current permission/account/budget
        schedule next attempt or mark Blocked/NeedsAttention/RetryExhausted
```

Recovery itself performs no provider call until it has reached the explicit retry-policy decision. Merely listing or rendering an unresolved task never dispatches inference.

## 17. Required acceptance tests

Before claiming this behavior:

- crash before dispatch intent → no provider call is inferred;
- crash after dispatch intent but before provider entry → recovered as unknown, never falsely completed;
- crash after actual provider entry → unknown attempt retained;
- crash after result persistence but before client delivery → result returned without another call;
- crash after completed result with branch CAS conflict → result remains durable/unattached;
- interactive unknown attempt never automatically retries after restart;
- user retry creates attempt N+1 and retains attempt N as unknown;
- user `retry current branch` creates a new logical operation rather than mutating the old attempt chain;
- autonomous retry survives repeated process restarts and does not reset attempt count/backoff;
- autonomous retry obeys max attempts, deadline and usage/spend budgets;
- personal-device offline/revoked path never falls back to a shared account;
- provider status reconciliation cannot itself dispatch inference;
- `SQLITE_BUSY`, lost wakeups and migration barriers never repeat provider calls;
- unknown tool effect pauses autonomous retry and remains a hazard;
- complete durable tool proposal with no started effect can be recovered without duplicate model invocation where compatible;
- same-branch race preserves the paid losing result;
- retry uses pinned original context unless a new logical operation is explicitly requested;
- retry/account changes are independently attributed and auditable;
- schedule occurrence identity is unchanged by internal model retries;
- old history/migrations remain readable without inventing historical retries.

## 18. Implementation placement

Database primitives belong in `aw-store`/the future `ReceiptStore` + `CommitLog` + scheduler projection. Generic attempt/operation types may live in `aw-agent-core` if they do not introduce database/network dependencies. Pipeline retry-policy definitions belong in the pipeline SDK/runtime when implemented. UI renderers consume authorized projections and issue typed retry/reconcile commands; they never decide authority or reconstruct provider credentials.

Keep the fast local path cheap: a normal successful model call adds a bounded operation/attempt record and a few semantic events. No retry scheduler, provider status polling or autonomous timer is initialized when no unresolved/autonomous work exists.

## 19. Phase 34 implementation note — process execution lease and real pre-attachment crash boundary

Phase 34 strengthens this contract without changing the retry authority model.

For persistent stores, the exact provider dispatch path now acquires a generation-fenced local execution lease inside the same transaction that durably records `InvocationStarted`. Provider code is entered only after that transaction commits. Normal `InvocationFinished` releases the exact lease. If the process dies first, owner fencing turns the lease into a hazard; startup recovery first records the attempt as unknown/interrupted plus its explicit retry/no-replay projection, then reconciles the process-coordination hazard. Lease expiry/process death never means “provider definitely did nothing,” and coordination reconciliation never authorizes a retry by itself.

Phase 34 also makes “persist paid result before branch CAS” a real crash boundary rather than only statement ordering inside one transaction:

1. Transaction A persists the final immutable result, attempt/operation settlement and recovery/accounting facts with `attach_status=pending`, then commits.
2. Transaction B separately attempts writer-epoch + branch head/revision attachment CAS.
3. If the process dies between those transactions, startup attaches/reconciles the existing durable result without another model call.
4. If the CAS no longer matches, the paid result becomes a durable attachment conflict and is not regenerated.

SQLite Busy/Locked, maintenance, restart and restore-generation fencing remain database/control-plane outcomes only. None of them dispatch inference or convert an unknown attempt into a retry.

## 20. Phase 50 guardian review recovery

Guardian review is a distinct paid provider effect and follows the same no-replay rule as routing/planning.

- `admitted` means no guardian provider intent was durably crossed; owner restart marks it `interrupted` rather than silently starting it later.
- `started` means provider intent exists; owner restart marks it `unknown` and never replays the guardian call.
- `pass`, `deny`, `waiting_human`, and `needs_evidence` are settled verdicts and survive restart.
- a waiting human approval is a local durable policy state, not a provider effect, so it remains waiting across restart.
- after `pass` or explicit human approval, `review_release` reconstructs and verifies the existing source join without inference. A crash between verdict settlement and release therefore does not require or permit another guardian call.
- changing the parent account/model invocation identity invalidates release; human approval never authorizes redisclosure to a new provider destination.

Guardian attempts consume the same delegation-root model-attempt/deadline/usage envelope as planner/worker calls. Unknown usage is not invented and unknown completion is not treated as a free retry.
