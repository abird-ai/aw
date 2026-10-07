# Phase 42 — isolated subagent worktrees and bounded file tools

**Status:** authorized 2026-10-05 by the user after Phase 41 review.

## Boundary

Phase 42 makes Phase-41 subagents useful for local file/code work without weakening aw's filesystem-effect fence.

A tool-enabled child receives a host-created isolated Git worktree and may receive bounded read-only or workspace file tools inside that worktree.

Phase 42 does not add:
- trusted shell/bash child tools;
- recursive subagents or swarms;
- model-selected child tool mode, root, Git ref, account, model, or route;
- child routing;
- schedules, cron, autonomous loops, or event/webhook triggers;
- automatic commit, staging, merge, rebase, fetch, push, patch import, or parent-checkout mutation;
- non-Git directory copying;
- automatic worktree deletion.

Phase 41 remains valid beneath this phase: every child is an ordinary durable Session in the same Workspace; exact parent Session/branch/turn and invocation identity are pinned; foreground/background lifecycle is durable; restart never silently replays inference; children never receive the subagent tool.

## User/host grant

Phase 42 adds one host-selected run flag:

    --subagent-tools off|read-only|workspace

Default is off.

A non-off value requires the separate --subagents opt-in. The model-facing subagent tool remains limited to task, optional title, and foreground/background mode.

The model cannot choose child tool mode, filesystem root, worktree path, Git executable, Git ref, provider, payer, credential custody, account, model, routing, or shell authority.

trusted is rejected for child Agents in this phase. The existing bash tool is intentionally not a sandbox and can escape a checkout through normal same-user OS authority. Shell-capable children therefore require a later reviewed sandbox/runner phase.

## Why managed Git worktrees

Conversation branching is not filesystem isolation. Two Sessions bound to one checkout can race even when SQLite branch state is isolated.

For a non-off child tool grant Phase 42:
1. discovers the parent checkout through the trusted Git executable;
2. requires the parent working directory and tool root to remain inside that repository;
3. requires a clean parent worktree, including no staged, unstaged, conflicted, or untracked changes;
4. pins the exact committed HEAD object ID;
5. creates a detached worktree below the private aw state directory;
6. binds the child Session working directory/tool root to the same relative locations inside that isolated checkout.

Dirty parents fail closed before child provider entry. Phase 42 deliberately does not silently give a child an older committed view when the parent has uncommitted work.

Non-Git roots fail closed for tool-enabled children. Arbitrary directory snapshots are deferred.

## Managed path

Managed worktrees live under:

    <state-dir>/subagents/worktrees/<execution-binding-id>/

The state directory, subagents directory, and worktrees directory are platform-validated private directories.

The binding ID/path is host-generated. The model cannot choose it.

Canonical path authority is stored as platform path bytes in SQLite BLOBs rather than lossy display strings.

## Git mutation boundary

Git worktree creation/removal are external filesystem/Git effects and receive durable intent before Git process entry.

The existing aw-git subprocess contract remains:
- explicit absolute trusted Git executable;
- no shell;
- cleared environment;
- no credential helpers or terminal prompting;
- global/system Git config disabled;
- hooks disabled;
- bounded output/time;
- no lazy network fetch.

Managed checkout additionally refuses executable clean/smudge/process filters and promisor/partial-clone configuration.

Allowed Git operations for Phase 42 are only:
- inspect exact clean HEAD;
- create one detached worktree at that exact object;
- rediscover/verify that worktree in the same common repository;
- explicitly remove that exact discovered managed worktree.

The child model does not receive Git management tools.

## Schema v26

Phase 42 advances SQLite to v26 and adds:

    subagent_execution_bindings

This table is separate from subagent_sessions because conversation provenance and external execution-resource lifecycle are different state machines.

A binding records:
- binding ID;
- exact parent Session / branch / turn;
- creating stable user;
- kind = managed_worktree;
- host-selected tool mode = read-only or workspace;
- canonical parent repository root bytes;
- canonical planned execution root bytes;
- exact base commit object ID;
- lifecycle state;
- optional linked child Session;
- timestamps and bounded error.

States are:
- planned;
- ready;
- bound;
- cleanup_planned;
- unknown;
- removed.

The owner commits planned before invoking git worktree add.

After Git succeeds and the host verifies exact path, common repository, clean checkout, and expected HEAD, the binding moves to ready.

Only a matching ready binding for the exact parent Session/branch/turn/tool mode can be consumed when the child Session is created. Child Session creation and ready -> bound occur in one SQLite transaction.

If the owner restarts while creation or cleanup is in progress, planned/cleanup_planned becomes unknown. Recovery never repeats Git mutation or deletes a possible worktree automatically.

A ready but unbound allocation is retained and visible for explicit cleanup.

## Child tool authority

off:
- Phase-41 behavior;
- no managed worktree;
- no local tools;
- same-root model-only synthetic run-key exception remains available.

read-only:
- isolated managed worktree;
- child advertises only read;
- no write/edit/bash/subagent.

workspace:
- isolated managed worktree;
- child advertises read, write, edit;
- no bash/subagent.

A non-off child uses its isolated SessionBinding as the normal physical root. It does not use the Phase-41 same-root synthetic model-only bypass. Ordinary durable tool-effect hazard/reconciliation semantics apply inside the child worktree.

Different managed worktrees may execute concurrently because they are distinct physical effect roots.

## Context disclosure

Execution isolation does not broaden context.

The child receives:
- parent system instructions plus a fixed bounded child-role suffix;
- the explicit delegated task;
- only host-granted file tools rooted at the isolated child tool root.

The child does not automatically receive:
- parent transcript;
- parent tool trace;
- Files/Git UI state;
- PTY state;
- arbitrary parent working-tree changes;
- shared drafts;
- another account/model;
- recursive subagent authority.

Foreground result return and background Session inspection remain Phase-41 behavior.

## Cleanup

Managed worktrees are retained after child completion/failure/cancellation for human inspection.

Human management commands are:

    aw subagent list
    aw subagent show SUBAGENT_ID
    aw subagent cleanup SUBAGENT_OR_BINDING_ID
    aw subagent cleanup SUBAGENT_OR_BINDING_ID --discard

Cleanup is never a model tool.

Rules:
- only the parent Session owner can clean;
- a running/admitted child cannot be cleaned;
- cleanup writes durable cleanup_planned before Git removal;
- normal cleanup refuses dirty worktrees and rolls the intent back to bound/ready;
- --discard is explicit destructive confirmation for dirty child work;
- ambiguous Git removal becomes unknown and is never retried automatically;
- successful removal is verified against Git worktree inventory before state becomes removed;
- child Session/history is never deleted by worktree cleanup;
- ready-but-unbound allocations can be cleaned by execution-binding ID.

## UI

Phase-41 Session nesting and Agent family sidecar remain the source of navigation.

For tool-enabled children the projection shows the authority/isolation marker, for example:

    workspace · isolated

or:

    read-only · isolated

The full managed path remains secondary host metadata and is not injected into model context.

Git's existing Worktrees view naturally sees the managed checkout because it is a real worktree. Selecting it for human review never rebinds an Agent automatically.

## Recovery

Owner startup:
- Phase-41 admitted/running child relations still become interrupted without model replay;
- planned/cleanup_planned worktree bindings become unknown;
- no Git create/remove is automatically retried;
- no worktree is auto-pruned;
- no child changes are auto-merged;
- ready/bound allocations remain inspectable.

Unknown file-write effects use the ordinary Session/tool hazard model inside the isolated child root.

## Deferred

Explicitly deferred after Phase 42:
- bash/trusted child tools;
- container/OS sandboxing;
- dirty-parent snapshot/materialization;
- non-Git managed-directory copies;
- automatic commit/stage/merge/rebase/patch import;
- recursive subagents / swarms / model-selected fan-out;
- child routing or independent account/model selection;
- descendant budget governance;
- schedules/cron/autonomy/event triggers;
- reviewer/synthesis teams;
- browser/shared-participant spawning;
- cross-machine child placement;
- automatic worktree cleanup/pruning.

These require another explicitly authorized numbered phase.

## Validation requirements

At minimum:
- schema v26 and v25->v26 identity-preserving migration;
- durable planned intent before Git process entry;
- owner restart planned -> unknown with no Git replay;
- dirty/non-Git parent fails before child provider entry;
- exact committed parent HEAD pin;
- canonical parent cwd/tool root inside repository;
- host-generated private managed path;
- real Git detached-worktree create/verify/remove tests;
- child Session binding remaps cwd/tool root into managed worktree;
- read-only child advertises/executes read only;
- workspace child advertises read/write/edit only;
- trusted child tools rejected;
- child never receives subagent/bash/Git management;
- tool-enabled child uses ordinary isolated physical-root journal semantics;
- foreground/background Phase-41 behavior remains green;
- dirty cleanup refuses unless explicit --discard;
- cleanup preserves child Session/history;
- actual compiled fake-provider + real-Git Phase-42 smoke;
- Session tree/Agent sidecar exposes tool-mode/isolation marker;
- Phase 41/40/39/38 and coordination regressions remain green;
- tiny/no-default binary remains free of this durable runtime;
- reference/static/whole-workspace/rustdoc/clippy gates;
- pinned Nix/release proof only when the Nix store is writable.
