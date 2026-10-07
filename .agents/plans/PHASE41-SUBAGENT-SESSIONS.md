# Phase 41 — bounded subagent child Sessions

**Status:** authorized 2026-10-04 by the user after Phase 40 review.

**Implementation boundary:** Phase 41 adds first-class native subagents as bounded child Agent Sessions. It does **not** add schedules, event/webhook triggers, recursive swarms, arbitrary DAG/fan-out planning, child routing, multi-model reviewer teams, or writable shared-root child tools.

## Product vocabulary

- **Agent** is the product-facing interactive thread/view.
- **Session** remains the durable storage/provenance object.
- A titled Agent renders as `Agent: <Session title>`.
- A **subagent** is an ordinary durable child Session related explicitly to one parent Session/branch/turn.
- The internal serialized workspace target remains `Conversation` for backward-compatible saved layouts; this is not the product-facing label.

The pre-Phase-41 adaptation at `c6fe57c` added `/title TITLE`, backend-neutral durable Session title updates, Agent-facing tabs, and the one-line fullscreen composer. `/title-generate` remains reserved: when implemented it is an explicit tool-free model operation, defaults to the current Agent account/model, and may use a separately configured already-authorized title model. Manual `/title` never invokes a model.

## Why child Sessions, not hidden tasks

A child is an **ordinary durable Session** in the same Workspace because that reuses the existing immutable history, branch CAS, provider identity, paid-result preservation, recovery, export, and UI navigation contracts.

The Phase-41 relation records only secret-free orchestration evidence:

- parent Session;
- parent branch;
- exact parent head/revision;
- exact parent turn;
- child Session;
- foreground/background mode;
- lifecycle state;
- task digest;
- exact invocation identity metadata;
- effective child tool mode;
- timestamps/error.

No API key, token, endpoint URL, shell command, arbitrary system prompt, routing strategy, cron expression, or webhook is stored in the relation.

## Admission and authority

The `subagent` tool is available only when all are true:

1. native single-user owner;
2. durable SQLite Session exists;
3. user explicitly launched with `--subagents`;
4. current model supports tool calls and proposes the registered `subagent` tool.

`--tools` does **not** grant subagents. `--subagents` is a separate opt-in because a child model call may create additional provider usage/billing.

Tool arguments are intentionally tiny:

```json
{
  "task": "bounded task text",
  "title": "optional child title",
  "mode": "foreground | background"
}
```

The model cannot choose actor/user, provider account, payer, credential custody, provider endpoint, model, Workspace/root, tool mode, routing strategy, child system prompt, schedule, or recursive depth.

`ToolContext` carries the exact already-authorized `InvocationIdentity` and durable `InvocationScope` of the parent round. The subagent service reselects that exact account/model and requires equality with the parent lease before child admission. Child dispatch uses that pinned `ProviderSelection` through `prompt_as`; a grant/account change before dispatch fails closed instead of silently changing destination/payer/custody/model.

The relation is admitted only by the parent Session owner and only while the exact parent branch has the exact active parent turn. Shared-draft membership or generic `members.can_append` never grants subagent spawning authority.

## Context disclosure

A Phase-41 child starts with isolated conversation history.

It receives the parent Session system instructions, a fixed bounded subagent-role suffix, and the explicit delegated `task`.

It does **not** receive the parent transcript, parent tool trace, Files/Git views, PTY output, shared drafts, or arbitrary current workspace UI state.

If the parent wants the child to know something, that data must be written into the task explicitly or later transferred through an explicit context mechanism.

## Local tools and same-root concurrency

**Phase-41 child Agents are model-only.** Effective child `tool_mode` is `off`, regardless of the parent Host's local tool grant.

This is deliberate. Existing aw durability allows one ordinary active execution per physical filesystem root so uncertain/mutating effects cannot race silently. Phase 41 does not weaken that invariant for local tools.

A currently parent-linked model-only child may run concurrently with its parent because:

- the child advertises zero tools;
- the child has no filesystem/shell effect path;
- the durable relation is already `running`;
- stored invocation identity exactly matches the child dispatch identity;
- the parent branch/run is still active;
- no unrelated root hazard exists;
- no unrelated running root execution exists.

Only after those checks does the child model run receive a synthetic internal run key `subagent:<physical-root-key>:<child-session>`. Ordinary Sessions keep the physical root key and the historical one-running-run-per-root index remains intact. The synthetic key cannot hide child filesystem effects because Phase-41 children have no local tools.

Writable/read-tool children are deferred until aw can give them an isolated ManagedWorktree/ManagedDirectory/other explicit execution binding or another reviewed coordination scheme.

## Foreground and background

### Foreground

The parent tool waits for the child turn.

- parent cancellation cooperatively cancels the child;
- completed child answer is returned as bounded tool data;
- large answers are bounded for the parent tool result while the complete child transcript remains durable;
- failed/cancelled child state remains inspectable in the child Session.

### Background

The tool durably admits the child Session/relation, marks the relation running, starts a bounded in-process worker thread with its own current-thread Tokio runtime, and returns the child Session/relation immediately.

Limits:

- at most 4 active background children per native service;
- at most 32 child relations per parent Session in Phase 41;
- no recursive child spawning because child Agents never receive the `subagent` tool.

If the aw owner exits/crashes while a child is admitted/running, restart changes the relation to `interrupted`. The child Session journal independently preserves any provider dispatch ambiguity/paid result. Nothing automatically replays the child model request.

## UI

### Sessions Explorer

In Tree mode:

```text
v Parent agent
  `- main
  `- v● Research child · background
      `- main
```

Direct Phase-41 child Sessions are nested under the parent Session. They are not also shown as unrelated roots. List mode remains a flat searchable branch list.

### Agent sidecar

When the current Agent belongs to a subagent family, a compact floating **Subagents** view is projected at the top-right of the Agent content area.

- parent view shows direct child Agents and durable state;
- child view also shows a parent switch target;
- rows switch to the selected Session's `main` branch;
- state glyphs distinguish admitted/running/completed/failed/interrupted/cancelled;
- the sidecar is transient UI projection, not persisted layout geometry and not model context.

## Recovery

Owner restart marks `admitted`/`running` subagent relations `interrupted`, does not delete child Sessions, does not infer that a provider effect did or did not happen, does not automatically replay, and leaves the child Session's ordinary durable run/recovery machinery authoritative.

A later explicit retry/re-run must be a new reviewed action.

## Schema

Phase 41 is SQLite **schema v25**.

New table:

- `subagent_sessions`

Phase 40 remains historical schema v24. All historical tags stay immutable.

Trusted-local direct mode still requires the exact current schema and still never migrates/restores. Phase-41 spawning itself is native owner-only; direct/shared/browser surfaces do not silently gain spawn authority.

## Deferred after Phase 41

Explicitly not in this phase:

- recursive subagents / swarms;
- model-selected fan-out counts;
- generative DAG/planners;
- child routing or independent account/model selection;
- child local read/write/bash tools;
- automatic worktree/directory creation;
- schedules/cron;
- background autonomy loops;
- event/webhook triggers;
- automatic reviewer/synthesis teams;
- cost/budget dashboards;
- shared-browser participant spawning;
- native shared-draft participation;
- cross-machine child placement.

Any of these needs a later explicitly authorized phase.

## Validation requirements

At minimum:

- schema v25 migration and v24→v25 migration test;
- owner-only exact active parent-turn admission;
- child is a normal same-Workspace Session;
- exact invocation identity persisted and reused;
- Phase-41 `tool_mode=off` enforcement;
- model-only same-root synthetic run-key proof;
- foreground fake-provider child result test;
- background fake-provider independent settlement test;
- owner restart → interrupted relation without replay;
- Agent sidecar and Session-tree render tests;
- tiny/no-default build remains free of subagent runtime;
- Phase-40 routing/draft/pipeline/store recovery regressions remain green;
- actual compiled fake-provider Phase-41 smoke;
- source/reference/static validation;
- whole workspace/all-targets and warnings-as-errors;
- pinned release toolchain only if the Nix store is writable; never overclaim it otherwise.
