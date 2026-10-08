# Phase 46 — bounded multi-model teams with explicit join/publication

**Status:** authorized 2026-10-05 by the user after Phase 45 review.

## Goal

Phase 46 adds one bounded multi-model team primitive on top of the proven Phase-41–45 child Session machinery.

A trusted host selects one immutable team template for the root Agent. The Agent may call a `team` tool with a task. The host runs every required template member as an ordinary foreground child Session, records each child result, performs a deterministic all-required join, and returns the joined evidence to the parent Agent. The parent Agent's next model round is the only synthesizer/publisher of the parent continuation.

There is no second conversation store and no second agent loop.

## Template

A team template is transport-neutral typed data in the existing `aw-pipeline` vocabulary crate:

- name;
- optional description;
- 2–4 ordered members;
- each member has a bounded ID, role and static instruction;
- join policy = `all_required` only.

Templates contain no provider URLs, credentials, account IDs, route candidates, tools, shell commands, filesystem paths, budgets or executable code.

Templates are immutable actor-owned revisions.

Management:

    aw team-template define FILE
    aw team-template list
    aw team-template show TEAM [--revision REV]

## Runtime selection

Root-run flags:

    --subagent-team TEAM
    --subagent-team-revision REV

`--subagent-team` requires `--subagents`.
An explicit revision requires a team selector.

The template is resolved once from the initiating user's durable store authority when the root Agent is prepared.

Only the root Agent receives the `team` tool.

Recursive child Agents may still use `subagent` according to Phase 44, but they do not receive `team` in Phase 46. Team-of-teams is deferred.

## Model-facing team tool

The `team` tool accepts only:

- `task`;
- optional `title`.

The model cannot select:
- template/member set;
- join policy;
- provider/account/model;
- route/revision/candidate;
- file-tool mode;
- worktree;
- depth;
- budget/deadline;
- publisher.

## Candidate execution

Candidates run sequentially in template order in Phase 46.

For each member:

1. persist member intent;
2. construct one bounded child task from:
   - predeclared member role;
   - predeclared member instruction;
   - shared team task;
3. invoke the existing subagent Service in foreground mode;
4. therefore reuse all existing child admission, root budgets, routing, recursive depth, worktree isolation and no-replay contracts;
5. persist child Session/subagent IDs plus answer digest/truncation evidence.

A team candidate is never represented as an in-memory pseudo-agent. It is an ordinary durable child Session.

Candidate full answers remain authoritative in those child Sessions.

## Join

Join policy is deterministic `all_required`.

If any required member fails/cancels/is incomplete:
- the team run fails;
- no join receipt is produced;
- parent receives a structured team-tool failure;
- already completed child Sessions remain durable/inspectable;
- no candidate is automatically replayed.

When every member completes:

1. import a bounded UTF-8 representation of each answer;
2. order by immutable template member order;
3. label each imported result with role + member ID;
4. mark truncation explicitly when only a bounded prefix is imported;
5. compute one deterministic joined-text SHA-256;
6. persist an immutable join receipt containing member/session/subagent IDs, answer digests, truncation flags and joined digest;
7. return joined evidence to the parent Agent as the team tool result.

SQLite does not duplicate the joined answer text or full child answers.

## Publisher/synthesis

Phase 46 fixes:

    publisher = parent_agent

The root parent Agent receives the team tool result in its normal tool/result context and performs the next synthesis model round itself.

The parent's final assistant message is therefore the single publication point.

The team run is anchored to that exact parent Session/branch/turn. The parent turn's existing durable tool record links the team tool result back to the team_run_id.

A `joined` team run means candidate evidence was successfully joined and returned to the parent tool loop. It does not fabricate or separately store an assistant publication.

A separate synthesizer child/model role is deferred.

## Schema v30

Add:

- `team_templates`;
- `team_template_revisions`;
- `team_runs`;
- `team_run_members`;
- `team_join_receipts`.

Team revisions and join receipts are immutable.

Team run states:

    admitted -> running -> joined
                         -> failed
                         -> interrupted

Member states:

    pending -> running -> completed
                       -> failed
                       -> interrupted

The run records:
- exact team revision;
- stable initiating user;
- exact parent Session / branch / turn;
- task digest;
- publisher_kind = parent_agent;
- lifecycle.

Member receipts record only:
- member ID/role/order;
- state;
- subagent ID;
- child Session ID;
- imported-answer SHA-256;
- truncation bit;
- bounded error/timestamps.

The immutable join receipt stores a JSON manifest of those IDs/digests plus the joined digest. It stores no joined/full answer body.

## Recovery

Owner restart marks admitted/running team runs interrupted and pending/running members interrupted.

Already completed member child Sessions remain authoritative.

Recovery never:
- reruns a team member;
- redoes the join;
- repeats provider calls;
- deletes candidate Sessions/worktrees;
- fabricates a parent publication.

The interrupted team can be inspected; a future explicit retry phase may define fresh-attempt semantics.

## Routing/tool/budget authority

Phase 46 grants no new provider or tool authority.

Each candidate delegates through the existing root Subagent Service:
- Phase 43 created/active/model-attempt/deadline limits;
- Phase 44 recursion/depth;
- Phase 45 child routing;
- Phase 42 off/read-only/workspace isolation;
- no trusted/bash.

Candidate member role/instruction affects task text/provenance only. It does not bind a special provider role or route target in Phase 46.

## Bounded evidence import

Shared team task:
- max 96 KiB.

Each imported member answer:
- max 20 KiB in the join representation.

The full child answer stays in its Session.

Total deterministic joined evidence remains under the existing bounded pipeline/tool result ceiling.

Member text is explicitly untrusted evidence. It cannot act as executable authority.

## UI / observability

Phase 46 does not add a new navigator.

Existing recursive Session tree naturally shows team-created child Sessions.

Management/inspection:
- `aw team-template ...` manages immutable templates;
- `aw subagent ...` still inspects child allocation/routing/budgets;
- Phase-46 team run inspection is initially through durable receipts/source tests and the parent tool result.

A richer Team/Pipeline/Logs content view is deferred to the later observability phase.

## Deferred

Explicitly deferred:
- reviewer stage consuming candidate outputs;
- separate synthesizer model/child;
- quorum/vote/first-success joins;
- parallel foreground candidates;
- team-of-teams;
- dynamic/planner-generated member lists;
- generative task DAGs;
- role-specific routing strategies/candidate subsets;
- per-member sub-budgets;
- monetary/cost budgets;
- guardians/approval DAGs;
- automatic patch selection/import/merge;
- schedules/autonomy/events;
- trusted child shell;
- cross-machine team placement.

## Validation requirements

At minimum:

- v29 -> v30 identity-preserving migration;
- team template validation/digest deterministic;
- 1 member / >4 members rejected;
- template cannot contain unknown authority fields due deny_unknown_fields;
- immutable revision creation/list/show;
- exact active parent turn required for team run;
- two-member all-required run lifecycle;
- candidate child Sessions are ordinary durable subagents;
- candidate outputs stored only as digests/Session links in team tables;
- deterministic join order independent of completion/result input order;
- no join when any required member fails;
- root parent alone receives team tool;
- recursive child tool surface has no team;
- candidate routing/worktree/budget behavior reuses subagent Service;
- joined tool output bounded and marks truncation;
- restart interrupts team without replay while preserving completed children;
- actual compiled team smoke with two candidate provider calls + parent synthesis call;
- parent final answer publication occurs only after joined team tool result;
- Phase-45/44/43/42/41/40/39/38 checkers remain green;
- tiny/no-default/pipelines/routing cuts remain green;
- whole-workspace/all-targets, doctests, rustdoc, Clippy, reference/static gates;
- pinned Nix/release claims only when Nix store is writable.
