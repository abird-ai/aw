# Phase 43 — root delegation envelopes and shared subagent budgets

**Status:** authorized 2026-10-05 by the user after Phase 42 review.

## Boundary

Phase 43 adds the durable **root delegation/budget foundation** required before recursive subagents, arbitrary fan-out, child routing, reviewer teams, or schedules.

It does **not** enable recursive child spawning. Child Agents still never receive the `subagent` tool.

Phase 43 makes the existing Phase-41/42 parent → direct-child primitive consume one shared durable envelope for the exact parent turn.

The envelope bounds:

- total child allocation attempts created from the root turn;
- concurrently active child Agents;
- child model invocation attempts;
- elapsed delegation deadline;
- delegation depth.

Phase 43 stores `max_depth = 1` and all admitted children have `depth = 1`. This is intentional. Phase 44 may explicitly authorize recursive delegation and increase/use remaining depth; Phase 43 must not make recursion possible merely because the schema is future-ready.

No child/account/model/tool/root/schedule authority comes from these budget fields.

## Host-selected limits

Native single-user `--subagents` gains host-selected limits:

    --subagent-max-created N
    --subagent-max-active N
    --subagent-max-model-attempts N
    --subagent-deadline-secs N

Defaults:

- max created children: 32;
- max active children: 4;
- max child model attempts: 64;
- deadline: 3600 seconds;
- max depth: fixed at 1 in Phase 43.

Hard validation keeps values bounded and requires `max_active <= max_created`.

These flags configure admission limits only. They do not select provider/model/account/payer/tool authority and are absent from the model-facing `subagent` arguments.

## Root identity

A delegation root is scoped to one exact root parent:

- parent Session;
- parent branch;
- parent turn;
- stable creating user.

The first subagent allocation for that exact parent turn creates the root envelope. Later direct-child allocations in the same turn reuse it only when the stored limits exactly match current host limits.

Changing host limits during one active parent turn is a configuration conflict, not an in-place budget mutation.

A later parent turn receives a different root envelope.

## Schema v27

Phase 43 advances SQLite to v27.

New tables:

- `delegation_roots`;
- `delegation_child_allocations`;
- `delegation_model_attempts`.

Additive nullable provenance columns are added to historical Phase-41/42 tables:

- `subagent_sessions.delegation_root_id`;
- `subagent_sessions.delegation_allocation_id`;
- `subagent_sessions.depth`;
- `subagent_execution_bindings.delegation_root_id`;
- `subagent_execution_bindings.delegation_allocation_id`.

Historical rows remain valid and nullable. New Phase-43 application writes always supply the root/allocation/depth.

## Root counters

A root records immutable limits plus monotonic/accounting counters:

- `children_created`;
- `active_children`;
- `model_attempts_started`;
- `model_attempts_finished`;
- known input tokens;
- known output tokens;
- calls without complete provider usage.

Provider-reported token usage is evidence, not billing verification. Missing usage is recorded as unknown/incomplete; never invent zero tokens or currency.

The root state is `active | exhausted | cancelled`.

Phase 43 does not introduce price/currency accounting.

## Child allocation admission

Before any managed-worktree creation or child provider entry:

1. validate exact parent owner/Session/branch/active turn;
2. get-or-create the exact root envelope;
3. check root state and deadline;
4. check `children_created < max_children_created`;
5. reserve one durable `delegation_child_allocations` row;
6. increment `children_created` atomically.

The allocation state is:

    reserved
      -> bound
      -> running
      -> completed | failed | cancelled | interrupted

or:

    reserved -> failed

An allocation attempt consumes one created-child slot even when later worktree/provider preparation fails. This prevents repeated failing preparation from resetting the root budget.

The allocation stores the intended depth. In Phase 43 depth must equal 1 and must be <= root max depth.

For Phase-42 tool-enabled children, the managed-worktree `planned` binding is created only **after** this allocation reservation. The v27 execution-binding columns link that external effect to the same root/allocation.

If worktree provisioning fails or becomes unknown before child Session creation, the allocation is terminally failed; the created-child counter is not decremented.

## Active-child admission

Creating the child Session binds its reserved allocation but does not yet consume an active slot.

Immediately before the child Agent becomes running:

- revalidate root state/deadline;
- require `active_children < max_active_children`;
- atomically increment `active_children`;
- transition allocation `bound -> running`;
- transition the existing subagent relation to running.

Child settlement decrements `active_children` exactly once and transitions the allocation to the same terminal outcome.

Owner restart that interrupts a running child also decrements the durable active count exactly once.

No process-local counter is authoritative for the root budget.

A small process-wide safety cap may remain as an implementation backstop, but it cannot increase a durable root limit.

## Model-attempt admission

Every child provider invocation round must reserve from the same root envelope **before provider dispatch**.

The child durable journal is wrapped by a budget journal.

On `InvocationStarted(turn_id, ordinal, identity)`:

1. require the child relation/allocation belongs to the root;
2. require root active/not past deadline;
3. require `model_attempts_started < max_model_attempts`;
4. insert a unique `delegation_model_attempts(root, child, turn, ordinal)` reservation;
5. increment `model_attempts_started`;
6. only then delegate to the ordinary Session journal, which persists the existing provider invocation intent;
7. only then may the Agent enter the provider.

The reservation is idempotent only for the exact same root/child/turn/ordinal/account/model identity. A conflicting duplicate fails closed.

If ordinary journal/provider preparation fails after reservation, that attempt remains consumed. Budget accounting is conservative and never enables a free retry.

## Model-attempt settlement

The ordinary Session journal's `InvocationFinished` record is committed first so a paid/provider result is never sacrificed to budget-accounting failure.

Then the budget journal settles the matching delegation attempt:

- state `finished`;
- outcome;
- provider-reported input/output tokens when complete;
- missing/incomplete usage counter otherwise;
- root `model_attempts_finished` and known token counters.

A duplicate exact finish is idempotent.

If the process dies after provider entry/result but before budget settlement, restart marks the still-started budget attempt `unknown`; the attempt remains counted as started and no provider replay occurs.

## Deadline

The root stores an absolute deadline established from the host-selected duration at first allocation.

Child allocation and each model-attempt reservation recheck current time against it.

The child Agent's request timeout is also clamped to the remaining root duration when the child starts, so an existing child turn cannot intentionally outlive the envelope by using the ordinary longer request timeout.

Deadline expiry prevents new child/model admissions. In-flight provider/file effects retain ordinary unknown-effect/reconciliation semantics; Phase 43 does not pretend a deadline rolls them back.

## Phase-42 worktrees

Phase 42 remains unchanged:

- tool-enabled direct children use isolated managed Git worktrees;
- `off` children may use the tightly scoped Phase-41 model-only same-root path;
- read-only/workspace tools remain host-selected;
- bash/trusted children remain forbidden;
- worktrees remain retained for human review and explicit cleanup.

Phase 43 only links the execution binding to the root/allocation and ensures allocation is reserved before the external Git effect.

## No recursion yet

Phase 43 deliberately keeps:

- child Agents without the `subagent` tool;
- max admitted depth = 1;
- parent/root identity inherited from the top-level parent turn only;
- no model-selected fan-out count;
- no child-created root envelope;
- no child routing.

This proves budget/depth accounting before recursion.

## Observability

`aw subagent list` includes visible delegation roots and their counters/limits.

Add:

    aw subagent budget SESSION_OR_CHILD_SESSION

The command resolves the relevant root envelope and shows:

- state/deadline;
- children created/max;
- active/max;
- model attempts started/max;
- finished attempts;
- known input/output tokens;
- incomplete-usage count;
- max depth/current child depth evidence.

The fullscreen Agent-family sidecar shows a compact root summary when the current Agent belongs to a Phase-43 root, for example:

    Subagents  2/32 · active 1/4 · calls 3/64

Budget metadata is host/UI state and is not automatically injected into model context.

## Recovery

Owner startup:

- Phase-41/42 relation/worktree recovery remains;
- running child allocations become interrupted;
- root active count is decremented by the exact interrupted allocation count;
- started model-attempt reservations become unknown;
- started attempts remain consumed;
- no root counter is reset;
- no child/model/Git effect is automatically replayed.

Counter repair must be conservative and derived only from durable linked rows, never from guessed process liveness.

## Deferred after Phase 43

Still explicitly out of scope:

- recursive child spawning;
- configurable depth > 1 in actual execution;
- model-selected fan-out;
- child routing / alternate account/model assignment;
- per-child sub-budgets;
- monetary price/currency budgets;
- automatic patch import/merge;
- trusted/bash child tools;
- schedules/cron/autonomy/event triggers;
- reviewer/synthesis teams;
- cross-machine child placement;
- browser/shared participant spawning.

Those require another explicitly authorized phase.

## Validation requirements

At minimum:

- v26 -> v27 identity-preserving migration;
- exact root uniqueness per parent Session/branch/turn;
- host-limit mismatch in one root fails closed;
- child allocation reservation is durable before managed-worktree Git effect;
- failed provisioning consumes created-child allocation;
- max-created rejection before Git/provider;
- max-active atomic enforcement across foreground/background children;
- active slot released once on completed/failed/cancelled/interrupted/restart;
- model-attempt reservation occurs before ordinary `InvocationStarted` provider intent;
- attempt limit blocks before provider dispatch;
- exact duplicate reservation idempotent; conflicting identity fails;
- provider-result journal survives budget settlement failure;
- usage counters preserve unknown usage honestly;
- restart started-attempt -> unknown/no replay;
- deadline blocks new children/model rounds and clamps child turn timeout;
- Phase-42 read-only/workspace real-Git tests remain green;
- child still receives no recursive subagent/bash;
- sidecar/CLI budget projection;
- actual compiled fake-provider Phase-43 budget smoke;
- Phase-42/41/40/39/38 checkers remain green;
- tiny/no-default/headless/routing cuts remain green;
- whole-workspace/all-targets, doctests, rustdoc, Clippy, reference/static gates;
- pinned Nix/release claims only when the Nix store is writable.
