# Phase 67 — Governed durable pipeline schedules

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `724b4571d8875be2c6eb2d93463a83f006b3b89b` (`phase-67`)
**Version:** 0.15.0
**SQLite schema:** v33
**Skipped phases:** Phase 64 and Phase 65 remain intentionally unused by explicit user request.
**Authority expansion:** bounded autonomous timing over authority already granted by an existing pipeline task binding; no new provider/tool/shell/webhook authority.

## Goal

Add the first real governed schedule/occurrence layer without creating a second execution engine.

A Phase-67 schedule points at one existing immutable **active caller-owned user-scoped task pipeline binding**. The schedule stores fixed input, a bounded interval, an overlap policy, durable next-due state and immutable occurrence identities. When an occurrence is due, it is generation-fenced, admitted through the existing idempotent pipeline-run API, and then executed through the existing pipeline/Agent runtime.

## Scope

Phase 67 intentionally supports only interval schedules:

```text
60 seconds <= interval <= 30 days
```

and only existing user-scoped task bindings.

Session-scoped scheduling is deliberately excluded because it would require pinning/revalidating an exact Session branch/head disclosure target. Cron expressions, webhooks, arbitrary events, shell commands and model-authored schedules are also excluded.

## CLI

```text
aw schedule define NAME --binding BINDING --every-seconds N
                   [--delay-seconds N] [--overlap serial|skip] [INPUT...]

aw schedule list
aw schedule show SCHEDULE
aw schedule pause SCHEDULE --expected-generation N
aw schedule resume SCHEDULE --expected-generation N
aw schedule occurrences SCHEDULE
aw schedule run-due [normal provider/tool/target options]
```

`run-due` handles at most one admitted occurrence per invocation.

A future long-lived owner may repeatedly invoke the same typed scheduling primitive; Phase 67 does not add a second hidden scheduler/executor loop.

## Durable model

Schema v33 adds:

- `pipeline_schedules`;
- `pipeline_schedule_occurrences`.

A schedule pins:

- stable owner user;
- exact pipeline;
- exact immutable pipeline revision;
- exact active user-scoped task binding;
- fixed UTF-8 input;
- interval;
- overlap policy;
- generation;
- next due time;
- next occurrence number.

The database has an insertion trigger requiring the binding to match the exact owner/pipeline/revision and to be an active user-scoped task binding.

Schedule authority fields are immutable. Pause/resume mutate only state + generation via expected-generation CAS.

## Occurrence identity and no replay

Each occurrence has a durable monotonically increasing occurrence number.

The pipeline request ID is deterministic:

```text
sched:<schedule_id>:<occurrence_no>
```

The claim is persisted **before** pipeline admission.

Pipeline admission then calls the existing `pipeline_admit_manual()` path. Its existing request-ID idempotency guarantees that a crash/retry after pipeline admission resolves to the same run rather than creating a second run.

Occurrence identity, pipeline attempt identity and any internal LLM retry identity remain separate.

## Claims

A due occurrence is claimed with:

- occurrence ID;
- schedule generation;
- claimant/client ID;
- deterministic request ID.

A different client or stale generation cannot admit the claim.

Only one earliest eligible occurrence is returned per claim call.

## Overlap

`serial`:
- if an earlier occurrence is still claimed/running/waiting approval, leave the schedule due;
- do not admit a second occurrence.

`skip`:
- persist a distinct `skipped` occurrence with reason;
- advance the next due time;
- never hide that the due occurrence was skipped.

Before overlap evaluation, an admitted occurrence whose authoritative pipeline run is already terminal is refreshed to completed/failed.

## Execution

`aw schedule run-due`:

1. opens the existing SQLite authority store;
2. claims one due occurrence;
3. admits the occurrence through the exact existing pipeline binding/revision;
4. releases the scheduling store handle;
5. runs that exact pipeline run through the existing `PipelineInvocation::Resume` path;
6. reopens the store and settles the occurrence from authoritative pipeline state.

No provider/tool dispatch occurs from schedule storage code.

## Pause/resume

Pause prevents future occurrence claims. It does not cancel, erase or rewrite an occurrence already admitted.

Resume re-enables future due claims without resetting occurrence numbering or consumed work.

Both operations require the exact current schedule generation.

## Non-goals

Phase 67 does not add:

- cron syntax;
- webhook/event ingestion;
- arbitrary shell commands;
- arbitrary scheduled prompts outside a pipeline binding;
- model-created or model-edited schedules;
- Session-scoped schedules;
- cross-machine schedule placement;
- parallel occurrence fan-out;
- monetary budget types;
- new provider/account/tool grants;
- automatic retry of unknown external effects;
- filesystem indexing/LSP;
- Phase 64/65 work.

## Validation target

1. schema v32 -> v33 migration and foreign-key check are green;
2. direct-current-schema trusted mode requires v33;
3. schedule creation requires exact active user task binding;
4. generation-CAS pause/resume is enforced;
5. due claims are claimant/generation fenced;
6. deterministic occurrence request IDs make pipeline admission idempotent;
7. serial overlap cannot admit a second active occurrence;
8. skip overlap persists an explicit skipped occurrence;
9. terminal pipeline runs refresh occurrence state;
10. actual CLI define/list/show/run-due/occurrences round trip is green on a deterministic pipeline;
11. run-due uses the existing pipeline executor, not a second provider/tool loop;
12. schema and historical pipeline/retry invariants remain green.

## Implementation evidence before tag

- Phase 67 checker: **71/71**;
- focused schedule store tests: **2/2**;
- actual-binary deterministic schedule smoke: **6/6**;
- full native `aw`: **256/256**;
- minimal/no-default warning-denied native: **82/82** plus inherited library/provider tests;
- whole-workspace/all-target compile: green;
- Clippy `-D warnings`: green;
- doctests/rustdoc warnings-as-errors: green;
- static validation: **1035 files / 1009 Rust test attributes / 739 links / 0 errors / 0 warnings**.

The source-bound reference profile reaches Phase 67 cleanly; the only remaining red step before final consolidation is the intentionally stale comprehensive-handoff authority, which will be regenerated after Phase 68.

## Next gate

continue to explicitly authorized Phase 68 release hardening.

Do not infer Phase 69 automatically.
