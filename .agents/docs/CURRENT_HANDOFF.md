# CURRENT HANDOFF — AW workspace/UX implementation state

Author: coordinating implementation agent
Owner: @pvl
Prepared: 2026-11-14
Source authority host: **L5 only**
Canonical AW checkout: `/home/pvl/spaces/abird/src/aw`

> This file is the short current-state entrypoint.
>
> For the full workspace/UI/UX chronology, design lineage, implementation
> milestones, authority decisions, PTY history, delegated-browser design,
> current dirty slice, pending scope and exact source map, read:
>
> `.agents/docs/WORKSPACE-VIEWS-UX-COMPREHENSIVE-HANDOFF-2026-11-14.md`
>
> For the finalized unified Jobs + Agent runtime lineage, read:
>
> `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
>
> **Authority order:** live source/tests/executed results > direct user
> instruction > read-only UX authority > active plan TODO/spec > this handoff >
> dated historical prose.

---

## 0. Immediate live state

### AW

- Branch: `main`
- HEAD / `origin/main`:
  `c9aaf0e0d311ba905f55875147392a071f64b037`
  - `Complete delegated browser authority and surface lifecycle`
- Nothing is staged.
- Current dirty **implementation/test/generated** paths are the in-progress
  delegated File/immutable-Review shipping UI/CLI slice:
  - `src/team.rs`
  - `src/web/team.rs`
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
- Preserve those files. Do not reset or reconstruct them.
- Current live protocol/version facts:
  - AW 0.15.0
  - SQLite v34
  - JSONL v1
  - native Workspace layout v4

### UX authority — explicit user instruction

The user explicitly designated:

`.agents/plans/ux/**`

as the **source of truth for all UI and UX**.

For this active workspace/UX effort:

- **read/inspect it**;
- treat V1.5 as the current UX target;
- preserve V0–V1.4 as historical design lineage;
- **never modify, stage, reset, stash, regenerate or commit anything there**;
- keep implementation plans/evidence in
  `.agents/plans/workspace-views/**` or `.agents/docs/**`.

This explicit user instruction supersedes older handoff wording that said not to
inspect the UX subtree. It does not authorize writes.

### Finalized runtime

Do not reopen the unified Jobs + Agent runtime unless a new scoped change
requires it:

- design authority:
  `890e9a383fe2edc3e0bd0aa34a2b217f2724475c`
- AW shared/runtime:
  `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
- AW runtime handoff sync:
  `88cd9655d402ea325873470202fd12133b1d8199`
- dotlink:
  `c90a6603ccef0bbf626ce10d95bedf59877747b8`

Views project the existing Agent/Session/Actor/Job owners. A view must never
become another runtime or authority owner.

---

## 1. Read before editing

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. `.agents/docs/WORKSPACE-VIEWS-UX-COMPREHENSIVE-HANDOFF-2026-11-14.md`
5. this file
6. live HEAD/status/staged/worktrees/current diff
7. UX authority, read-only:
   - `.agents/plans/ux/README.md`
   - `.agents/plans/ux/V1.5-UNIFIED-FOCUS-AND-TABS.md`
   - `.agents/plans/ux/NAVIGATION-AND-STATE.md`
   - `.agents/plans/ux/INTERACTIONS-AND-ACCEPTANCE.md`
   - `.agents/plans/ux/VISUAL-SPEC.md`
   - `.agents/plans/ux/TERMINAL-V1.5-CELL-STUDY.md`
   - relevant v1.5 prototype/reducer/tests
8. active plan:
   - `.agents/plans/workspace-views/README.md`
   - `.agents/plans/workspace-views/TODO.md`
   - `.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
   - `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
   - `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md`
   - `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
   - `.agents/plans/workspace-views/NEXT-AGENT.md`
9. live source/tests for the owner you will touch

---

## 2. What `c9aaf0e` completed

### Distinct user/Actor Agent branch

Complete and accepted:

- owner/recipient exact delegated browser grants;
- security-epoch + generation + expiry + revoke fencing;
- held-response publication revalidation;
- recipient logout revocation;
- per-grant browser cache isolation/scrub;
- Team recipient directory/chooser;
- shipping Solid Agent send/receive;
- real two-profile Chrome 157 acceptance at 390×844;
- source draft/caret preserved;
- receiver private Agent unchanged;
- zero provider/execution side effects.

### File / immutable Review backend authority

Also complete:

- exact retained Surface read substrate;
- File = exact launch-root scope + relative path;
- Review = exact retained scope + immutable Review ID;
- no delegated read mints Files root, Git inventory/context or write authority;
- opt-in trusted `serve_with_delegated_surfaces`;
- delegated SurfaceHost is owner-held and separate from normal Team AppState;
- terminals forcibly disabled;
- Sites cleared;
- ordinary Team `/api/workspace` remains disabled;
- complete distinct-Actor backend lifecycle:
  `register -> prepare -> receive -> read -> accept -> confirm -> heartbeat ->
  release` plus status/cancel/revoke;
- exact owner/recipient/resource/generation revalidation around publication;
- cross-kind ticket replay denied.

Canonical spec/evidence:

`.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`

---

## 3. Current dirty implementation slice

The current 14 dirty implementation/test/generated paths form the accepted
**File/Review shipping UI/CLI integration**. The slice is complete and green in
focused tests plus real two-profile Chrome 157 acceptance, but remains
**uncommitted**.

### Team CLI / source composition

`src/team.rs`

Adds explicit:

`delegate_workspace_views: bool`

- default false;
- included in config fingerprint;
- opted-in participant gets trusted delegated Surface source;
- terminals off;
- Sites empty;
- no ordinary recipient Workspace root.

### Team delegated source routes

`src/web/team.rs`, `src/web/workspace.rs`

- narrow source capability/read path;
- retained owner SurfaceHost;
- strict auth/identity revalidation;
- bounded body/time;
- ordinary `/api/workspace` remains disabled.

### Browser delegated document owner

`web/src/workspace-views.js`

- exact per-grant `reader` + `authorityKey`;
- delegated File/Review standalone document;
- ordinary source replacement does not scrub independent delegated docs;
- delegated 403 scrubs only the exact delegated grant/cache/state;
- full browser auth expiry clears all;
- delegated document cannot move into unrelated ordinary scope.

### File/Review send/receive

`web/src/document-docking.js`

- correlation fragment: `#aw-surface-dock=`;
- exact owner loaded File/Review only;
- exact recipient;
- receive descriptor + grant generation;
- recipient read before Accept;
- Confirm/heartbeat/release;
- no protected bytes in ticket.

### Shipping Solid

`web/src/main.jsx`, `web/src/styles.css`

- ordinary vs Team delegated source mode;
- recipient selector;
- private read-only delegated document link;
- delegated read-only badge;
- delegated receive flow;
- no re-delegation/move into unrelated scope;
- auth-expiry cleanup.

The accepted slice retains the prior Team 48/1, Team config 12/12,
browser/controller 187/187, strict library Clippy and default-binary evidence.
Bounded File/Review presentation transfer is green; post-fix focused
docking/workspace browser tests are **78/78** and two production builds are
deterministic **8/8**. Real Chrome 157 fresh-profile File and Review runs at
390×844 prove exact distinct-user authority, presentation restore,
revoke/logout/reload scrub, unchanged Agents/draft/caret, no overflow or page
JS errors, unchanged synthetic Git, provider count 0 and zero
run/model/tool/effect rows. The Chrome gate also found and fixed a cold
CodeMirror lifecycle bug by replacing the Solid `lazy()` editor boundary with
a cached explicit dynamic import awaited before document admission while
retaining the separate editor chunk.

---

## 4. Remote-device trust backend checkpoint — accepted, product integration open

The next separate workspace-view boundary is now partially implemented in the
current **uncommitted** tree. See
[REMOTE-DEVICE-TRUST.md](../plans/workspace-views/REMOTE-DEVICE-TRUST.md).

Implemented:

- SQLite schema **v35** durable remote-view grants;
- exact portable device/stable-user identity and active Domain membership;
- Domain policy + security epoch fencing;
- trusted peer plus explicitly registered requester remote Machine;
- requester/target Machine security + capability revisions;
- exact hosted Workspace revision + policy revision;
- bounded Files/Git permissions, expiry, explicit revoke and reconnect
  revalidation;
- additive identity-auth **v3 only for remote-view grant issue**, preserving
  legacy v2 wire/transcript when absent;
- additive Machine-read **v2 only for protected `View*` reads**, preserving
  legacy v1 wire when absent;
- fresh device signature per protected read bound to lease/request/grant/
  Workspace/request digest;
- second grant authorization after read and immediately before response
  publication;
- downgrade fence: remote-view service rejects legacy reads; ordinary service
  rejects `View*`.

Evidence on the current tree: Store remote-view **4/4**, focused remote-view/
wire **5/5**, full AwNet **24/24**, `aw-store --lib` **212/212**,
`composite_design` **7/7**, strict affected-target Clippy green for `aw-store --lib` and the `aw` net binary with `-D warnings`, formatting and diff check green.

This does **not** enable shipping remote Workspace views. Still open are the
V1.5 target/lens + exact registered root/worktree product integration, a real
production-authorized two-node continuity→grant→reconnect→protected read→revoke
acceptance run, remote Agent/member views, and offline/stale remote presentation.
Do not manufacture that proof by hand-authoring an identity-continuity snapshot
that production source export would not authorize.

Separately, the local browser V1.5 visual matrix is accepted, including the
canonical **Build / terminal-grid** saved layout. Review, contained Files/Git,
standalone File/Diff + immersive zoom, Review Agent/Terminal maximize, Minimal,
palette, light theme and 390×844 responsive geometry are live-checked. Build is
now a terminal-granted non-closable tab over the existing retained PTY inventory:
opening it starts nothing; explicit start creates PTYs; 1/2/3–4 terminals render
as bounded saved layouts; per-tile maximize never changes inventory or custody.
A real claimed PTY remained the sole input owner across maximize/restore and a
17-second hidden Review interval (past the 15-second lease TTL), with all four
terminal IDs unchanged. Narrow Build is full-width 390px with zero page overflow
and a scrollable four-terminal stack. Review and Build share the same terminal
custody/input component, and terminal selection is disabled while Review owns
input until explicit Release. The responsive pass also fixed the earlier
implicit-grid bug (266px main pane → full 390px, zero page overflow).

---

## 5. V1.5 UX rules that apply to these slices

- exactly **two modes**: Minimal and Workspace;
- Minimal has no top header/sidebar/tab strip; one bottom status;
- Agent is a Workspace pane/tab, not a third mode;
- maximize/restore is presentation-only and reversible;
- File/Diff have three distinct actions:
  - contained Preview;
  - Break out to standalone tab;
  - Maximize current pane;
- standalone identity is canonical site/scope/resource, not filename;
- one title owner; avoid redundant nested headers;
- use the existing command palette, not a second global Quick Switch;
- prefixes remain `> ! @ # $ / &`;
- Ctrl+Shift+P/F1 opens palette; Ctrl+P stays composer/PTY-owned;
- pane focus never implies PTY input/resize ownership;
- opening/closing a delegated File/Review must preserve current Agent,
  composer draft/caret and PTY ownership;
- view/ticket/path/label/browser auth never creates resource authority.

---

## 6. Next implementation order

1. Revalidate and preserve both accepted current slices: the delegated
   File/Review product diff and the remote-view backend/protocol checkpoint.
2. Do not commit/push/tag/release/deploy unless explicitly authorized.
3. Next, integrate product remote views through the existing V1.5 Workspace
   target/lens and exact registered root/worktree bindings; consume the v35
   grant and protected AwNet `View*` path rather than inventing another trust
   system.
4. Run a real production-authorized two-node continuity→grant→reconnect→read→
   revoke acceptance when that product integration supplies the actual Domain/
   Machine context. Keep remote Agent/member views separate unless explicitly
   owned by that slice.
5. Post-ACK arbitrary partitions, remote/distinct-Actor PTY custody,
   non-Unix custody/broker restoration and actual Windows retained-directory
   runtime acceptance remain separate.
6. Keep `.agents/plans/ux/**` read-only and do not reopen same-UID Linux
   `terminal-promote`.
7. Update durable docs and pause at the next coherent checkpoint.

---

## 7. Separate future scope

After delegated File/Review shipping acceptance:

- remote Workspace product integration/live acceptance on top of the accepted
  v35 trust/protected-read backend;
- post-ACK arbitrary process/network partitions;
- remote/distinct-Actor PTY custody;
- macOS/Windows PTY custody;
- broker crash/reboot execution restoration;
- actual Windows retained-directory runtime acceptance;
- separately executed browser visual/screenshot acceptance.

Same-UID Linux `terminal-promote` is complete. Do not reimplement it.

---

## 8. Safety / Git

- Preserve all current dirty implementation/test/generated paths.
- Never write to `.agents/plans/ux/**`.
- Before commit, inspect exact owned paths/staged stat.
- Commits are signed.
- Commit does not imply push permission.
- Push/tag/release/deploy require explicit authorization.
- Keep unrelated work untouched.

---

## 9. Starting prompt

Use:

`.agents/plans/workspace-views/NEXT-AGENT.md`

It is maintained as the pasteable next-agent prompt and explicitly carries the
user's read-UX / never-write-UX instruction.
