# Next agent — real two-AwNode remote Workspace acceptance

Use `dotlink---l5` only.

Canonical repo:

`/home/pvl/spaces/abird/src/aw`

Do not use A1. Do not create another worktree. Preserve all unrelated dirty or
staged work. Do not commit, push, tag, release or deploy unless explicitly
authorized.

Keep each model/user implementation turn near **30 minutes**. Aim for the next
coherent checkpoint, update durable state, then pause.

## Read first

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
5. `.agents/docs/CURRENT_HANDOFF.md`
6. `.agents/plans/workspace-views/TODO.md`
7. `.agents/plans/workspace-views/REMOTE-DEVICE-TRUST.md`
8. `.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
9. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
10. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
11. live source/tests for the exact owners you touch.

For any UI/UX change, read `.agents/plans/ux/**` as **read-only authority**.
V1.5 is current. Never modify that subtree.

## Revalidate before editing

- `git rev-parse HEAD`
- `git rev-parse origin/main`
- `git status --short --branch`
- `git diff --cached --name-status`
- `git worktree list --porcelain`
- `git diff -- .agents/plans/ux`
- `git diff --check`

Expected pushed HEAD:

`62b8da6babfa729d6bbb9c21e75ac89c228b22b1`

Expected current tree is dirty with the production remote-view server CLI and
browser-origin/SSH-forward changes recorded in the comprehensive handoff.
Preserve them.

## Mission

Continue the **real production-authorized two durable AwNode proof** from the
first protected-read transport boundary. Do not restart authority design.

Use two independent durable state roots on L5.

**Do not hand-author a continuity snapshot or v35 grant.**

Use the actual production pairing, Phase-30 continuity, identity-auth and
Machine-service commands.

### Current executed checkpoint

The L5 production gate has already established with public `aw` commands:

- two fresh file-backed durable AwNodes;
- real direct pair/trust;
- source Domain/device and explicit target Machine;
- signed Workspace writer handoff A → B;
- on the imported replica Domain, explicit registration of only the exact
  trusted recorded control AwNode as requester remote Machine;
- real Phase-30 continuity accept + signed sync;
- requester device proof;
- target Files+Git remote-view issuance policy;
- real **server-issued v35 grant**;
- owner-private `aw web` admission and opaque `rwt_...` browser descriptor;
- no browser descriptor leakage of peer/room/Domain/Workspace/grant/device-key
  authority material.

The first protected capability read is the current blocker. The HTTP endpoint
returns factual 503 unavailable. Temporary diagnosis-only logs (already
removed) established that the direct QUIC connection gets through
`open_bi/accept_bi`, then the client send half is closed before the first
RouteHello bytes (`ClosedStream`) and the server sees zero-byte FIN
(`FinishedEarly(0)`).

Isolation already green:

- same-room `net punch` twice consecutively;
- plain Machine FileRead;
- identity-auth → same-room Machine FileRead;
- two sequential same-room one-shot Machine services;
- real remote-view v3 grant → clean web-owner stop → same durable requester
  state + same room plain Machine FileRead.

The remote HTTP path already uses `tokio::task::spawn_blocking`.

**Next engineering target:** isolate and fix only the in-web-owner protected-read
direct-route/QUIC stream lifecycle/context, then resume the production gate at
remote target capabilities. Do not weaken v35, owner custody, browser secrecy,
exact-source pins or no-Local-fallback to make the test pass.

The current uncommitted support fixes are intentional:

- exact replica-control remote-Machine registration without owner/admin import;
- replica-only v35 member evidence tied to the exact accepted Phase-30 source;
- exact local-hosted Workspace lookup for remote service;
- resident Workspace-bound Machine service retaining one `SurfaceHost`;
- exact-generation reusable-rendezvous retransmit suppression.

Post-cleanup focused evidence is green: rendezvous **1/1**, v35 **4/4**,
Phase-30→explicit replica Machine→v35 **1/1**, writer handoff/failback **1/1**,
formatting and current CLI build.

### Server grant policy

The target identity-auth server must opt in explicitly. Current CLI help spells
the flattened flags with the `--net-` prefix:

`identity-auth-serve --net-remote-view-max-ttl <ttl>
  --net-remote-view-grant files
  --net-remote-view-grant git`

Use only the exact surface grants needed.

### Server read service

Serve the exact hosted Workspace/root using:

`machine-service-serve ... --net-remote-view-workspace <workspace>
  --net-root <root>
  --net-grant identity-authenticate
  --net-grant file-read
  --net-grant git-read`

The selected Workspace must be the exact active hosted Workspace ID on that
local Machine. Remote-view mode intentionally rejects a simultaneous Domain
selector.

### Required proof

Reach a coherent checkpoint in this order:

1. [established] two independent durable AwNodes exist;
2. [established] real pairing/trust;
3. [established] exact Domain/Machine/replica-control relationships;
4. [established] real Phase-30 identity continuity;
5. [established] requester device proof;
6. [established] target issues real v35 grant;
7. [established] opaque remote target is admitted and browser-visible;
8. [**current blocker**] at least one protected remote Workspace capability +
   File/Git read succeeds.

If that boundary is green within the turn, continue only to the next coherent
step:

9. Files search;
10. Git inventory;
11. Git open;
12. retained immutable Review ID.

Leave reconnect/revoke for the following checkpoint if the turn is approaching
the 30-minute bound.

## Preservation assertions

Before and after remote selection/read, record:

- Agent Session/branch identity;
- draft/caret;
- provider/model target;
- Review/Build PTY IDs;
- PTY input/resize owner;
- local Git HEAD/index;
- provider/model/tool/run/effect rows.

Target selection must not retarget any of those.

## Browser forwarding contract

Preserve the current uncommitted forwarding design:

- `--listen` = backend loopback socket;
- optional `--browser-origin http://LOOPBACK[:PORT]` = exact browser-visible
  Host/Origin for SSH forwarding;
- do not weaken Host/Origin/Sec-Fetch checks;
- do not accept non-loopback browser origins.

## Do not do

- do not modify `.agents/plans/ux/**`;
- do not reimplement `terminal-promote`;
- do not resurrect retired job/subagent routes;
- do not grant Files root from one File;
- do not grant Git inventory/write from one Review;
- do not treat pairing/browser auth/path/label/tab/ticket as resource authority;
- do not expose route/grant/key material to browser state;
- do not retarget Agent/model/provider/PTY from ActiveTarget;
- do not silently fall back Local after remote loss;
- do not auto-start or claim a PTY from view selection;
- do not claim the two-node gate unless the production flow actually ran.

Update `REMOTE-DEVICE-TRUST.md`, Workspace `TODO.md`, this file and
`CURRENT_HANDOFF.md` with exact factual evidence at the checkpoint.
