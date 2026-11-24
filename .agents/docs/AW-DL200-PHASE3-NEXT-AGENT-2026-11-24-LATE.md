# AW DL-200 Phase 3 — copy-ready next-agent prompt (2026-11-24 late checkpoint)

Paste the following into the next implementation chat:

```text
Use dotlink---l5 as the ONLY source-authoring connector.

Continue in:
- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink
- read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve all unrelated/concurrent
work and staging. Never list/read/inspect/write/stage/reset/stash/commit
AW/.agents/plans/ux/** or its staging. Physical Sandbox mount/config-v13 and
release/0.17 Sandbox implementation remain frozen unless explicitly promoted.
No push, tag, release, publish or deploy without separate authorization.

Read first:
1. AW/AGENTS.md and .agents/plans/README.md, then the active plan/ownership.
2. AW/.agents/docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md
   (canonical; complete signed chronology and detailed Phase-3 owner record).
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. Same plan SPEC.md, IMPLEMENTATION.md, TODO.md, in order.
5. AW/.agents/docs/CURRENT_HANDOFF.md.
6. Historical design and implementation provenance referenced in the handoff:
   DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md,
   UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md,
   AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md,
   AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md,
   earlier Nov 24 Phase-3 handoff (historical).
7. For cross-repo work, dotlink/AGENTS.md and its CURRENT_HANDOFF.md.
8. Live source/tests for every owner you touch. Live source/executed behavior
   outrank stale dated prose.

Current signed IMPLEMENTATION checkpoint beneath docs:
0bf9bf6cbdce3d0929885198f93528572b85cf2b
Restore baseline initial tasks through existing encrypted custody
Parent 85028a4ed680760c9dbf3e198a1ec18d1c006421.
Store schema v58. Phase 0/0B published; Phase 1 complete locally;
Phase 2 complete locally through v54; Phase 3 underway; Phases 4–7 pending.
Task-only None/empty Auto baseline restore and explicit resume/abandon already
landed. Do not redo that or earlier completed cuts. A docs-only commit may sit
above the implementation checkpoint. Revalidate HEAD/main and exact scoped
owner paths before writing.

Carried verified publication baseline (not local tracking-ref truth):
AW remote main 78339410710472ec51d5ff46a5c9e586b67ff30d;
AW shared Process/R6 20a63a4d4547cc20b2a154787823125b82f5e385;
dotlink main 85ceccc1088e201c98c994a46a68834440eeab32;
dotlink AW pin 20a63a4d4547cc20b2a154787823125b82f5e385;
release/0.17 576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9.
Do not repin/publish or infer actual remote state from local origin/main.

NEXT: exact live source-aware Actor/Turn admission across the original
Agent/Actor/TurnJournal/context/queue owners. At handoff,
aw-agent-core/src/{agent,journal}.rs and aw-store/src/journal.rs are dirty
under another agent's ownership. Recheck exact status; do not touch those
or other concurrent paths unless ownership is clear. Source-bearing
execution admission remains HELD. The native host route, real Git resolver
and independent pinned approval exist, but production provisioning and
complete runtime participation are not yet promoted. Do not infer a live
grant from persisted evidence or enable unsupported Job/Graph/Process/control.

If still blocked, take ONE independent actual production host/restore
composition or effectful fresh baseline Prepared custody boundary under
independent authorized physical/provider fences. VaultProvider/VaultGrant
remains a sealed Phase-3 requirement; implement only a coherent owner +
actual consumer slice when its prerequisites are satisfied. Use live TODO
and the remaining-boundary table. Do not invent speculative admission
layers to work around another owner. Do not claim Phase 3 complete until
production containment/dispatch, provisioning, checkpoint/Sandbox/Vault
RPCs, recovery and grouped qualification are actually green.

Preserve: stable DriverKey + artifact schemas; add-only leaves until complete
durable dependency proof; source set 128 effective / 1024 absolute; original
Prepared before effects; inspect exact descriptor, never blindly retry;
Unknown stays Unknown. Immutable evidence or live provider-held fence
through final commit (persisted handle is identity only). One final atomic
publication; wakes are reconstructible. Restore always fresh Agent + Sandbox,
no inherited secrets, credentials, leases, device/network authority, active
Processes or staged inputs. Dirty Git uses private synthetic commit, never
user-visible synthetic HEAD. Retention/reconciliation conditional and exact,
no arbitrary destructive cleanup. Use same Agent/History/ACL/Task/Queue
constructor; no fake empty source barrier for task-only baseline. No
TransactionManager/OperationManager, global replay ledger/revision clock,
generic Jobs+Events engine, second Process/Event/Project/Session/Source/ACL
owner, Agent-Sandbox rebind, public spawn pool, cross-store v1 delivery or
arbitrary Vault projection target language. Phase 7 owns compatibility removal.

Keep implementation elegant, simple, clear and intuitive. Composable,
reusable Rust primitives, static/zero-cost where practical; easy performance
wins, targeted hot-path improvements, clean/simple first otherwise. No
over-engineered fixtures or evidence pinning. One grouped real
failure-boundary regression per owner. Limit each turn to ~30 minutes;
checkpoint and pause at the next coherent point.

At each checkpoint: focused real tests, strict relevant check/Clippy/fmt/
scoped diff; factual CURRENT_HANDOFF + plan TODO; exact owned paths only
using private GIT_INDEX_FILE; signed commit with expected-parent CAS on main;
verify signature and owned cleanliness; pause. Do not count zero-test
filters or deferred real Bubblewrap/Windows-native evidence as green.
Expected signature:
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
No remote action unless separately authorized.

Start by revalidating the actual repository state, selecting the next
non-conflicting coherent implementation boundary, and then implement it.
```
