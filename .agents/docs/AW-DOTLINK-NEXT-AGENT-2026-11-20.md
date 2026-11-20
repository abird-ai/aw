# AW + dotlink next-agent starting prompt — 2026-11-20

Use `dotlink---l5` only.

Continue cross-repo work in:

- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`
- read-only release reference:
  `/home/pvl/spaces/abird/src/dotlink-017-mounts`

Do not use A1. Do not create another worktree. Preserve all unrelated dirty
work. Do not inspect or touch AW `.agents/plans/ux/**`. Do not implement the
frozen sandbox/mount/config-v13 plan unless I explicitly promote it.

Keep each implementation turn to roughly 30 minutes. Aim for the next coherent
checkpoint, update durable docs, then pause. Do not commit, push, tag, release
or deploy unless explicitly authorized.

Read first, in this order:

1. `/home/pvl/spaces/abird/src/aw/AGENTS.md`
2. `/home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
3. `/home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md`
4. AW 017 README / DESIGN / AUDIT / TODO
5. `/home/pvl/spaces/abird/src/aw/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
6. `/home/pvl/spaces/abird/src/dotlink/AGENTS.md`
7. `/home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md`
8. dotlink 017 README / TODO / D4-D7 source-cutover map /
   V0174 equivalence
9. dotlink DL-060 TODO
10. live source/tests for every owner you touch.

Current exact Git state:

AW:

~~~text
HEAD == origin/main ==
4234447612421069a746c925c76a25e3ba27774a
~~~

AW has a substantial **uncommitted** v36 admission/Agent-source batch. Do not
reset or overwrite it.

dotlink:

~~~text
HEAD = 20b6f045f79fa7725f286e05b40e9262b1c9e4d3
origin/main = 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
local branch = 4 commits ahead
~~~

dotlink also has **uncommitted** post-017 source hardening. Preserve it.

The read-only release worktree is clean at local `release/0.17 = a2be5098e990f78656db746e10f5652a494347c2`, 28 commits ahead of published `origin/release/0.17 = v0.17.5 = 3995448d7bf33794ef84c7bb51150d77cc260ada`. Do not modify or reset that worktree.

All seven AW Git packages in dotlink Cargo.lock still resolve to:

~~~text
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
~~~

The 0.17 D4-D7 shared-consumer implementation and semantic equivalence are
complete. Do not reopen them without a reproduced regression.

Current AW v36 batch is at a coherent **uncommitted** source checkpoint:

- all previously listed v36 admission/Agent-source/restart/notification work;
- explicit v35 -> v36 migration/open coverage with projection backfill and exact
  issuer-less v35 Remote Start replay;
- `RemoteStartIssuer { client_id, trust_epoch }` and
  `RemoteStartLease { target, issuer }`;
- issuer-bound v2 authorization digest for new Starts while issuer-less v1 facts
  remain exact replay-only history;
- issuer-independent committed Start recovery by immutable attempt+semantic
  identity before any new issuer/lease is obtained;
- indexed canonical Agent binding lookup with current Session authorization
  recheck;
- query-only `job_exposure_current` projection derived from immutable
  commit-log positions;
- index-backed/keyset mixed Agent/finite Job pagination under the existing
  position cursor contract;
- historical P9/D4-D7/v0.17.5 equivalence status reconciled as complete.

Validation already green on L5:

- focused migration/replay/participant/source/restart/wake/list regressions;
- aw-tools Remote Start 6/6, aw-store Remote Start 4/4, aw-mcp exact wire test;
- aw-store lib check and AW lib/bins check;
- strict `-D warnings` Clippy for aw-tools/aw-mcp all targets, aw-store lib,
  and AW all targets (`--no-deps`).

Repeated aw-store all-target check/Clippy attempts were externally SIGKILLed by
L5 with no compiler diagnostics; do not classify those as code failures. A
root Clippy run did find two real issues (an over-wide positional API and a
Rust-2024-only let-chain); both were fixed and subsequent root all-target strict
Clippy (`--no-deps`) is green.

Dotlink process/control hardening is now also at a coherent **uncommitted**
source checkpoint:

- ordinary waited-child exit closes managed descendants immediately as
  reasonless housekeeping;
- inherited output has a separate 250 ms post-exit grace;
- escaped-`setsid` retained FDs preserve known exit and mark output incomplete
  through shared `ProcessSettlement + Status.incomplete`;
- revision-fenced public cancel requires canonical shared ownership;
- stdin authority verification/revision acceptance is linearized before bytes;
- duplicate static MCP instruction ownership is removed;
- wait/read/cancel control tests use deterministic canonical shared Running
  Process fixtures rather than child sleeps;
- final L5 gate is **703/0/1 + 42/42**, strict all-target/all-feature Clippy,
  fmt/diff, Alejandra and Markdown **0 issues / 86 files**.

AW-side issuer identity/wire semantics are implemented, but current dotlink
still consumes published AW `1a5d0f65...`. Do not add a temporary path
dependency or duplicate the issuer type.

Next implementation order:

## 1. Preserve the two qualified dirty checkpoints

There is no remaining source item in the current AW v36 or dotlink
process/control batches that can be completed without crossing an explicitly
guarded boundary.

## 2. After explicit AW publish/repin authorization only

- publish/repin the issuer-bearing AW shared wire and corrected shared
  `ProcessTerminal::status()` projection;
- remove dotlink's temporary `shared_process_status` compatibility shim;
- make dotlink issue `RemoteStartLease` only for explicitly trusted OAuth AW
  bridge clients;
- bind the actual authenticated `client_id` and trust epoch;
- prove an ordinary authenticated OAuth client cannot self-mint a new Start;
- preserve exact committed replay after issuer revocation;
- rerun the cross-repo trust/replay/full gates.

Native Windows/macOS validation remains external. Sandbox/config-v13 remains
frozen unless explicitly promoted.

Do not:

- invent signals from timeout/cancel;
- merge NoTarget with Unknown;
- erase terminal facts after later failure;
- parse rendered receipt prose as identity;
- let response fitting mutate raw offsets/EOF/ACK truth;
- cache request-specific projections as canonical replay;
- mint a new replay timestamp;
- fabricate Agent Attention events;
- create another Job/event/policy runtime or registry;
- pull full aw-mcp into dotlink;
- implement sandbox/config-v13 without explicit promotion.

When done with a turn, report exactly:

- what changed;
- what was validated;
- what remains;
- Git status/ref state;
- whether anything was committed/pushed.
