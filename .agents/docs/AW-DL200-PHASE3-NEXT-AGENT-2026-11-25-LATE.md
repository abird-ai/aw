# AW DL-200 Phase 3 — copy-ready next-agent prompt (2026-11-25 late)

Paste the following into the next implementation chat:

```text
Use dotlink---l5 as the ONLY source-authoring connector.
AW: /home/pvl/spaces/abird/src/aw
dotlink: /home/pvl/spaces/abird/src/dotlink
Frozen read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1 or create another worktree. Preserve unrelated/concurrent work
and staging. Never list/read/inspect/write/stage/reset/stash/commit
AW/.agents/plans/ux/** or its staging. Physical Sandbox mount/config-v13 and
release/0.17 remain frozen. No push, tag, release, publish or deploy without
separate authorization.

Read first, in order:
1. AW/AGENTS.md and AW/.agents/plans/README.md, active plan ownership.
2. AW/.agents/docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25-LATE.md
   (current §§1–7; §8 complete ten new checkpoint records; Appendix A full
   previous handoff with all earlier chronology, original architecture and
   decisions, and links to the beginning-to-current design lineage).
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md,
   SPEC.md, IMPLEMENTATION.md, TODO.md in order. R6 SPEC remains sealed.
4. AW/.agents/docs/CURRENT_HANDOFF.md (latest actual incremental records).
5. Relevant original designs/handoffs indexed in the comprehensive handoff,
   including original AW foundations, unified runtime/DL-150, R1–R5 resolved
   checklist, Nov 21/22 R6, Phase 1/2 and earlier Phase 3. Prior pending/status
   statements are historical; do not duplicate completed cuts.
6. For cross-repo work, dotlink/AGENTS.md, CURRENT_HANDOFF.md and live pin.
7. Live source and tests of every owner you intend to touch.
Live source and executed tests outrank dated prose; sealed SPEC is normative.

Latest verified signed implementation checkpoint:
ffa8fc940fd82468925be3505adf7aa6082399b4
Expose replay-safe empty-baseline checkpoint restore through trusted stdio
Parent 077ac07803d9fa19db9743393cc4e68f2a6f4676.
A signed documentation-only checkpoint may sit above this. Revalidate actual
HEAD/main, expected parent, owned paths and signature before changes.
Store v60, Git binding/anchor/recovery 1/2/4 (prior recovery supported).
Phases 0/0B, 1, 2 complete and published. Phase 3 advanced but not complete;
Phases 4–7 pending formal integration and qualification.

Ten subsequent implementation cuts are already signed and complete within
stated scope: local shell containment + trusted startup minimum, Git driver
production owner startup, persistent signing Vault configuration, registry-
bound SourceDriver handles and complete Git artifact-state preflight, native
single-link file policy, explicit configured Git native approval, redacted
Sandbox/checkpoint metadata, history/empty Auto checkpoint creation and
explicit empty-baseline task-less restore. Do not repeat these cuts. Source
admission, arbitrary source Process/Job routes, real physical authority,
remaining Vault projections/product activation and source-aware checkpoint/
Sandbox host mutations are not thereby completed. Latest headless 8/8,
Store-backed port 1/1, original restore owner 1/1, strict Host/full product
Clippy, durable/minimal checks, binary build and CLI opt-in smoke passed.
L5 Rust 1.95.0, pinned Nix 1.98.1 not requalified; previous broad and native
platform blockers retained. Strict Unix ClosedDescendants refuses; Windows
and real Bubblewrap qualification deferred. Do not claim complete acceptance.

Last independently verified AW remote main:
4ddeed8d0023cd0222633fc496f0d252e3dda54c
19 local first-parent commits beyond that tracking ref at implementation tip;
no later authorized push. dotlink HEAD/main 85ceccc1088e201c98c994a46a68834440eeab32;
local origin/main fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94 (older tracking
ref; no new remote query). AW pin 20a63a4d4547cc20b2a154787823125b82f5e385;
frozen release 576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2. No repin or publication.

NEXT (priority): exact live source-aware Actor/Turn admission across the
ORIGINAL Agent/Actor/TurnJournal/context/queue owners. Revalidate current
ownership: crates/aw-agent-core/src/{agent,journal}.rs,
crates/aw-store/src/journal.rs and src/resources_cli.rs were concurrently
occupied. Do not touch others' dirty files. Source-bearing execution is HELD.
Require actual independent live source authority, exact binding and
ToolBinding/actor/site/writer, original journal/queue participation and real
approved native Actor end-to-end/restart/revocation qualification. Never open
admission from persisted grant, registered driver, configured router or cached
preflight. No speculative admission wrapper or unsupported Graph/Job/Process/
control promotion. If original owners remain occupied, choose ONE substantive
independent actual host authority/containment consumer or remaining sealed
resource surface, e.g. configured Vault activation, actual protected metadata
authority, checkpoint retirement with original retention/replay owner, or
complete actual Process authority. Coordinate resource CLI when available.
Do not invent another configuration wrapper or endlessly subdivide already
qualified structural mutation primitives. See §5 of latest handoff/TODO for
full remaining acceptance and Phases 4–7.

Preserve one original truth, stable DriverKey/schema, complete source sets,
Prepared-before-effect, replay-first exact immutable receipts, no blind retry,
Unknown custody, live provider/physical fence through commit, original
ToolStarted/Finished and factual Process/Occupancy/stop evidence, immutable
Agent-Sandbox binding, no ambient fallback, no secret leakage or model-supplied
authority. Pins, private paths and installed definitions are observations not
physical authority. Preserve original feature boundaries and XDG paths.

Implementation style: elegant, simple, clear, intuitive composable Rust lib;
static/zero-cost abstractions where natural; reusable primitives for other
models/hosts; easy performance wins, targeted hot-path performance and
maintainability first. No overengineered granular tests/evidence pinning.
One focused real grouped owner regression and meaningful integration gate.
Limit each turn to ~30 minutes; checkpoint at next coherent tested boundary
and pause. No background promises.

Run actual relevant tests/strict Clippy/check/fmt/diff. On L5 export AW_GIT and
AW_BASH from command -v for real Git/shell tests; offline locked cargo. If a
long command returns a job handle, poll/read that original result and ack
only your own completed job. If interrupted, revalidate actual refs, scoped
owned status and original job result before continuing; do not blindly rerun.
Update CURRENT_HANDOFF and active plan TODO factually, retain limitations and
unchanged pending items. Use private index to validate exact expected-parent
owned tree; signed commit only owned paths, verify exact parent/tree/signature
and clean owned paths. Never inspect or change other owners' staging. If HEAD
changes or tool denies an operation, stop and coordinate, do not bypass.
Expected signature: pvl@prasannavl.com ED25519
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI.
Nothing pushed without separate authorization.

Begin by revalidating repository state and selecting the next non-conflicting
coherent implementation boundary. Proceed with implementation, not another
plan-only turn, unless actual ownership/authority blocks the work.
```
