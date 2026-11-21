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

Current live refs:
  AW HEAD:      0d2dc10e20b62ebf0f064cdbc43615ae2d728ad7
  dotlink HEAD: fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94
  release ref:  576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

Published dotlink AW pin remains:
  7c1670821b15082821a0b9621cd1caccbe62f144

Important current state:
- R6 + shared Process/release-correction amendment are sealed and authoritative.
- AW corrective Process source is implemented locally/uncommitted and green.
- dotlink v0.17.9 product-policy + async controlled-Process consumer source are implemented
  locally/uncommitted and validated green only against the dirty local AW source.
- dotlink Cargo.toml/Cargo.lock intentionally still pin published AW 7c16708. An ordinary pinned
  build currently fails on the expected missing new async API (SpawnControl, async Process::spawn,
  pre-spawn error variants). Do not revert consumer source to the old API.
- Phase 0B is locally complete: replay disclosure, participant Provider/payer, AW policy tombstones,
  dotlink Remote Start tombstones and lifetime-history qualification are done.
- release/0.17 reference is exact v0.17.9 / 576d0a1 and read-only.
- real Bubblewrap worker-retirement is unavailable on L5 because the outer sandbox rejects bwrap's
  /proc mount; Windows-native qualification is also unavailable here.

Do not redo already-complete source cuts.

If the user asks to continue implementation without authorizing publication, only work on tasks that
are genuinely unblocked and do not require pretending the old AW pin has the new API.

If the user explicitly authorizes commit/publish/repin, the next sequence is:
1. revalidate both dirty trees and exact release ref;
2. commit/publish AW corrective source first;
3. atomically repin all dotlink AW Git dependencies to that AW revision;
4. re-diff release/0.17 from exact v0.17.9;
5. run ordinary pinned dotlink build + the already-green shell 86/86, Graph 13/13, real-Spawn-Lane
   cancellation, pre-spawn-timeout and spawn-refusal consumer gates;
6. run real Bubblewrap worker-retirement on a suitable host and Windows-native qualification;
7. review; then begin Phase 1.

Phase 1 starts only after that publication/platform gate is accepted. Phase 1 is the StoreScope /
StoreId + StorePos / Agent migration-history-disclosure foundation described in IMPLEMENTATION.md.

Keep implementation turns around 30 minutes and checkpoint at the next coherent green point.
Prefer simple owner-local Rust primitives, zero-cost/static abstractions and easy hot-path wins.
Do not create a generic TransactionManager, global replay ledger, universal revision clock,
generic Jobs+Events traversal engine, distributed Event router, public spawn-pool framework or
unused registry implementations.
~~~
