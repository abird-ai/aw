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

Published implementation checkpoints after reseal:
  AW main        59d67babaa846fa9b60e39a27170ac9d148e3b6f
  AW runtime pin 7c1670821b15082821a0b9621cd1caccbe62f144
  AW R6 seal     7c3800c
  dotlink main   fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94
  dotlink source 16475e4
  dotlink AW pin:
               7c1670821b15082821a0b9621cd1caccbe62f144
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
  AW shared Process runtime is published and consumed by dotlink:
    - Tree -> Occupancy / ProcessTerminal.occupancy
    - weak direct-Unix negative -> Occupancy::Unknown
    - minimal aw-process solely owns async Scope/Child/stdin/pipes/wait/stop/reap/drain
    - global bounded drain flushes captured partial sink batches before incomplete publication
    - interactive stdin writes are cancellation/deadline-preemptible
    - aw-tools-adapter retains policy/storage/ProcessReceipt only; aw-terminal PTY stays separate
  AW grouped evidence: aw-process 3/3, runner 4/4, adapter Process 27/27, strict Clippy green.
  dotlink `16475e4` consumes aw-process, deletes its parallel shell Process owner, stores one
  canonical optional ProcessTerminal, preserves tree compatibility, uses non-login Bash, and
  owns the 30-minute checkpoint / 35-minute hard conversational shell horizon.
  dotlink source graph + strict all-target Clippy are green. One real shell executable run
  reached 133/134 before the final shared drain fix; that sole failure is now the focused AW
  regression test. L5 externally SIGKILLs final dotlink codegen/link before test execution.

First revalidate current refs/status. Then follow the user's instruction.
R6 is already sealed. Continue Phase 0 only: rerun the grouped dotlink shell/Process/
Remote-Start executable gate on a host where linking is not externally killed and finish
outstanding Phase-0B owner/evidence work. Do not start Phase 1 before Phase 0 is accepted.

Keep implementation turns around 30 minutes and stop at the next coherent checkpoint.
~~~
