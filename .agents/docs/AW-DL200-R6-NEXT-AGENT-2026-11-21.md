# DL-200 R6 next-agent prompt — 2026-11-21

Copy/paste this prompt to the next agent.

~~~text
Use dotlink---l5 only.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless
explicitly promoted.
Do not commit/push/tag/release/deploy unless explicitly asked.

Read first:
1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. dotlink/AGENTS.md
10. dotlink/.agents/docs/CURRENT_HANDOFF.md
11. live source/tests for every owner touched

Current implementation source checkpoints after reseal:
  AW source      c7cc948
  AW R6 seal     7c3800c
  dotlink source 89ff3fb
  dotlink AW pin:
               b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
  read-only 017:
               94ae8bf2757a279dc5b16b50b010ee1d278c3b44

DL-200:
  R6 is explicitly sealed and authoritative as of 2026-11-21.
  R1-R5 are historical design provenance.
  Implementation was separately promoted; Phase 0 is in progress under R6.

R6 keeps the public model/API/phases unchanged and simplifies internal mechanics:
  Owner / Fact / Operation / Projection / Cursor / Scope
  StoreId + StorePos
  replay/Prepared + ExternalEvidence
  projection safe-prefix/head catch-up/atomic swap
  cursor envelope
  EventScan
  DriverKey + per-artifact schema
  typed registry core
  one shared Agent-neutral Process runtime
  Tree -> Occupancy

Do not create a universal transaction manager, generic Jobs/Event traversal engine,
global replay ledger, distributed Event router, universal revision clock or unused
registry implementations.

Current source:
  AW `c7cc948` checkpoints the pre-R6 Phase-0A Process work plus independent Phase-0B
  store/policy/exposure work. Preserve both conceptual slices.
  dotlink `89ff3fb` checkpoints Remote Start/replay hardening and has NOT begun the R6
  shared-Process cutover.

First revalidate current refs/status. Then follow the user's instruction.
R6 is already sealed. Continue Phase 0 only; do not start Phase 1 before the coherent
Phase-0 checkpoint is reviewed/accepted.

Keep implementation turns around 30 minutes and stop at the next coherent checkpoint.
~~~
