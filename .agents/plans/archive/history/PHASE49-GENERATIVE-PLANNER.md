# Phase 49 — governed generative task planning

Status: authorized 2026-10-05 by the user after Phase 48 review.

## Boundary

Phase 49 adds an optional root-only generative task planner on top of the existing Phase-39 pipeline kernel and Phase-41–46 delegation/team machinery.

The planner proposes bounded typed task data. It does not author executable pipeline code or grant authority.

SQLite advances to v31.

Phase 49 does not add reviewer/guardian stages, inter-agent result messaging, parallel DAG execution, schedules/autonomy, automatic Git merge/import, or cross-machine placement.

## Host opt-in and planner selection

Planner execution requires the existing separate --subagents opt-in plus:

- --subagent-planner-route ROUTE
- optional --subagent-planner-route-revision REV
- repeatable --subagent-planner-role ROLE
- --subagent-planner-max-tasks N

Hard max tasks is 6.

If no planner roles are supplied, the host allowlist defaults to one semantic worker role.

The planner routing strategy is immutable, actor-owned, role=planner and mode=fixed.

Phase 49 deliberately does not use another classifier to select the planner model. Planner target selection is host-declared and inference-free.

The fixed planner target still uses the Phase-40 destination fingerprint/profile/account/model registry and is revalidated at dispatch.

## Model-facing plan tool

Only the root Agent receives the plan tool.

The plan tool arguments are:

- task
- optional title

The model cannot choose:

- planner route/revision;
- planner account/model/endpoint;
- planner role allowlist;
- max task count;
- child routing strategy;
- child account/model;
- tool mode;
- worktree/root;
- depth;
- budget/deadline;
- scheduler;
- reviewer/guardian;
- merge/import policy.

Recursive child Agents do not receive the plan tool in Phase 49.

## Planner evidence and proposal

The planner receives only bounded JSON evidence:

- root task text;
- host-approved role labels;
- max_tasks.

Planner output must be strict JSON matching a bounded proposal containing tasks.

Each task contains only:

- id;
- role;
- task text;
- depends_on task IDs.

Limits:

- 1..6 tasks;
- unique bounded task IDs;
- role must be in the host allowlist;
- bounded task text;
- bounded dependency list;
- dependencies must refer to tasks in the same proposal;
- no duplicate dependencies;
- no self-dependency;
- graph must be acyclic.

Planner output cannot contain endpoint, account, model, credential, tool grant, route strategy, budget, shell command, schedule, URL or executable pipeline code.

Malformed, oversized, unknown-role, unknown-dependency or cyclic proposals fail closed before any child task is created.

## Tool-free paid planner invocation

aw-agent-core exposes a dedicated PlannerExecutor / PlannerJournal path.

Planner provider invocation:

- has no tools;
- does not append a conversation Turn;
- does not reuse provider-private continuation state from Chat;
- uses InvocationPurpose::Planner;
- persists exact planner provider intent before provider entry;
- revalidates selected provider/account/model/destination around extension hooks and dispatch;
- emits no partial plan on failure;
- commits planner outcome/usage before child execution begins.

Extensions receive dedicated before_planner / after_planner hooks.

## Shared root delegation envelope

Planner admission creates or reuses the ordinary Phase-43 delegation root for the exact active parent Session/branch/turn.

The root additionally pins one planner_routing_revision_id.

If the root already exists:

- delegation limits must match;
- child routing revision must match;
- an existing planner revision cannot change.

If the planner creates the root first, later ordinary planned children reuse the same root.

Planner admission does not consume a fake child allocation.

Before planner provider entry, the planner attempt consumes one shared root model-attempt slot.

Known usage is reconciled into the same root token counters.

The root deadline applies to the planner and all planned children.

max_tasks must fit inside the root's remaining child-created budget before planner provider entry.

## Durable v31 planner receipts

Schema v31 adds:

- delegation_roots.planner_routing_revision_id;
- planner_operations;
- planner_task_runs;
- planner_join_receipts.

planner_operations records:

- root and exact parent Session/branch/turn;
- exact parent branch head/revision at admission;
- immutable planner strategy revision;
- secret-free planner InvocationIdentity;
- exact planner input digest/evidence;
- planner lifecycle;
- validated proposal JSON/digest;
- bounded completion metadata/error;
- timestamps.

Lifecycle:

admitted -> started -> planned -> running -> joined

Terminal alternatives:

failed / cancelled / unknown / interrupted

A started planner operation becomes unknown on owner restart and is never replayed automatically.

A planned/running operation becomes interrupted on owner restart and no child task or join is replayed automatically.

planner_task_runs records only task topology, lifecycle, child/session links and answer digest/truncation evidence. It does not duplicate full child answers.

planner_join_receipts are immutable and store only provenance manifest + joined digest, not joined/full answer bodies.

## Planned task execution

After a valid planner proposal commits, aw computes deterministic topological order.

Tasks execute sequentially in Phase 49.

For each task:

1. dependencies must already be completed;
2. task row becomes running;
3. one ordinary foreground subagent is invoked through the existing subagent Service;
4. ordinary Phase-41–45 child Session, routing, worktree and root-budget semantics apply unchanged;
5. completed child answer is verified against the authoritative child Session before its digest/receipt is attached;
6. next task may begin only after dependency completion.

A task failure makes the planner operation fail, interrupts remaining pending tasks, and produces no final planner join.

## Dependency semantics

Phase-49 dependencies are ordering constraints only.

A downstream child receives:

- root objective;
- its own planned task;
- predecessor task IDs.

It does not automatically receive predecessor answer text.

This intentionally avoids introducing a new child-to-child disclosure channel before the later inter-agent coordination phase.

Full predecessor answers remain in their child Sessions and are visible only through existing authorized Session access.

## Worker routing and authority

Planned tasks execute via the same existing subagent Service.

Therefore they may use the already host-selected child routing strategy from Phase 45.

The planner role does not select child targets.

Task role labels are semantic planner/task metadata, not provider account IDs and not tool grants.

Child tool mode/root/worktree/depth/budget remain entirely host-controlled.

Planner output cannot widen them.

## Final join and publication

After all tasks complete, aw reconstructs a deterministic topological join.

Each imported task result is bounded.

Full answers remain in child Sessions.

The store independently verifies:

- every task is completed;
- child relation belongs to the exact planner parent turn;
- child Session has one authoritative completed answer;
- imported result equals that answer or an exact bounded prefix when marked truncated;
- task ID/role/order match the validated proposal.

The immutable join receipt stores task/session/subagent IDs, dependency metadata, answer digests, truncation flags and joined digest.

It stores no full child answer body.

The root parent Agent receives the joined plan result as one plan tool result and remains the sole final publisher/synthesizer.

## Restart and unknown effects

No planner/provider/child effect is replayed from history alone.

On owner restart:

- admitted planner operations become interrupted;
- started planner operations become unknown;
- planned/running planner operations become interrupted;
- pending/running planned task rows become interrupted;
- completed child Sessions remain durable and inspectable;
- no final join is fabricated.

An unknown planner provider attempt still consumes its started root model-attempt slot.

## Deferred

Phase 49 explicitly defers:

- planner-selected provider/model/route strategy;
- planner-selected tool/root/worktree/budgets;
- dynamic arbitrary pipeline code;
- parallel planned task execution;
- child result disclosure to dependent children;
- authorized peer messages;
- resource claims beyond existing worktree/root fencing;
- reviewer stages;
- guardian/approval DAGs;
- separate synthesis model;
- quorum/vote/first-success joins;
- monetary/cost sub-budgets;
- schedules/autonomy/events/webhooks;
- automatic patch selection/import/merge;
- cross-machine planner/child placement;
- plan tool recursion from child Agents.

## Validation requirements

At minimum:

- planner evidence/proposal parser rejects unknown fields/malformed JSON;
- unique IDs and host role allowlist enforced;
- unknown/self/duplicate dependencies rejected;
- cycles rejected;
- deterministic topological order;
- strict tool-free PlannerExecutor with dedicated InvocationPurpose::Planner;
- planner attempt intent committed before provider entry;
- fixed actor-owned role=planner route required;
- planner strategy destination/profile revalidated;
- planner call consumes shared root model-attempt/deadline budget;
- planner admission creates no fake child count;
- max_tasks cannot exceed remaining child budget;
- validated proposal is durable before child execution;
- dependency task cannot start early;
- planned children use ordinary durable subagent Sessions;
- forged imported child answer rejected;
- task failure yields no join;
- restart after planner provider intent produces unknown/no replay;
- restart after plan materialization interrupts/no task replay;
- deterministic parent join stores IDs/digests only;
- actual compiled smoke proves planner -> two sequential children -> parent synthesis;
- second child receives predecessor ID but not predecessor answer;
- shared root accounting counts planner + both child calls;
- parent Agent is sole final publisher;
- historical Phase 48 and earlier checks remain green;
- v30->v31 migration preserves store identity;
- trusted-local direct remains exact-current-schema and therefore requires v31.
