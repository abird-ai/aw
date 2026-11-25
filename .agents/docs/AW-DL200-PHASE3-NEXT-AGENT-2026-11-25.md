# AW DL-200 Phase 3 — copy-ready next-agent prompt (2026-11-25)

Paste the following into the next implementation chat:

```text
Use dotlink---l5 as the ONLY source-authoring connector.

Continue in:
- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink
- read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated/concurrent
work and staging. Never list/read/inspect/write/stage/reset/stash/commit
AW/.agents/plans/ux/** or its staging. Physical Sandbox mount/config-v13 and
release/0.17 Sandbox implementation remain frozen unless explicitly promoted.
No push, tag, release, publish or deploy without separate authorization.

Read first, in order:
1. AW/AGENTS.md and AW/.agents/plans/README.md, then active plan ownership.
2. AW/.agents/docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25.md
   (current §§1–7, exact 24-checkpoint delta ledger and complete historical
   appendix with original architecture/decisions and all earlier chronology).
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. Same plan SPEC.md, IMPLEMENTATION.md, TODO.md, in order.
5. AW/.agents/docs/CURRENT_HANDOFF.md.
6. Historical initial design and implementation references indexed in
   the comprehensive handoff, especially R1–R5 resolved checklist, Nov 14
   unified runtime, Nov 21/22 R6, Nov 23 Phase 1, and Nov 24 Phase 3.
   Earlier status/next-action prose is historical; don't repeat completed cuts.
7. For cross-repo work: dotlink/AGENTS.md and its CURRENT_HANDOFF.md.
8. Live source and tests of every owner you intend to touch.
Live source and executed tests outrank dated prose; sealed SPEC is normative.

Latest verified signed implementation checkpoint:
2b4fac33d8345f284ecfd3e0585f332c72037dc6
Enforce trusted minimum Process containment before launch
Parent f3a7535262d588e08a8d058201467fcd3decf0e7.
A signed documentation-only checkpoint may sit above it. Revalidate
actual HEAD/main, exact parent, owned paths and staging before changes.
Store schema v60, Git recovery schema v4 (1–3 retained).
Phases 0/0B, 1, 2 complete and published. Phase 3 advanced but underway.
Phases 4–7 pending formal integration/qualification.

The interrupted minimum-containment implementation is ALREADY complete and
signed. Recovered validation: Process 8/8, adapter 258/258, Host 16/16,
source-effects-only Host 2/2, strict affected Clippy, workspace all targets/
features, scoped formatting/diff passed. Git 33/33 at prior affected cut;
Store 239/244 earlier combined, five known legacy failures remain.
No automatic production source-aware dispatch or strong Linux backend.
ClosedDescendants fails closed before Unix Direct spawn; Windows suspended
Job existing backend, runtime qualification deferred. Real Bubblewrap
worker-retirement also deferred. Do not redo completed construction,
derive, attach/replace/release (including empty layout), exact partial
replacement recovery, native artifact recovery or Vault finite projection.

Last independently verified remote AW main:
4ddeed8d0023cd0222633fc496f0d252e3dda54c
Local implementation after that is unpushed. Shared Process/R6 published
AW 20a63a4d4547cc20b2a154787823125b82f5e385.
dotlink main 85ceccc1088e201c98c994a46a68834440eeab32;
dotlink AW pin 20a63a4d4547cc20b2a154787823125b82f5e385;
frozen release/0.17 576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2.
Do not infer current remote truth from tracking refs or repin/publish.

NEXT: exact live source-aware Actor/Turn admission across the original
Agent/Actor/TurnJournal/context/queue owners. Recheck concurrent ownership:
crates/aw-agent-core/src/agent.rs, crates/aw-agent-core/src/journal.rs,
crates/aw-store/src/journal.rs were still occupied at handoff. Source-bearing
execution admission remains HELD. Do not open it from persisted grant,
configured router, driver registration or cached preflight. Require actual
independent live authority and original runtime participation. Do not
enable unsupported Graph/Job/Process/control routes.

If those paths remain occupied, choose ONE meaningful independent production
physical-authority/provisioning or containment/dispatch consumer through
existing owners. Keep frozen lower mount convergence untouched. Remaining
bound-Agent structural mutation, other baseline/layout policy cases, broader
Run-hazard recovery, remaining Vault broker/socket/file/env and persistent
provider configuration, protected metadata and host RPC surface are listed
in the handoff/TODO. Do not invent speculative admission layers or spend
turns subdividing already qualified structural mutation primitives.

Preserve stable DriverKey/schema compatibility and exact immutable receipts;
one original owner per truth; Prepared before effects; no blind retry;
Unknown retained; actual live independent provider/physical fence through
commit (persisted pin/handle is never authority); exact full-set source
claims; one atomic publication; original ToolStarted/ToolFinished and factual
physical custody; no Agent-to-Sandbox rebinding; no ambient fallback; no
secret leakage or arbitrary model-supplied authority. Preserve original
Process/Occupancy/stop evidence and future-only provider revocation.
Use static/zero-cost abstractions when practical. Elegant, simple, clear,
intuitive; take easy performance wins without convoluted maintenance.
Keep reusable primitives for future models/hosts. Target true hot paths.
Do not over-engineer test granularity, fixtures or evidence pinning.

Limit each implementation turn to approximately 30 minutes and pause at the
next coherent checkpoint. Run focused real grouped regressions and relevant
strict Clippy/check/fmt/diff. Update CURRENT_HANDOFF and active plan TODO
factually. Validate exact owned tree with private GIT_INDEX_FILE, check
expected parent, commit only owned paths signed (normal git commit --only -S
has worked; intent-to-add only new owned file if needed), verify parent/tree,
signature and clean owned paths, preserve unrelated staging. If a tool call
is denied or HEAD changed, stop and coordinate rather than bypass or retry.
Expected signature:
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
If interrupted, revalidate current refs/owned files and recover only your
original durable job results before restarting or committing.
Do not push unless separately authorized.

Begin by revalidating repository state, selecting the next non-conflicting
coherent implementation boundary, and proceeding with implementation.
```
