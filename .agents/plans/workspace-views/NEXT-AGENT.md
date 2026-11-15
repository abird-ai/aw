# Next agent — delegated File/Review shipping integration

Use `dotlink---l5` only.

Canonical repo:

`/home/pvl/spaces/abird/src/aw`

Do not use A1. Do not create another worktree. Preserve all unrelated dirty or
staged work. Do not commit, push, tag, release or deploy unless explicitly
authorized.

Keep one model/user implementation turn near ~30 minutes. Aim for the next
coherent checkpoint, record state, then pause.

## Critical UX authority rule

The user explicitly designated:

`.agents/plans/ux/**`

as the **source of truth for all UI and UX**.

You are explicitly authorized to **read/inspect that subtree** for this task,
despite older repository/handoff language that said not to inspect it.

You are **not** authorized to modify it.

- never write there;
- never stage/reset/stash/commit it;
- do not regenerate its previews/assets;
- keep implementation plans and acceptance evidence in
  `.agents/plans/workspace-views/**` or `.agents/docs/**`.

V1.5 is the current UX target.

## Read first

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. `.agents/docs/WORKSPACE-VIEWS-UX-COMPREHENSIVE-HANDOFF-2026-11-14.md`
5. `.agents/docs/CURRENT_HANDOFF.md`
6. UX authority, read-only:
   - `.agents/plans/ux/README.md`
   - `.agents/plans/ux/V1.5-UNIFIED-FOCUS-AND-TABS.md`
   - `.agents/plans/ux/NAVIGATION-AND-STATE.md`
   - `.agents/plans/ux/INTERACTIONS-AND-ACCEPTANCE.md`
   - `.agents/plans/ux/VISUAL-SPEC.md`
   - `.agents/plans/ux/TERMINAL-V1.5-CELL-STUDY.md`
   - relevant read-only v1.5 JS/tests when implementing an interaction
7. `.agents/plans/workspace-views/README.md`
8. `.agents/plans/workspace-views/TODO.md`
9. `.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
10. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
11. `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md`
12. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
13. live source/tests and the current dirty diff

Source/tests/executed results outrank prose.

## Current live state

AW HEAD:

`c9aaf0e0d311ba905f55875147392a071f64b037`

`Complete delegated browser authority and surface lifecycle`

`origin/main` is at the same commit.

Nothing is staged.

There are 14 dirty **implementation/test/generated** paths. Preserve them; they
are one current in-progress product slice:

- `src/team.rs`
- `src/web/dock.rs`
- `src/web/team.rs`
- `src/web/team/delegated.rs`
- `src/web/team/tests.rs`
- `src/web/workspace.rs`
- `tests/web_docking_handoff.test.mjs`
- `tests/web_workspace.test.mjs`
- `web/dist/app.js`
- `web/dist/style.css`
- `web/src/document-docking.js`
- `web/src/main.jsx`
- `web/src/styles.css`
- `web/src/workspace-views.js`

Do not reset or reconstruct them.

## What is already complete

### Unified runtime

Finalized and unrelated to the next UI slice:

- AW runtime: `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
- AW handoff sync: `88cd9655d402ea325873470202fd12133b1d8199`
- dotlink: `c90a6603ccef0bbf626ce10d95bedf59877747b8`

Do not reopen old Jobs/Agent runtime work or resurrect
`subagent/team/plan/review_release/job_*` model routes.

### Workspace views

Already implemented/accepted:

- native read-only Agent views;
- scoped native Files/Git containers;
- browser Agent views;
- browser scoped Files/Git;
- same-window and cross-window File/Review docking;
- same-owner independent-browser pairing;
- same-machine terminal docking;
- persistent broker-owned PTYs;
- same-UID Linux original-shell `terminal-promote`;
- actual different-UID FD admission refusal.

Do not reimplement Linux PTY migration.

### Distinct user/Actor delegated browser authority

Committed at current HEAD `c9aaf0e`:

- exact memory-only delegated grant registry with generation/revocation/expiry;
- Agent-branch distinct-Actor lifecycle and shipping Solid integration;
- real two-profile Chrome 157 Agent-branch acceptance;
- exact File/immutable-Review Surface read substrate;
- opt-in trusted delegated SurfaceHost composition;
- complete distinct-Actor File/Review backend HTTP lifecycle:
  register / prepare / receive / read / accept / confirm / heartbeat / release /
  status / cancel / revoke;
- ordinary Team `/api/workspace` remains disabled;
- recipient receives no Files root, Git inventory/context, terminal, Site or
  execution authority.

Read
`.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
for exact evidence.

## Current accepted dirty slice — preserve it, do not redesign it

The current 14 dirty implementation/test/generated paths are the accepted
**File/Review shipping integration**. They remain uncommitted.

### Team CLI/source composition

`src/team.rs`

- explicit `delegate_workspace_views: bool`, default false;
- changes configuration fingerprint;
- opted-in owner gets a trusted read-only delegated source;
- normal participant Workspace authority remains absent;
- terminals disabled, Sites empty.

### Team delegated source routes

`src/web/team.rs`, `src/web/workspace.rs`

- narrow delegated source capability/read endpoints;
- retained owner SurfaceHost only;
- strict auth/identity revalidation;
- bounded action body/time;
- no ordinary recipient `/api/workspace` enablement.

### Browser document lifetime

`web/src/workspace-views.js`

- delegated File/Review document has exact per-grant reader + authority key;
- ordinary source replacement does not scrub unrelated delegated documents;
- delegated 403 scrubs only that exact grant/document cache/state;
- global auth expiry clears all;
- delegated document cannot move into an unrelated Workspace scope.

### File/Review delegation flow

`web/src/document-docking.js`

- `#aw-surface-dock=` correlation fragment;
- exact File/Review send/receive;
- recipient read before Accept;
- confirm/heartbeat/release;
- no protected bytes in ticket;
- grant generation bound into receiver lifetime.

### Shipping Solid

`web/src/main.jsx`, `web/src/styles.css`

- distinguish ordinary Workspace source vs trusted Team delegated source;
- explicit recipient selector and private read-only link for owner File/Review;
- received delegated document is standalone/read-only;
- no re-delegation or move into unrelated scope;
- auth expiry scrubs delegated state.

This 14-path File/Review product slice is now an **accepted checkpoint** in the
working tree. It remains uncommitted.

### Bounded presentation transfer — implemented in this dirty tree

The new distinct-Actor File/Review route now reuses the existing bounded v1
presentation envelope:

- File: Find intent/result, caret/selection and viewport/scroll;
- Review: two-pane Find, selection, scroll and focused pane;
- presentation remains ticket metadata only and never changes delegated
  resource/grant/recipient/generation/authority identity;
- mutable File fingerprints are rechecked against the exact owner read at
  prepare, receiver Accept and owner Release;
- receiver read/render/fingerprint/restore/target recheck completes before
  Accept;
- recipient chooser/button focus staging preserves the document's presentation
  focus without granting authority.

Targeted tests already authored in the dirty tree cover:
- owner-only delegated source capability without ordinary Workspace authority;
- exact File send/receive with no protected bytes in the ticket;
- correlation-fragment stripping;
- revoked receiver document scrub;
- delegated document surviving ordinary source replacement;
- distinct-grant cache isolation for the same File;
- full auth expiry scrubbing ordinary + delegated caches.

The concurrent owner has now recorded this checkpoint green:

- Team module **48 passed / 1 intentional ignored**;
- Team config **12/12**;
- browser/controller **187/187**;
- strict aw-cli library Clippy and default binary check;
- Vite build 174 modules;
- two consecutive builds byte-identical for all **8/8** embedded assets.

Additional executed evidence from the presentation checkpoint:

- `node --test tests/web_docking_handoff.test.mjs` — **16/16 passed**;
- focused delegated File lifecycle — passed;
- focused immutable Review lifecycle with pinned `AW_GIT` — passed;
- held-response revoke fencing — passed;
- reduced Team+Surfaces library check — passed with only the existing
  `bind_execution_owner` warning;
- formatting/diff check — passed before durable-doc edits;
- fresh Vite builds — 174 modules, deterministic **8/8** assets.

Do **not** repeat those gates by habit. Real Chrome 157 now closes the
File/Review browser boundary. Separate fresh-profile File and Review runs at
390×844 proved distinct Alice/Bob principals and cookie values, exact resource
identity, fragment stripping, no recipient Workspace/root/Git authority,
presentation restore, factual Release, explicit File Revoke scrub, File
reload/no-replay, Review logout scrub, unchanged source/receiver Agents and
source draft/caret, zero overflow/page JS errors, unchanged synthetic Git,
provider count 0 and zero run/model/tool/effect rows.

The live gate also exposed and fixed a real cold editor-mount defect: the full
split editor chunk could finish downloading while Solid `lazy()` stranded
CodeMirror lifecycle. The product still code-splits the ~1.50 MB editor module,
but now uses one cached explicit dynamic import awaited before ordinary
File/Review admission and both docking receiver paths. Post-fix focused
browser/controller tests are **78/78** and final Vite output is deterministic
**8/8**.

## Current remote-device trust checkpoint — backend/protocol accepted, product open

The current uncommitted tree also advances the next separate workspace-view
boundary. Read [REMOTE-DEVICE-TRUST.md](REMOTE-DEVICE-TRUST.md) before changing
it.

Implemented and green:

- SQLite schema **v35** durable remote-view grants;
- exact portable device + stable user + active Domain membership;
- Domain policy/security epoch fences;
- trusted peer plus explicitly registered requester remote Machine;
- requester/target Machine security + capability revisions;
- exact hosted Workspace revision + policy revision;
- bounded Files/Git permissions, TTL, explicit revoke and reconnect revalidation;
- additive Phase-30 identity-auth v3 only for remote-view grant issuance while
  legacy identity auth stays byte-compatible v2;
- additive Machine-read v2 only for protected `View*` reads while legacy reads
  stay byte-compatible v1;
- fresh device signature per protected read, bound to lease/request/grant/
  Workspace/request digest;
- final grant reauthorization immediately before response publication;
- remote-view server rejects legacy reads and ordinary server rejects `View*`,
  closing downgrade bypass.

Evidence: Store **4/4**, focused remote-view/wire **5/5**, all AwNet **24/24**,
`aw-store --lib` **212/212**, composite image **7/7**, strict affected-target Clippy green for `aw-store --lib` and the `aw` net binary with `-D warnings`, formatting/diff green.

Do **not** mark remote views complete yet. Still open: shipping Workspace target/
lens and exact root/worktree binding, production-authorized two-node continuity
→ grant → reconnect → protected read → revoke acceptance, remote Agent/member
view authority, offline/stale native/browser presentation, and live UX
acceptance. Do not hand-author a continuity snapshot that production export
would not authorize merely to manufacture the two-node proof.

## V1.5 UX rules the implementation must follow

- exactly two modes: Minimal and Workspace;
- Minimal is headerless: no topbar/sidebar/tabs, one bottom status;
- Agent is a Workspace pane/tab, not a third mode;
- maximize/restore is presentation-only and reversible;
- File/Diff actions are distinct:
  - Preview inside contained Files/Git;
  - Break out to standalone tab;
  - Maximize current pane;
- one title owner; no redundant nested headers;
- standalone File/Diff identity is canonical scope/site/resource, not filename;
- use the existing Command Palette, not a second Quick Switch;
- palette prefixes remain `> ! @ # $ / &`;
- Ctrl+Shift+P/F1 opens palette; Ctrl+P remains composer/PTY-owned;
- view selection never grants Files/Git/Agent/PTy authority;
- pane focus != PTY input owner != resize owner;
- opening/closing delegated File/Review must preserve current Agent,
  composer draft/caret and PTY ownership.

## Next implementation order

1. Revalidate HEAD/status/staged/worktrees and preserve both accepted current
   slices: delegated File/Review plus the remote-view backend/protocol checkpoint.
2. Do not commit, push, tag, release or deploy without explicit user
   authorization.
3. Do not reopen delegated File/Review authority/presentation/loader work, or
   weaken the v35 remote-view trust/wire/downgrade fences, unless new evidence
   contradicts their acceptance.
4. The **local V1.5 browser visual matrix, including Build / terminal-grid,
   is accepted**. Build is a terminal-granted non-closable saved layout over
   retained real PTYs; selection/maximize starts nothing, explicit start is the
   only creation path, and a claimed tile retained its exact lease through
   maximize and a 17-second hidden-tab interval. Review/Build share one terminal
   custody component. Do not redo these screenshots/custody checks unless source
   changes.
5. Product remote views are now at the **live two-node acceptance boundary**. Host plumbing and V1.5 browser ActiveTarget semantics are implemented: Local | opaque remote Machine selector, lazy remote capability discovery, follower Files/Git rebind with navigator presentation preserved, contained previews cleared on target change, exact-source standalone pins with per-document readers, Local Review/Build PTY authority independent of target selection, and no Session/Agent execution retarget. Focused browser/controller gate is **85/85** and full-product check/strict Clippy are green. Next, use a real production-authorized pair of Machines and the real Phase-30 continuity path to run continuity→grant→select remote target→Files/search/Git inventory/open/retained Review→reconnect→re-read→revoke. During that live gate verify Agent Session/draft/caret and PTY inventory/input/resize owner remain unchanged, target loss becomes factual stale/unavailable presentation with **no Local fallback**, and a pinned standalone remote File/Review remains tied to its original source until its exact grant is revoked. Do not hand-author continuity state. Keep remote Agent/member views separate unless that slice explicitly owns them.
6. Post-ACK arbitrary partitions, remote/distinct-Actor PTY custody,
   macOS/Windows custody/broker restoration and actual Windows retained-directory
   runtime acceptance remain separate after that.
7. Keep V1.5 UX authority read-only and do not reimplement same-UID Linux
   `terminal-promote`.
8. Update durable docs and pause at the next coherent checkpoint.

## Do not do

- do not write to `.agents/plans/ux/**`;
- do not start another UX design instead of finishing the dirty slice;
- do not grant a Files root from one delegated File;
- do not grant Git inventory/write authority from one delegated Review;
- do not treat Team membership, browser auth, path, label or ticket as resource
  authority;
- do not let a delegated document move into an unrelated ordinary scope;
- do not make same-owner pairing equivalent to distinct-user delegation;
- do not retarget current Agent or execution site from a view;
- do not reimplement `terminal-promote`;
- do not claim **shipping remote views** merely because the v35 trust/protocol
  backend is green; product integration and live two-node/UI acceptance remain;
- do not claim macOS/Windows/broker-reboot restoration without those separate
  gates.
