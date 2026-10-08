# Phase 44 — bounded recursive subagents and shared-root fan-out

**Status:** authorized 2026-10-05 by the user after Phase 43 review.

## Boundary

Phase 44 enables bounded recursive delegation on top of the Phase-43 root envelope.

A child Agent may receive the same bounded `subagent` tool when:
- the root host explicitly configured `--subagent-max-depth > current_depth`;
- the current child is a running member of that exact root;
- the descendant reuses the same actor/account/model lease;
- the descendant consumes the same root created/active/model-attempt/deadline counters;
- the descendant inherits the same or narrower local file-tool authority.

Phase 44 supports:
- recursive chains;
- multiple direct children from one Agent;
- bounded sibling fan-out through multiple subagent calls, including concurrent background children subject to the shared active/root/process caps.

Phase 44 does **not** add:
- child routing or alternate provider/account/model selection;
- per-child reset/sub-budgets;
- trusted/bash child tools;
- automatic reviewer/synthesizer roles;
- automatic join/reduction semantics;
- schedules/cron/autonomy/event triggers;
- automatic Git merge/stage/commit/import;
- cross-machine descendants;
- browser/shared participant spawning.

## Host-selected max depth

Add:

    --subagent-max-depth N

Default: 1.
Hard maximum: 4.

Default 1 preserves Phase-43 behavior exactly.

The model-facing `subagent` schema still contains only:
- task;
- optional title;
- foreground/background mode.

The model cannot choose depth, max depth, account/model, routing, tool mode, filesystem root, Git ref, worktree path, deadline, child counts, or budgets.

## Shared root envelope

Every descendant uses the same Phase-43 root:
- exact original root Session / branch / turn;
- one created-child counter;
- one active-child counter;
- one child-model-attempt counter;
- one deadline;
- one stable initiating user.

Recursion never creates a new root envelope.

Depth increases by exactly one from the durable parent child allocation.

A descendant allocation records:
- root ID;
- direct parent Session / branch / turn;
- direct parent allocation ID;
- authoritative recursive depth;
- normal Phase-43 lifecycle state.

Admission requires:
- direct parent child relation is running;
- direct parent allocation is running;
- direct parent relation/allocation belong to the same root;
- next depth = parent depth + 1;
- next depth <= root recursive max depth;
- all shared root created/deadline constraints pass.

## Schema v28

Phase 44 advances SQLite to v28 additively.

Phase 43 intentionally constrained legacy columns to depth 1. To preserve historical rows and avoid destructive table rebuilds across cyclic Session/worktree foreign keys, v28 adds authoritative recursive columns:

- `delegation_roots.recursive_max_depth`, default 1, bounded 1..4;
- `delegation_child_allocations.recursive_depth`, default 1, bounded 1..4;
- `delegation_child_allocations.parent_allocation_id`, nullable self-reference;
- `subagent_sessions.recursive_depth`, default 1, bounded 1..4.

The historical v27 `max_depth` / `depth` columns remain compatibility evidence and stay equal to 1. New Phase-44 application logic uses the recursive columns as authority.

New triggers enforce:
- root recursive max depth is immutable;
- root child allocation at depth 1 has no parent allocation;
- descendant allocations have a same-root parent allocation whose recursive depth is exactly one lower;
- new subagent relation recursive depth equals its linked allocation recursive depth;
- recursive depth is immutable after child relation creation.

Existing v27 rows migrate to recursive max/depth = 1.

## Root creation/reuse

Top-level child admission creates/reuses the root exactly as Phase 43, with host limits including `recursive_max_depth`.

Changing max depth or another root limit during an active parent turn conflicts rather than mutating the existing root.

## Descendant service lineage

The native subagent service becomes lineage-aware.

Top-level Host service:
- root lineage = none;
- current depth = 0.

Each created child receives a service clone bound to:
- root ID;
- current allocation ID;
- current recursive depth.

The clone shares:
- ProviderAccounts registry;
- SessionStore;
- host budget configuration;
- file-tool mode;
- trusted Git path/state root;
- process-wide background safety counter.

The child receives the `subagent` tool only when:
- current depth < root recursive max depth.

At max depth, the tool is absent rather than advertised then denied.

## Account/model authority

Descendants do not route.

The direct parent child’s exact stored InvocationIdentity must equal the descendant tool context identity before descendant admission.

The descendant Agent is built from the exact currently revalidated parent ProviderSelection.

No recursion step may widen:
- actor;
- account;
- payer;
- credential custody;
- executor;
- model;
- destination.

Phase 44 has no child routing strategy argument.

## File-tool authority and worktrees

Descendant file-tool mode is the same host-selected mode as the root in Phase 44:
- off -> off;
- read-only -> read-only;
- workspace -> workspace.

That is equal authority, never broader.

For non-off descendants:
- direct parent must already be bound to a verified managed worktree;
- direct parent Session cwd/tool root must be within that exact binding;
- descendant worktree is a sibling under aw's managed worktree directory, not nested inside the parent checkout;
- descendant is based on the direct parent worktree’s exact clean committed HEAD;
- dirty direct-parent worktree fails closed.

Therefore a workspace child cannot delegate its uncommitted edits in Phase 44. Dirty-state snapshot/materialization remains deferred.

Each descendant managed worktree gets its own Phase-42 execution binding linked to the same root/new descendant allocation.

## Fan-out semantics

The Agent core validates a complete tool-call batch before first effect, then executes tool calls sequentially.

Phase 44 therefore supports:
- multiple foreground child calls sequentially;
- multiple background child calls that launch one by one and then execute concurrently once admitted.

Root created/active/model/deadline counters apply across all siblings/descendants.

No claim is made that foreground tool calls in one model round execute in parallel.

## Background concurrency

The durable root active-child limit remains authoritative.

The existing process-wide maximum-four background worker counter stays only as an additional safety cap. It cannot increase a smaller root max-active limit.

Descendants use the same shared process counter.

## Cancellation and deadline

Foreground descendant execution receives the direct parent cancellation token.

Background descendants detach exactly as Phase 41/42 but still:
- consume the shared root active slot;
- use the shared root deadline;
- consume shared model-attempt budget.

Root deadline remains absolute from first allocation.

Child request timeout is clamped to remaining root time at every depth.

Phase 44 does not introduce a new root-cancel command.

## Recovery

Owner restart:
- existing Phase-41 child relations interrupt;
- Phase-42 worktree uncertainty rules remain;
- Phase-43 root active/model-attempt accounting remains;
- recursive allocations are treated identically because every depth shares the same root.

No descendant/root budget is reset on restart.

No child/provider/Git effect is automatically replayed.

## UI

Sessions Tree becomes recursively nested by durable parent Session relation rather than stopping after one child level.

Each recursive child row shows:
- state;
- title;
- tool/isolation marker;
- depth where useful.

The Agent-family sidecar is local around the current Agent:
- one direct parent row when current is a child;
- direct children of the current Agent;
- shared root-budget header.

This lets users move up/down the recursive tree without flattening every descendant into one sidecar.

No Session/worktree path is injected into model context by the UI.

## CLI

Existing:

    aw subagent list
    aw subagent show ID
    aw subagent budget SESSION
    aw subagent cleanup ID [--discard]

continue to work.

`list`/show output includes recursive depth and parent allocation lineage.

No separate recursive-spawn CLI is added; recursion remains an explicitly host-enabled model tool behavior.

## Validation requirements

At minimum:
- v27 -> v28 identity-preserving migration;
- existing v27 roots/children migrate with recursive depth/max = 1;
- default max depth 1 preserves Phase-43 child tool surface (no recursive subagent);
- hard max depth 4;
- top-level child depth 1;
- descendant depth = parent + 1;
- depth > root max denied before worktree/provider;
- descendant cannot create a new root;
- descendant exact parent identity/account/model revalidated;
- descendant allocation consumes same root created counter;
- descendant running consumes same root active counter;
- descendant model rounds consume same root model-attempt counter;
- recursive foreground chain actual-binary smoke;
- multiple background sibling admissions respect shared active cap;
- tool-enabled recursive child receives equal/no-broader file tools;
- recursive managed worktree is sibling and requires clean direct parent checkout;
- dirty parent child worktree blocks descendant before descendant provider;
- max-depth child does not advertise subagent;
- Session tree recursively nests grandchildren;
- Agent sidecar shows direct parent/direct children plus root budget;
- Phase-43/42/41/40/39/38 checkers remain green;
- tiny/no-default/headless/routing cuts remain green;
- whole-workspace/all-targets, doctests, rustdoc, Clippy, reference/static gates;
- pinned Nix/release claims only when the Nix store is writable.
