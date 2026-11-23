Use dotlink---l5 as the only source-authoring connector.

Continue in:

- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink
- read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated/concurrently owned work.

Never inspect, list, stage, reset, stash, read, modify, or touch:

AW/.agents/plans/ux/**

Sandbox/mount/config-v13 and the 017 Sandbox implementation remain frozen unless explicitly promoted.

Do not push, tag, release, or deploy unless separately authorized.

Read first, in this order:

1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
11. dotlink/AGENTS.md
12. dotlink/.agents/docs/CURRENT_HANDOFF.md
13. live source/tests for every owner you touch.

Current implementation checkpoint beneath the handoff documentation:

03d1224ef7b2054b3856fa559dcc23ac666f341f — Rebuild Agent exposure from owner state

Actual verified remote refs on 2026-11-23:

AW remote main:
78339410710472ec51d5ff46a5c9e586b67ff30d — Own logical Sandboxes

dotlink remote main:
85ceccc1088e201c98c994a46a68834440eeab32

dotlink AW pin:
20a63a4d4547cc20b2a154787823125b82f5e385

release/0.17:
576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

The local AW tracking ref origin/main is stale; do not use it as publication truth.
The implementation line through 03d1224 is 9 implementation commits ahead of the actual verified remote
main; docs-only handoff commits may sit above it.

R6 and the shared Process/release-correction amendment are sealed.

Phase 0 / 0B are complete and published.

Real Bubblewrap worker-retirement and Windows-native Process qualification are explicitly deferred evidence.
Do not claim them green and do not block later phases on them.

Current AW Store schema is v43.

Phase 1 is late-stage and current source already includes:

- StoreScope = store_id + restore_generation;
- StorePos over store-local commit-log ordering;
- v38 authoritative agent_state_current + agent_acl_current;
- v39 Domain principals/groups;
- full seven-permission Agent ACL:
  view/contribute/submit/control/derive/share/manage;
- group-aware authorization + opaque access_generation;
- v40 immutable HistorySpan / HistoryBase / HistoryGrant;
- persistent bounded DisclosureManifest DAG;
- exact structurally shared history forks;
- canonical Agent + settled-Turn selectors;
- model agent(..., from?);
- external host Agent state lower shell;
- factual current/stable/non-current Agent close composition;
- taskless host activation;
- task-bearing Prepared create + atomic initial Pending input publication;
- native agent_create;
- v41 logical Sandbox owner + immutable Agent binding;
- native sandbox_list/get/close;
- same-user cross-Project agent.create with explicit destination Sandbox;
- v42 staged Agent input-plan owner;
- v43 staged mutation replay;
- atomic staged Send;
- all five native agent_inputs methods;
- immutable Turn/Tool/model provenance;
- finite Job/Process provenance;
- authoritative Agent exposure/index normalization from agent_state_current.

Do not redo those owners.

The next concrete Phase-1 gap is thin native RPC exposure for Store operations that already exist:

agent.list
agent.get
agent.edit
agent.admission
agent.access.list
agent.access.set

Implement one concurrent AgentStatePort over the existing Store methods.

Reuse:

- SessionStore::agent_list
- SessionStore::agent_get
- SessionStore::agent_edit
- SessionStore::agent_admission
- SessionStore::agent_access_list_ref
- SessionStore::agent_access_set_ref

Do not add:

- new Agent state tables;
- a second ACL owner;
- a new replay ledger;
- a new constructor;
- provider/model/Sandbox/Vault/source authority fields;
- Phase-2 EventScan;
- Phase-3 SourceDriver/Sandbox materialization;
- Phase-4 Jobs high-water/tree traversal machinery.

Use strict sealed parameter parsing.
Keep read operations as pure Store projections.
Keep title/admission/access mutation semantics in their existing owners.
Use current/canonical Agent refs exactly as the SPEC defines.
Reject unknown or authority-bearing fields before the port.

Prefer extending the existing host state-shell regression and strict headless frame/capability regression rather than creating a new fixture family.

After AgentStatePort, re-audit SPEC §28 against actual native capabilities.
Only if no real Phase-1 gap remains should Phase 1 be marked complete and Phase 2 begin.

Keep each implementation turn around 30 minutes and pause at the next coherent green checkpoint.

Avoid over-engineered granular tests/evidence.

Favor simple, elegant, composable, reusable Rust primitives and zero-cost/static abstractions.
Take easy performance wins where maintenance remains simple; otherwise prefer clean/simple first.

Do not create:

- TransactionManager / OperationManager;
- global replay ledger;
- universal revision clock;
- generic Jobs+Events traversal engine;
- distributed Event router;
- public spawn-pool framework;
- second Process/Event/Project/Session/Source owner;
- unused registry implementations;
- Agent↔Sandbox rebind API;
- cross-store Event→Agent delivery v1;
- cross-store ordered recursive Jobs traversal v1.

At each checkpoint:

1. run focused real regression(s);
2. run strict relevant Clippy/check/fmt/diff;
3. update CURRENT_HANDOFF.md + plan TODO.md factually;
4. commit exact owned paths only using a private GIT_INDEX_FILE;
5. use expected-parent CAS when updating main;
6. make a signed terse checkpoint commit;
7. verify signature;
8. pause;
9. push only when separately requested.

Expected signing identity:

Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
