# DL-200 R6 next-agent prompt — 2026-11-22

Canonical full context:
[AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md)

Copy/paste this prompt to the next agent:

~~~text
Use dotlink---l5 as the only source-authoring connector.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1.
Do not create another worktree.
Preserve unrelated/concurrently owned work.
Never inspect/list/stage/reset/stash/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless explicitly promoted.
Do not push/tag/release/deploy unless separately authorized.

Read first, in order:
1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. dotlink/AGENTS.md
11. dotlink/.agents/docs/CURRENT_HANDOFF.md
12. live source/tests for every owner touched

Current published refs:
  AW main:
    0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e
    Wire Agent from refs

  AW R6/Phase-0 publication:
    20a63a4d4547cc20b2a154787823125b82f5e385
    Complete DL-200 R6 Process and replay correction

  dotlink main:
    85ceccc1088e201c98c994a46a68834440eeab32
    Publish DL-200 R6 consumer and repin AW

  dotlink AW pin:
    20a63a4d4547cc20b2a154787823125b82f5e385

  release/0.17:
    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

Important current state:
- DL-200 R6 + the shared Process/release-correction amendment are SEALED.
- Phase 0 shared Process convergence is published.
- Phase 0B replay/admission/disclosure/lifetime work is complete and published.
- Ordinary pinned dotlink build, shell 86/86, Graph 13/13, queued Spawn-Lane cancellation,
  pre-spawn-timeout/no-Process and canonical spawn-refusal gates are green.
- Real Bubblewrap worker-retirement and Windows-native qualification were explicitly deferred;
  do not claim them green and do not block Phase 1 on them.
- release/0.17 remains exact v0.17.9 / 576d0a1 and read-only.
- dotlink intentionally still pins the Phase-0 AW revision; do not repin just because AW Phase 1 moved.

Phase 1 current implementation is already substantial:
- StoreScope = existing store_id + restore_generation; StorePos wraps commit_log.seq.
- schema v38: agent_state_current + agent_acl_current are authoritative Agent envelope/ACL truth.
- Agent rev is branch-local after migration; audit is evidence, not rev owner.
- schema v39: one Domain user/group principal namespace, simple non-nested groups, tombstones.
- effective Agent access = owner UNION direct user UNION all matching active groups.
- full seven permissions:
    view / contribute / submit / control / derive / share / manage
- group membership changes group rev only; no Agent-rev fanout.
- 32-byte access_generation fences wake/history/security observations without becoming Agent rev.
- schema v40: immutable HistorySpan / HistoryBase / HistoryGrant +
  persistent SHA-256 DisclosureManifest DAG.
- sealing is lazy/idempotent/revision-neutral.
- exact history forks create a physically fresh Session/Agent and structurally share immutable history.
- inherited history is rendered as HistoryBase spans + local tail; backing node digests are rechecked.
- active source Turn N forks only through committed N-1 via produced_from_head.
- Store create_agent_from_history_ref accepts canonical Agent or settled Turn refs.
- settled Turn selectors work for local attached results and inherited HistoryBase Turns.
- active/unsettled Turn refs fail closed.
- model agent(task,title?,background?,from?) uses existing ExecutionRef contextual grammar,
  canonicalizes from, restricts model-selected source to the current Project, and attaches history
  inside the existing durable child-allocation transaction.
- from="." and the equivalent absolute ref replay the same committed child.
- no-from constructor fingerprint shape remains backward compatible.

Phase-1 implementation checkpoint sequence:
  9784619 Start DL-200 Phase 1 store-scope foundation
  d9f53db Wire Agent owner seam
  77fced2 Make Agent recovery exact
  b7059fb Mark accepted Agent input
  0e82f73 Activate Agent owner state
  10aa467 Add Domain principal groups
  bcca6dd Add group Agent access
  706f6f1 Add immutable history owner
  a5aa8fd Add full Agent access
  0db1144 Fork Agent history
  0f3f3eb Wire Agent from refs

Do not redo those owners.

Next coherent implementation boundary:
complete the EXTERNAL HOST Agent lifecycle/creation shell over existing owners.

Start with SPEC §28.3 agent.create policy/defaulting:
  from?   = Agent ref | settled Turn ref
  project = trusted Project selection/default current/source
  title?
  task?
  access  = private | inherit | custom principal grants
  sandbox = auto | Sandbox ref

Compose existing:
- create_agent_from_history_ref
- resources::create_in_workspace_tx
- agent_access_list / agent_access_set
- set_agent_title
- Agent lifecycle/close/admission helpers
- current AgentBinding/canonical identity
- existing authenticated replay identity/fingerprint

Do NOT create a second constructor/publication path.
History ancestry must never grant Sandbox/source/Vault authority.
Model callers must never choose trusted Project/ACL/Sandbox authority.

The complete host §28 shell remains open:
- agent.create
- list/get
- edit
- close
- admission
- access.list/set exposure
- later staged inputs as specified

After that, return to remaining Phase-1 owner work:
- provenance propagation through newly committed Event/Tool/Process/model facts where needed;
- minimal authoritative snapshots only for real consumers;
- normalize/rebuild Agent exposure from agent_state_current where pending;
- reachability/flatten acceleration only when a real consumer proves the need.

Do not prebuild Phase-2/4 generic engines.

Keep implementation turns around 30 minutes and pause at the next coherent green checkpoint.
Avoid over-engineered granular tests/evidence.
Prefer simple, clean, composable, reusable Rust primitives.
Favor zero-cost/static abstractions and easy hot-path wins when they stay maintenance-light.

Do not create:
- generic TransactionManager / OperationManager
- global replay ledger
- universal revision clock
- generic Jobs+Events traversal engine
- distributed Event router
- public spawn-pool framework
- second Process/Event/Project/Session/Source owner
- unused registry implementations
- cross-store Event delivery or ordered recursive Jobs federation in v1

At each coherent checkpoint:
1. run focused real regression(s);
2. strict relevant Clippy/check/fmt/diff;
3. update CURRENT_HANDOFF + plan TODO factually;
4. commit exact owned paths only using a private GIT_INDEX_FILE;
5. expected-parent CAS branch update;
6. signed commit;
7. verify signature;
8. pause;
9. push only when separately requested.

Expected Git signature:
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
~~~
