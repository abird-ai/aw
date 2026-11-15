# CURRENT HANDOFF — AW current implementation state

Author: coordinating implementation agent
Owner: @pvl
Prepared: 2026-11-15
Source authority host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`

> **Full current handoff:** read
> `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`.
>
> That file integrates the original AW architecture, shared tools/dotlink
> convergence, unified Jobs + Agent runtime, Workspace/PTY/browser history,
> delegated authority, V1.5 UX/Build, remote trust/projection/admission/
> ActiveTarget, the current uncommitted production server/browser-origin work,
> exact validation evidence and the live next gate.
>
> Authority order: live source/tests/executed results > direct user instruction >
> read-only UX authority > active plan/TODO/spec > comprehensive handoff > older
> dated prose.

## Immediate state

- Branch: `main`
- HEAD / `origin/main`:
  `62b8da6babfa729d6bbb9c21e75ac89c228b22b1`
  — `Complete remote Workspace target integration`
- Nothing staged.
- One worktree.
- `.agents/plans/ux/**` remains read-only and has no diff.
- Use `dotlink---l5` only. Do not use A1.
- Do not create another worktree.
- Do not commit/push/tag/release/deploy without explicit authorization.
- Keep implementation turns near 30 minutes; checkpoint and pause.

## Current uncommitted paths

Preserve all of these:

- `.agents/docs/CURRENT_HANDOFF.md`
- `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
- `.agents/plans/workspace-views/README.md`
- `.agents/plans/workspace-views/NEXT-AGENT.md`
- `.agents/plans/workspace-views/REMOTE-DEVICE-TRUST.md`
- `.agents/plans/workspace-views/TODO.md`
- `crates/aw-store/src/remote_view.rs`
- `crates/aw-store/src/resources.rs`
- `crates/aw-store/tests/resources.rs`
- `crates/aw-surfaces/src/lib.rs`
- `src/awnet/reachability.rs`
- `src/main_full.rs`
- `src/resources_cli.rs`
- `src/web.rs`
- `src/web/agent_view_tests.rs`
- `src/web/team.rs`

The uncommitted implementation contains:

1. production remote-view server CLI enablement:
   - `identity-auth-serve --remote-view-max-ttl ... --remote-view-grant files|git`
   - `machine-service-serve --remote-view-workspace WORKSPACE`
   - exact server MachineLink/read policy and hosted-Workspace preflight;
   - retained read-only SurfaceHost projection;
   - read-only SurfaceHost no longer needs a fake shell path.
2. SSH-forwarded browser authority:
   - `--listen` = backend loopback socket;
   - `--browser-origin http://LOOPBACK[:PORT]` = exact browser-visible authority;
   - exact Host/Origin/Sec-Fetch policy retained;
   - browser-authority-scoped cookie names.

## Current accepted major checkpoints

- unified Jobs/Agent runtime: `0a6c6a0` / handoff `88cd965`
- delegated browser authority: `c9aaf0e`
- V1.5 Workspace UX + remote trust: `aaab161`
- remote Workspace target integration: `62b8da6`

## Latest production two-AwNode checkpoint

The real L5 production gate has now been executed from two fresh file-backed
durable AwNode state roots using public `aw` commands only. No continuity state,
v35 grant or SQLite authority was hand-authored.

Established through production commands:

- direct QUIC pair/trust;
- source Domain/device setup and explicit target Machine registration;
- signed Workspace writer handoff A → B;
- on the replica host, explicit registration of the exact trusted recorded
  control AwNode as the requester remote Machine;
- real Phase-30 continuity accept + signed sync;
- real device-key identity authentication;
- target identity-auth server explicitly opted into Files+Git v35 issuance;
- **server-issued v35 remote-view grant succeeded**;
- `aw web` admitted the grant and exposed only a fresh opaque `rwt_...` target;
  the browser descriptor contained no peer/room/Domain/Workspace/grant/key
  authority material.

The first protected remote capability read is still blocked. While the same
`aw web` owner is live, the direct-route stream for the Machine-service request
opens but closes before the first RouteHello bytes are written. Temporary local
diagnostics (removed before this checkpoint) observed Quinn
`ClosedStream` on the client and `FinishedEarly(0)` on the server; the web API
correctly collapsed this to HTTP 503 `remote Workspace target unavailable`.

Isolation evidence rules out the broad authority/rendezvous/MachineLink paths:

- same-room `net punch` succeeds twice consecutively;
- plain Machine service FileRead succeeds;
- identity-auth → same-room plain Machine service succeeds;
- two sequential same-room one-shot Machine-service connections succeed;
- real remote-view v3 grant issuance followed by clean `aw web` owner shutdown
  and same-state/same-room plain Machine read succeeds.

Therefore the remaining core gate is specifically the in-web-owner
protected-read direct-route/QUIC stream lifecycle. Do not weaken v35, owner
custody, browser secrecy or exact-source pinning to bypass it.

Current uncommitted fixes supporting the production path also include:

- imported replica Domains may explicitly register only their exact trusted
  recorded control AwNode as a remote Machine; ordinary Domains still require
  owner/admin + Domain-authority write;
- replica-only v35 membership evidence is accepted only from the exact active
  Phase-30 binding source recorded for that replica Domain; no owner/admin role
  is materialized;
- remote Machine service resolves only an exact active hosted Workspace ID
  owned by the local Machine in remote-view mode;
- Workspace-bound remote Machine service remains resident across fresh
  MachineLink transports so one retained `SurfaceHost` owns a stable source/
  scope and immutable inventory/Review pins;
- reusable rendezvous ignores only exact recently matched `(node_id, nonce)`
  retransmits; fresh generations reuse the room immediately.

Focused post-cleanup evidence is green: rendezvous generation regression **1/1**,
v35 authority **4/4**, Phase-30 → explicit replica Machine → v35 **1/1**,
Workspace writer handoff/failback **1/1**, formatting, and current CLI build.

Still not established and must not be claimed:

- production Files/search/Git inventory/open/retained Review;
- reconnect/re-read of those exact-source resources;
- revoke → stale/unavailable/no-Local-fallback after an actual protected read;
- live non-empty draft/caret and PTY preservation across remote selection.

When the transport defect is fixed, resume the same production path at explicit
remote ActiveTarget capability/read, then prove target selection/reconnect/
revoke does not change:

- current Agent Session/branch;
- unsent draft/caret;
- provider/model execution target;
- local Review/Build PTY inventory;
- PTY input/resize owner;
- local Git HEAD/index;
- model/provider/tool/run/effect rows.

No silent Local fallback is allowed when the remote target disappears.
Standalone pinned File/Review must remain on its original source until its exact
authority is revoked.

For the exact production commands, invariants, source map and test history read
the comprehensive handoff.
