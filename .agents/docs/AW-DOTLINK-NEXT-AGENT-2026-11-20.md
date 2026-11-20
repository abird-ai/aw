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

Current published source state:

AW reusable source pin:

~~~text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
Complete v36 runtime admission and process semantics
~~~

dotlink:

~~~text
source checkpoint = 648519add3d174b9e13de81aeca62431591dac6d
published main     = bbd29c13904b7255eb28e2ed6c776c3590ac122a
~~~

Preserve unrelated AW working-tree material exactly, especially
`.agents/plans/ux/**` and the untracked
`.agents/plans/dl150-jobs-host-input-extension-2026-11-20/` tree.

The read-only release worktree is clean at local
`release/0.17 = a2be5098e990f78656db746e10f5652a494347c2`, 28 commits ahead
of published
`origin/release/0.17 = v0.17.5 = 3995448d7bf33794ef84c7bb51150d77cc260ada`.
Do not modify or reset that worktree.

All seven AW Git packages in dotlink Cargo.lock now resolve exactly to:

~~~text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
~~~

Published source checkpoints:

- AW reusable source pin:
  `b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b`;
- dotlink repinned source:
  `648519add3d174b9e13de81aeca62431591dac6d`;
- dotlink published main/docs tip:
  `bbd29c13904b7255eb28e2ed6c776c3590ac122a`.

The 0.17 D4-D7 shared-consumer implementation, semantic equivalence, AW v36
batch, dotlink process/control hardening and trusted Remote Start issuer cutover
are complete. Do not reopen them without a reproduced regression.

Completed AW source includes:

- v35 -> v36 migration/open coverage and projection backfill;
- issuer-bearing `RemoteStartLease` / `RemoteStartIssuer` v2 Start semantics;
- issuer-independent exact committed replay;
- indexed canonical Agent lookup and index-backed mixed Agent/finite listing;
- durable Agent/Turn source/wake/restart behavior;
- shared `ProcessTerminal::status()` where output completeness is orthogonal to
  known exit/stop truth.

Completed dotlink source includes:

- immediate reasonless managed-descendant cleanup on ordinary child exit;
- independent 250 ms post-exit output grace;
- escaped-`setsid` FD handling that preserves factual exit;
- direct shared process-status consumption with no compatibility shim;
- shared-owner-only revision-fenced cancel;
- stdin authority/revision linearization before bytes;
- one MCP instruction owner;
- separate ordinary OAuth and AW-bridge issuer authority;
- monotonic per-client trust epochs controlled by
  `oauth trust-aw-bridge` / `oauth untrust-aw-bridge`;
- 401/403/trusted issuer-bearing lease semantics;
- exact committed replay before current issuer trust, so revocation blocks new
  effects without erasing historical fact.

Final repinned dotlink gate on L5:

- main **706/0/1**;
- integrations **42/42**;
- strict all-target/all-feature Clippy, fmt/diff, Alejandra and Markdown green.

There is no remaining source implementation in this workstream that should be
started implicitly. Remaining boundaries are external/deferred only:

- native Windows Job Object runtime qualification;
- native macOS runtime/deployment qualification;
- trusted-selector Bubblewrap smoke on a suitable host;
- sandbox/config-v13, still frozen unless explicitly promoted.

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
