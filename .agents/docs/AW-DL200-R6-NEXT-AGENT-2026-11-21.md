# DL-200 R6 next-agent prompt — 2026-11-21

Canonical full context:
[`AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md`](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md)

Copy/paste this prompt to the next agent:

~~~text
Use dotlink---l5 only.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect/list/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless explicitly promoted.
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
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. dotlink/AGENTS.md
11. dotlink/.agents/docs/CURRENT_HANDOFF.md
12. live source/tests for every owner touched

Current published refs:
  AW R6/Phase-0: 20a63a4d4547cc20b2a154787823125b82f5e385
  dotlink R6:     85ceccc1088e201c98c994a46a68834440eeab32
  release ref:    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9
  dotlink AW pin: 20a63a4d4547cc20b2a154787823125b82f5e385

Important current state:
- R6 + shared Process/release-correction amendment are sealed, published and pinned.
- Phase 0B is complete and published.
- Ordinary pinned dotlink build, shell 86/86, Graph 13/13, real Spawn-Lane cancellation,
  pre-spawn-timeout/no-Process and spawn-refusal gates are green.
- Real Bubblewrap worker-retirement and Windows-native qualification were explicitly deferred by the
  user; do not claim them green and do not block Phase 1 on them.
- Phase 1 is underway. The first current AW slice adds one private StoreScope over existing
  store_id+restore_generation and transparent StorePos(u64), and threads typed commit_log.seq through
  commit insertion -> Job exposure projection while keeping persisted/public compatibility u64.
- StoreScope 2/2, mixed Job exposure, coordination 24/24 and strict aw-store Clippy/fmt/diff are green.
- release/0.17 remains exact v0.17.9 / 576d0a1 and read-only.

Do not redo already-complete Phase-0 source cuts.

Continue Phase 1 from IMPLEMENTATION.md: add only the StoreScope-bound cursor/projection mechanics
needed by the authoritative Agent migration/history/disclosure work, then implement that Agent
foundation. Do not build generic cursor/transaction/replay engines or Phase-2/4 machinery early.

Keep implementation turns around 30 minutes and checkpoint at the next coherent green point.
Prefer simple owner-local Rust primitives, zero-cost/static abstractions and easy hot-path wins.
Do not create a generic TransactionManager, global replay ledger, universal revision clock,
generic Jobs+Events traversal engine, distributed Event router, public spawn-pool framework or
unused registry implementations.
~~~
