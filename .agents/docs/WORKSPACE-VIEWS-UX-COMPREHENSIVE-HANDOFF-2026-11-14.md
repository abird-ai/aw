# Workspace Views + UX comprehensive handoff — 2026-11-14

Author: coordinating implementation agent
Owner: @pvl
Source authority host: **L5 only**
Canonical repo: `/home/pvl/spaces/abird/src/aw`
Current branch: `main`
Prepared from live HEAD/source/tests/plans plus the read-only UX design authority.

> This is the durable entry point for the active `workspace-views` / UI+UX program.
> It is intentionally comprehensive enough that a new agent should not need this
> chat history before continuing.
>
> **Authority order for this effort:** live source + executed tests > direct user
> instructions > `.agents/plans/ux/**` for presentation/interaction UX >
> `AGENTS.md` repository safety > active plan TODO/specs > this handoff > dated
> historical prose.
>
> The user explicitly authorized reading `.agents/plans/ux/**` as the source of
> truth for UI/UX. That authorization is **read-only**: never write, stage, reset,
> stash, regenerate or commit anything in that subtree. Implementation plans,
> acceptance notes and handoffs belong outside it.

---

## 0. Immediate live state

### Git / worktree

- HEAD: `c9aaf0e0d311ba905f55875147392a071f64b037`
  - `Complete delegated browser authority and surface lifecycle`
- `origin/main` is at the same commit.
- One canonical AW worktree only.
- Nothing is staged.
- Current dirty implementation/test/generated state is an **in-progress delegated
  File/Review shipping UI/CLI slice** and must be preserved:
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
- Latest observed implementation/test/generated diff is **1,234 insertions /
  85 deletions across those 14 paths**. The new tests and generated assets are
  part of the same in-progress slice. Bounded presentation transfer is now
  implemented and has focused executed evidence; real two-profile File/Review
  Chrome acceptance remains open.
- Do not reset/reconstruct them. Re-read their live diff before continuing.

### Queue

- P1 `release-readiness`: remaining work is genuinely external/native-host or
  owner-decision work.
- P2 `workspace-views`: active implementation program and current practical
  priority.
- P2 `dotlink-tool-port`: separate lineage; do not confuse it with this view/UX
  work.

### Finalized runtime lineage

The unified Jobs + Agent runtime is finished and should not be reopened as part
of workspace UI work:

- design authority: `890e9a383fe2edc3e0bd0aa34a2b217f2724475c`
- shared/runtime implementation:
  `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
- final AW handoff/pin sync:
  `88cd9655d402ea325873470202fd12133b1d8199`
- final dotlink bridge:
  `c90a6603ccef0bbf626ce10d95bedf59877747b8`

Permanent consequence: views project existing Agent/Session/Actor/Job owners and
exact capabilities; views never become a second runtime or authority owner.

---

## 1. Non-negotiable operating rules

- Use **dotlink---l5** only for implementation/source work.
- Do not use A1.
- x2 may be used only as an inference host; it is not source authority.
- Keep each implementation turn near ~30 minutes and checkpoint at the next
  coherent boundary.
- Preserve unrelated dirty/staged work.
- Do not create another worktree.
- No destructive Git operation.
- Commit/push/tag/release/deploy require explicit authorization for that action.
- Read `.agents/plans/ux/**` freely for this workspace/UX task because the user
  explicitly designated it as the UI/UX source of truth.
- **Never modify anything under `.agents/plans/ux/**`.**
- Keep implementation plans/specs, acceptance evidence and handoffs in
  `.agents/plans/workspace-views/**` or `.agents/docs/**`.
- Live source/tests outrank all prose about whether something is implemented.
- Historical unchecked milestone bullets are not current TODOs until reconciled
  against later commits and the live TODO.

---

## 2. Exact reading order for a new agent

Read before editing.

### Repository / current state

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. this file
5. `.agents/docs/CURRENT_HANDOFF.md`
6. live `git status --short`, HEAD, staged files, worktrees and current diff

### UX authority — read-only

7. `.agents/plans/ux/README.md`
8. `.agents/plans/ux/V1.5-UNIFIED-FOCUS-AND-TABS.md`
9. `.agents/plans/ux/NAVIGATION-AND-STATE.md`
10. `.agents/plans/ux/INTERACTIONS-AND-ACCEPTANCE.md`
11. `.agents/plans/ux/VISUAL-SPEC.md`
12. `.agents/plans/ux/REFERENCES-AND-VISUAL-INVENTORY.md`
13. `.agents/plans/ux/TERMINAL-V1.5-CELL-STUDY.md`
14. when implementing interaction behavior, also inspect read-only:
    - `ux-v1.5.js`
    - `ux-views.js`
    - `ux-palette-v1.5.js`
    - `test-ux-v1.5.js`
    - `test-ux-interactions.js`
15. read V0→V1.4 design docs when changing a behavior inherited by V1.5; do not
    silently discard earlier advantages.

### Active implementation plan

16. `.agents/plans/workspace-views/README.md`
17. `.agents/plans/workspace-views/TODO.md`
18. `.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
19. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
20. `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md`
21. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
22. `.agents/plans/workspace-views/NEXT-AGENT.md`
23. live source/tests for the exact owner being changed

### Older/finalized context when needed

24. `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
25. `.agents/docs/HANDOFF.md`
26. Git history / selected plan committed groups
27. release-readiness TODO only when changing release gates

---

## 3. Product / identity model that UI must never blur

The UX archive and implementation plans use specific nouns deliberately:

- **Machine** — physical/virtual execution host.
- **Space** — logical collection of machine memberships, projects and UI state.
  A per-machine Local Space may represent Home; Home is not automatically a Git
  repository.
- **Project** — logical repository/development identity.
- **Worktree** — concrete checkout on one host/site.
- **Session** — durable conversation/history.
- **Agent / Actor** — executing identity with real parent/child lineage,
  Session binding, grants and execution site.
- **PTY / Terminal** — process terminal with independent viewer/control/resize
  ownership.
- **Workspace tab / pane / document** — presentation identity, not authority.

Never infer authority from:

- path text;
- file name;
- display name;
- tab/pane/container ID;
- browser cookie alone;
- pairing code alone;
- docking ticket alone;
- location in the visual hierarchy;
- recent ACK/heartbeat;
- matching bytes after a revoke.

Presentation state may point at an exact capability; it never mints one.

---

## 4. UX design chronology — V0 to V1.5

The UX subtree preserves the full design lineage. V1.5 is current, but earlier
versions explain why the current shape exists.

### V0 — Pi-like minimal Agent + three-mode mental model

Preserve:

- simple native Agent transcript;
- understated tool strips/status;
- compact composer/footer;
- nested Agent hierarchy;
- first-class Git/files/terminal Workspace;
- explicit machine/project/worktree context;
- keyboard-first operation;
- status color always redundant with text/shape.

Historical V0 had Minimal / Agents / Workspace modes and a permanent inspector in
some layouts.

### V1 — tiling

Added:

- composable native Agent, real PTY, Git, File and other pane contents;
- saved layout tabs;
- split/resize/focus/zoom;
- Agent and PTY ownership remain separate from pane identity;
- closing/zooming a pane never terminates an Agent/process;
- PTY ordinary keys belong to the terminal app unless a deliberate multiplexer
  leader captures them.

### V1.1 — Recents + Activity

Added:

- intentional Recents;
- quiet background/delegated Agent activity;
- Now / attention / Automations distinctions;
- completion/polling does not reorder Recents;
- Agent state, unread state, machine connectivity and Git dirtiness stay distinct.

### V1.2 — Space / machine header

Moved Space and machine navigation to the global header and distinguished Local
Home from named/fleet Spaces.

### V1.3 — integrated navigation

Integrated Projects/Recents upper navigation with Activity lower navigation while
retaining the full tiled Workspace.

### V1.4 — preserved canonical baseline

Established:

- upper Projects/Recents nav;
- lower independent Activity/Git/Files dock;
- full Workspace review layouts;
- compact/minimal variants;
- terminal-cell/ANSI studies and fixed-size visual evidence.

### V1.5 — current UX target

**Only two presentation modes: Minimal and Workspace.**

There is no third Agents mode anymore.

#### Minimal

- genuinely headerless;
- no global top bar;
- no sidebar;
- no tab strip;
- native Agent transcript + composer;
- exactly one compact bottom status line;
- same Agent/Session/context as Workspace;
- mode switch changes presentation only.

#### Workspace

- global Space/Machine context;
- Projects/Recents upper navigator;
- independent Activity/Git/Files lower dock;
- saved layout/document tabs;
- native Agent is a normal pane/tab;
- maximizing the Agent is the old Agents-mode presentation, not a new mode.

Mode changes never:

- start/stop an Agent;
- move execution site;
- change credentials/grants;
- destroy PTYs;
- discard scroll/draft state.

---

## 5. V1.5 canonical interaction contract

### 5.1 Pane maximize / restore

Every major pane may maximize/restore:

- Agent;
- File explorer/editor;
- Git changes/diff;
- each real PTY;
- standalone File/Diff.

Maximize:

- is tab-local;
- creates no new tab;
- changes no selected resource;
- changes no authority;
- preserves split ratios, viewport, scroll, selection and focus for restore.

Double-click may toggle only on a **pane header**; it must not steal editor/PTY
content clicks.

### 5.2 File/Diff: three different actions

**Preview**
- contained Files/Git selection changes only that container's viewer;
- no new tab;
- no write/stage/effect.

**Break out**
- creates/focuses a standalone File/Diff tab;
- original Files/Git container remains;
- exact scoped identity is preserved.

**Maximize**
- shows that existing pane alone;
- no new tab;
- no retarget.

Standalone tab identity includes canonical Machine/Site + Worktree/Scope +
File/Review identity, not filename alone. Diff identity additionally includes its
exact scope/revision semantics.

Reopening the same canonical resource deduplicates and focuses the existing tab.

### 5.3 One title owner

V1.5 polish deliberately removes redundant nested title bars:

- Workspace global header answers where;
- tab answers which document/layout;
- split pane header exists only to distinguish siblings;
- standalone File/Diff should not repeat the same filename in another full header;
- immersive zoom keeps only a subtle identity + Restore.

### 5.4 One existing command palette

Do **not** build another Quick Switch.

Current aw palette invocation:

- Ctrl+Shift+P
- F1

Ordinary Ctrl+P stays composer/PTY-owned except already-supported navigation-only
fallback.

Canonical categories/prefixes:

- `>` Actions
- `!` Windows
- `@` Files
- `#` Sessions
- `$` Terminals
- `/` Commands
- `&` Agents — V1.5 addition

Choosing a palette result routes to the same typed presentation action; palette
text never becomes authority.

### 5.5 Agent semantics

- selecting an Agent focuses/opens its existing Agent viewer;
- a delegated child is a real Actor/Session relation, not a third visual mode;
- background is metadata, not another Agent identity;
- selecting a child never retargets another Agent's execution authority;
- current composer/draft/caret remain bound to the current Agent unless a real
  activation flow changes them.

### 5.6 PTY semantics

- pane focus, viewer status, input owner and resize owner are distinct;
- maximize/tab switch cannot steal PTY control;
- stale/disconnected terminal presentation is read-only until valid reattach;
- Escape belongs to the foreground terminal application when the PTY owns input.

---

## 6. Original workspace-views architecture

The accepted design checkpoint is
`47ee8a03abecbb3aee84a49c0a590096d76ed1a7`.

Core thesis:

> Multiple views/resources inside one Workspace project existing owners and
> exact retained capabilities. A view is never a second Runtime and never
> creates authority from labels, paths, layout state or browser auth.

Permanent design decisions:

- one current Agent and composer;
- read-only Agent views pin exact Store/Session/branch;
- browsing never activates an Agent;
- unsaved Agent is not persisted just to inspect it;
- strict read path does not mint SessionHandle;
- uncertain history may be shown but never acknowledged/replayed by viewing;
- refresh revalidates authority even when content/revision appears unchanged;
- revoke/deny clears protected content and derived state;
- async replies carry exact view/resource/lifetime generation and are discarded
  if closed/superseded;
- flat stable-ID container graph;
- bounded panes/tabs;
- layout migration preserves geometry/IDs/focus or fails atomically;
- publication helper is reused only where measurement proves benefit; it never
  replaces authority/expiry/sequence checks.

---

## 7. Implementation chronology

The plan TODO is the exhaustive checklist. This section is the durable high-level
chronology.

### Native Agent views

- `47ee8a0` — accepted design.
- `1c2ec6e` — native read-only Agent views.
- `2809fa5` — async Find completion.
- `3b0dbb5` — Find intent/request lifetime fencing.

### Native scoped Files

- `6b96f01` — retained Windows directory bindings.
- `3851dbf` — scoped Files containers, explicit retained capabilities,
  reconnect, v3 layout migration, stale-result fencing.

### Native scoped Git

- `491f366` — retained repository identities.
- `b33a418` — scoped Git containers, exact context/review identity, selected
  index admission, v4 native layout.

### Browser read-only Agent

- `a0ffa3e` — browser read-only Agent tabs.
- `68629d8` — explicit browser draft targets.
- `94697a7` — reachable draft consent + real Chrome acceptance.

### Browser Files/Git

- `57a6059` — scoped browser Files/Git.
- `0159f0b` — bounded merged editors + final Chrome acceptance.

### Publication/presence measurement

- `2c07425` — measured actual HTTP publication and shared-draft presence;
  retained existing ownership where measurement did not justify redesign.

### Browser docking / same-owner continuity

- `e06ffa7` — same-window File/Diff reparent.
- `7de8658` — File cross-window handoff.
- `00559eb` — immutable Review handoff.
- `41e66c4` — Find/scroll/selection/focus continuity.
- `e558919` — receiver-confirmed lifecycle.
- `0efea351` — explicit same-owner independent Chrome-profile pairing.

Permanent lesson: same-owner pairing/docking is **not** cross-user delegation.

### Native terminal / PTY work

- `c98987a` — same-owner second-terminal docking.
- `bd87e5d` — separately launched broker-owned persistent PTY.
- `635e1ec` — reversible original-reader quiescence.
- `a1100a9` — PTY master + pidfd escrow.
- `2feaa78` — Linux migration prerequisites.
- `207c552` — bounded parser checkpoint + staged escrow.
- `62f3735` — exact pidfd exit facts.
- `7ced795` — guarded source release/passive parent custody.
- `2b8d769` — reproducible native fixtures.
- `9f1cc28` — inert adopted reactor.
- `11999aa` — reader retirement under held-slave cases.
- `9d82f1a` — **same-UID Linux original-shell custody + shipping
  `terminal-promote` complete**.
- `65f04ca` — actual different-UID FD admission refusal.

Do not reopen historical “migration unimplemented” prerequisite bullets as
current Linux TODOs.

### Unified Jobs/Agent runtime

Finalized independently:

- AW `0a6c6a0`
- handoff sync `88cd965`
- dotlink `c90a660`

Relevant authority principle: source/effect/site/generation and replay-first
semantics are reusable for view delegation; do not build a second loose browser
permission engine.

---

## 8. Distinct user/Actor delegated browser authority

### 8.1 Authority design

The canonical spec is
`.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`.

A grant is memory-only, owner-side, bounded and exact. It binds:

- owner principal + stable user/security epoch;
- recipient principal + stable user/security epoch;
- exact source/site;
- one narrow immutable resource:
  - Agent branch: Store/Session/branch;
  - File: source/scope/relative path;
  - Review: source/scope/immutable Review ID;
- monotonic grant generation;
- issue/deadline;
- active/revoked state;
- required parent authority evidence.

Every receiver read revalidates before and after protected work and again before
publication where needed. Revoke is monotonic. Regrant creates a new generation.

Never granted by this capability:

- Agent activation/retarget/prompt/Job control;
- Files root beyond exact File;
- Git inventory/context/write authority beyond exact immutable Review;
- provider/account/payer/tool authority;
- PTY custody;
- remote-device trust.

### 8.2 Agent branch — complete

Committed and accepted in `c9aaf0e` lineage:

- distinct Team participant identity is the recipient auth boundary;
- exact Agent branch grant/ticket lifecycle;
- owner-side strict branch read;
- recipient gets no Session participation or Store authority;
- held-response and publication revalidation;
- generation/revoke/logout/expiry fencing;
- per-grant cache isolation/scrub;
- shipping Solid recipient chooser + delegated receive;
- real two-profile Chrome 157 acceptance at 390×844;
- Alice/Bob distinct principals/cookies;
- draft/caret preserved;
- recipient active private Agent unchanged;
- zero provider calls / zero execution effect rows.

### 8.3 File / immutable Review authority substrate — complete

Committed before current dirty UI slice:

- shared exact retained Surface read primitive;
- File is launch-root-only exact scope/path;
- Review is exact retained scope + immutable Review ID;
- no Git inventory selection/grant creation in the read primitive;
- same-owner docking reuses this canonical read;
- grant registry uses exact File/Review identity + Surface owner epoch.

### 8.4 File / Review distinct-Actor HTTP lifecycle — complete

Committed in `c9aaf0e`:

- opt-in trusted `team::serve_with_delegated_surfaces`;
- each delegated source is a separate owner-held `Arc<SurfaceHost>`;
- normal participant `AppState.surfaces` remains absent;
- normal `/api/workspace` remains disabled;
- terminals and Sites are forcibly disabled on delegated surface hosts;
- one shared lifecycle:
  `register -> prepare -> receive -> read -> accept -> confirm -> heartbeat -> release`
  plus status/cancel/revoke;
- prepare validates a real exact owner-side retained read before grant issuance;
- receive discloses only descriptor + generation, not bytes;
- read is owner-mediated and revalidated around publication;
- cross-kind ticket replay denied;
- recipient logout / owner expiry / explicit revoke fence publication.

Tests proved exact File/Review reads, forged resource denial, revoke-held response
fencing and zero provider/model effects.

---

## 9. Current uncommitted implementation — preserve exactly

HEAD `c9aaf0e` already completed the backend lifecycle. The current dirty
14-path implementation/test/generated slice contains the Team CLI + shipping
Solid File/Review integration plus the bounded distinct-Actor presentation
transfer implemented on top of that route.

It is **in progress, uncommitted and not yet accepted**.

### 9.1 `src/team.rs`

Adds explicit Team config:

`delegate_workspace_views: bool`

Properties:

- default false for v1 compatibility;
- included in config fingerprint;
- visible in configuration validation output;
- only explicitly opted-in participants get a trusted delegated Surface source;
- source uses Team trusted directory;
- terminals disabled;
- Sites empty;
- Team chooses `serve_with_delegated_surfaces` only when sources exist.

This is the missing CLI/config composition step named in the committed spec.

### 9.2 `src/web/team.rs` + `src/web/workspace.rs`

Adds narrow source-side endpoints/capabilities for opted-in Team owner identity:

- delegated-surface capability metadata;
- source-side delegated workspace capability read;
- source-side delegated read action through the retained owner SurfaceHost;
- identity revalidation before response publication;
- strict body/time bounds.

These do not enable ordinary recipient `/api/workspace`.

### 9.3 `web/src/workspace-views.js`

Adds delegated File/Review document ownership:

- `openDelegatedDocument(kind, target, reader, authorityKey)`;
- delegated documents have an exact per-grant reader + authority key;
- ordinary Workspace source replacement clears only ordinary source-owned views;
- delegated grant lifetime is independent;
- a delegated 403 scrubs only that delegated cache/content/state;
- full browser auth expiry still clears everything;
- delegated documents cannot be moved into unrelated Workspace scopes;
- exact File path or Review ID remains immutable.

This is a critical security boundary: do not “simplify” delegated documents into
ordinary container scope state.

### 9.4 `web/src/document-docking.js`

Adds a separate correlation fragment:

`#aw-surface-dock=<ticket>`

Implements File/Review send/receive over the existing backend lifecycle:

- source validates loaded exact owner File/Review;
- prepare sends exact target + recipient;
- receiver validates resource descriptor + grant generation;
- receiver constructs exact per-grant reader;
- receiver performs current delegated read before Accept;
- Confirm/heartbeat/release follow the existing authority/liveness semantics;
- known 4xx failures close/scrub appropriately;
- ticket carries no protected document bytes.

### 9.5 `web/src/main.jsx`

Integrates shipping Solid:

- distinguish ordinary Workspace source from Team delegated source;
- source owner may select Team recipient and create a private read-only link;
- received delegated File/Review opens as a standalone delegated document;
- delegated documents are visibly read-only/delegated;
- delegated documents do not expose re-delegation/move controls;
- auth expiry clears protected delegated state;
- Team source owner can use delegated read source without enabling ordinary
  recipient Workspace root.

### 9.6 `web/src/styles.css`

Only small controls/styles for recipient selector/share UI.

### 9.7 CLI + Solid delegation checkpoint now recorded

The concurrent implementation owner has now recorded a green CLI/controller
checkpoint in `DELEGATED-BROWSER-AUTHORITY.md`. Treat that plan evidence as
current unless live tests/source contradict it:

- authenticated Team module: **48 passed / 1 intentional ignored**;
- Team CLI/config: **12/12 passed**, including v1 default-false and fingerprint
  rotation;
- browser model/controller suites: **187/187 passed**;
- production Vite build: 174 modules;
- two consecutive production builds: **8/8 byte-identical embedded assets**;
- strict aw-cli library Clippy: passed;
- default CLI binary check: passed;
- reduced Team library check: passed with the known pre-existing
  `bind_execution_owner` warning;
- reduced Team binary is explicitly **not** a gate for this slice because it
  reaches an unrelated existing `src/subagents.rs` feature-boundary error
  before the Team source path.

The dirty tests now specifically cover:

- owner-only delegated Surface source without ordinary Team Workspace authority;
- exact File send identity with no protected bytes in the ticket;
- receiver operation with no local Workspace source/root;
- correlation fragment stripping;
- known revoke cleanup;
- delegated document surviving ordinary source replacement;
- same-resource grant A/B cache isolation;
- 403 exact-grant scrub;
- 401 global ordinary+delegated cache scrub.

### 9.8 Bounded distinct-Actor presentation transfer — implemented, focused green

The distinct-Actor route now reuses the existing bounded v1 presentation
contract instead of inventing a second state format.

- File carries bounded Find intent/result, caret/selection and viewport/scroll.
- Review carries bounded two-pane Find, selection, scroll and focused pane.
- Presentation is ticket metadata only. It does **not** enter the delegated
  resource identity, binding, recipient identity, generation or authority key.
- File presentation fingerprints are checked against the exact owner bytes at
  prepare, again before recipient Accept, and again before owner Release.
- Immutable Review presentation remains bound to the exact retained Review ID
  plus the existing content fingerprint used by the browser restore path.
- The receiver independently reads the delegated resource, waits for the exact
  editor to render, verifies the presentation fingerprint, restores bounded
  editor state, rechecks target/render identity, and only then Accepts.
- Recipient chooser focus is handled as presentation only: pointer-time staging
  preserves the last in-document focus while the actual send gesture recaptures
  current Find/selection/scroll state. It grants no authority.

Executed evidence for this checkpoint:

- `node --test tests/web_docking_handoff.test.mjs` — **16/16 passed**,
  including File restore, two-pane Review restore, fingerprint mismatch fencing,
  exact revoke scrub, fragment stripping, and lost Release-response truthfulness;
- focused delegated File lifecycle — passed;
- focused immutable Review lifecycle with pinned `AW_GIT` — passed;
- `delegated_file_revoke_fences_held_response_and_future_reads` — passed;
- reduced Team+Surfaces library check — passed with only the existing
  `bind_execution_owner` warning;
- `cargo fmt --all -- --check` and `git diff --check` — passed before the
  durable-doc updates;
- two consecutive production Vite builds transformed 174 modules and were
  byte-identical for **8/8** emitted assets.

Tracked production assets are rebuilt in the dirty tree. Latest observed SHA-256:

- `web/dist/app.js`:
  `2ff174edceb39f9b0230fec735facae56f62a2132c13b64a43f686f03a6c89c5`
- `web/dist/style.css`:
  `efedfa9f5aab7b8b5358d97569cf59379570553362223e6ce17453e0947df114`

The distinct-Actor File/Review browser slice is now **accepted** in the current
uncommitted product diff.

Real Chrome 157 acceptance used separate fresh Alice/Bob profiles at 390×844
and completed File and immutable Review transfers. It proved distinct Team
principals and cookie values, exact recipient/resource identity, correlation
fragment removal before every protected request, no recipient Workspace/root/Git
authority, restored bounded presentation state, factual source Release,
File explicit Revoke scrub, File reload/no-replay, Review logout scrub, unchanged
source/receiver private Agents and source draft/caret, zero page-wide overflow,
zero uncaught JS exceptions / `console.error`, unchanged synthetic Git
HEAD/index, fake provider count 0, and zero execution/model/tool rows in private
SQLite.

The live gate found and fixed one real shipping frontend defect: a cold,
code-split CodeFile/CodeDiff could download the complete editor chunk yet strand
CodeMirror mount under the Solid `lazy()` ownership boundary. The final product
continues to keep the ~1.50 MB editor chunk out of the initial app bundle, but
uses one cached explicit dynamic import that is awaited before ordinary
File/Review admission and both docking receiver paths. The real editor component
is therefore mounted from an already-resolved module.

At 390×844 this synthetic fixture has no internal editor scroll range, so source
and receiver viewport offsets truthfully clamp to zero and match exactly.
Focused controller regressions retain nonzero viewport-offset coverage.

Post-fix focused browser/controller evidence is **78/78**; two production Vite
builds remain byte-identical across **8/8** assets.

Remote-device trust remains a separate later gate.

---

## 10. UX-specific implementation constraints for the dirty File/Review slice

The backend grant is already narrow. The shipping UI must also follow V1.5.

### File/Review presentation

- delegated File/Review should appear as exact read-only documents;
- one title owner; avoid duplicate headers;
- standalone File and Diff remain distinct presentations;
- no implicit contained Files/Git navigator is created for the recipient;
- receiving one File does not create a Files root;
- receiving one Review does not create Git inventory/context/write authority;
- delegated document cannot be moved into ordinary unrelated container scope;
- maximize is presentation-only and must preserve the delegated grant identity;
- close scrubs only that document/grant lifetime;
- revoke/403 scrubs protected bytes, Find/selection/viewport derived state.

### Command palette

If delegated documents become palette-addressable later:

- use the existing palette;
- do not create another global switcher;
- preserve `> ! @ # $ / &`;
- result selection routes to existing typed view action only;
- palette text/display label is not authority.

### Focus/draft

Opening/closing/delegating File/Review must not:

- replace current Agent;
- activate another Session;
- clear or submit composer draft;
- move caret;
- steal PTY input/resize;
- create model/tool calls;
- change Files/Git authority outside the exact grant.

---

## 11. Linux PTY truth — do not reopen

Same-UID Linux original-shell migration is complete.

Shipping concepts:

### `terminal-dock`

Moves presentation/control of the same PTY while original Workspace retains
ownership.

### persistent start

Launches a new broker-owned PTY that survives the launching frontend.

### `terminal-promote`

Migrates an **already-running Workspace shell** to the broker on same-UID Linux.

Current accepted semantics:

- exact shell PID preserved;
- bounded parser state preserved;
- one reader / one writer / one process owner;
- no input replay;
- activation only after exact signed release;
- source never resumes after committed release;
- source death alone does not authorize activation;
- exact pidfd signaling;
- distinct-UID FD admission is denied and tested.

Still separate/unimplemented:

- remote/distinct-Actor PTY custody;
- macOS/Windows custody acceptance;
- broker crash/reboot execution restoration;
- arbitrary terminal-app compatibility beyond current fixtures.

---

## 12. Permanent Files/Git authority decisions

### Files

- ScopeId is not a path grant.
- Access requires an explicitly retained WorkspaceFiles capability.
- launch root survives Agent presentation changes independently;
- external folders require explicit reconnect after restart;
- last-reference closure revokes unused external capability;
- every async result carries scope/path/binding/lifetime;
- reconnect scrubs old plaintext/derived state before new content;
- Windows uses retained directory identities; actual Windows runtime remains a
  separate gate.

### Git

- context ID names exact authorized checkout;
- worktree selection mints a new context, never retargets by path;
- Diff retains immutable Review + original context;
- index mutation rechecks exact review/HEAD/binding at owner admission;
- reconnect/restore/final-reference closure revokes queued admission;
- already admitted external mutation is not silently replayed/cancelled;
- Git-derived terminal launch rechecks exact captured binding.

---

## 13. Validation evidence to carry forward

Do not rerun unchanged primitives by habit.

### Current delegated browser authority checkpoint `c9aaf0e`

Recorded committed evidence includes:

- owner delegation registry/races;
- distinct Agent branch HTTP/docking;
- Agent branch shipping Solid;
- real two-profile Chrome 157 at 390×844;
- File/Review Surface substrate;
- distinct-Actor File/Review HTTP lifecycle;
- docking regressions;
- reduced Team/Web boundaries;
- strict Clippy / formatting;
- Node and production frontend build at the Agent-branch checkpoint.

See `DELEGATED-BROWSER-AUTHORITY.md` for exact commands/counts.

### Current dirty File/Review UI slice

CLI/source composition, delegated document lifetime, send/receive controls,
bounded File/Review presentation transfer, cold editor-module admission,
targeted Rust/Node regressions, deterministic production assets and real
two-profile Chrome File + Review acceptance are green as scoped above. This
accepted slice remains **uncommitted**.

### Native/current workspace evidence

Fresh validation before delegated-browser work proved:

- narrow-frame layout;
- maximize/restore preserving draft/scroll/layout;
- focus/composer preservation;
- palette cancel/resize focus preservation;
- browser docking owner suite;
- real `terminal-promote` parser/vim/less/source-crash.

Carry these unless their owner changes.

---

## 14. Genuine remaining product scope after the dirty File/Review slice

Delegated File/Review shipping acceptance is complete. Remaining
workspace-views work is separate:

### Remote-device trust / remote views — backend matrix implemented, product open

The current **uncommitted** tree now implements the reusable backend authority
matrix in [REMOTE-DEVICE-TRUST.md](../plans/workspace-views/REMOTE-DEVICE-TRUST.md):

- SQLite schema **v35** durable remote-view grant;
- exact portable device + stable user + active Domain membership;
- Domain policy/security epoch;
- exact trusted peer and explicitly registered requester remote Machine;
- requester and target Machine security/capability revisions;
- exact hosted Workspace revision/policy revision;
- bounded Files/Git permissions, expiry and persisted revoke;
- reconnect only while every pinned authority fact remains current;
- additive Phase-30 identity-auth v3 only for remote-view issuance, while
  legacy identity auth remains v2-compatible;
- additive protected Machine-read v2 only for `View*`, while ordinary Machine
  reads remain v1-compatible;
- fresh device signature on every protected read, bound to lease/request/grant/
  Workspace/request digest;
- current grant reauthorization again immediately before response publication;
- server-mode downgrade fence: remote-view service rejects legacy reads and
  ordinary service rejects `View*`.

Evidence: Store **4/4**, focused remote-view/wire **5/5**, all AwNet **24/24**,
`aw-store --lib` **212/212**, composite image **7/7**, strict affected-target Clippy green for `aw-store --lib` and the `aw` net binary with `-D warnings`, formatting/diff green.

This is **not yet a shipping remote-view claim**. Still needed:

- V1.5 Workspace target/lens and exact registered remote root/worktree binding;
- a production-authorized two-node continuity → grant → reconnect → protected
  read → revoke acceptance run;
- remote Agent/member view authority where applicable;
- offline/stale native/browser presentation behavior;
- live browser/native focus/layout/Agent-draft acceptance;
- no local fallback and no execution retarget throughout.

Do not hand-author an identity-continuity snapshot that production source export
would not authorize just to manufacture the two-node proof.

### Browser crash / partition semantics

Recent ACK is presentation/liveness evidence only. Still need deliberate
post-ACK process/network partition semantics beyond current inspect/revoke state.

### Broker crash/reboot

No PTY execution restoration promise exists yet. Do not replay/recreate shells.

### macOS / Windows PTY custody

Separate native acceptance.

### Windows scoped-directory runtime

Compilation exists; actual Windows retained-directory/reparse/symlink runtime
acceptance remains open.

### Visual / screenshot acceptance

Live browser visual/screenshot acceptance is a separately executed gate. Rust or
Node owner tests do not substitute for it.

---

## 15. Source map

### Team / delegated browser authority

- `src/team.rs`
- `src/web.rs`
- `src/web/team.rs`
- `src/web/team/delegated.rs`
- `src/web/delegation.rs`
- `src/web/surface_read.rs`
- `src/web/delegated_surface_tests.rs`
- `src/web/dock.rs`
- `src/web/agent_view.rs`
- `src/web/workspace.rs`
- `src/web/team/tests.rs`

### Shipping browser

- `web/src/main.jsx`
- `web/src/workspace-views.js`
- `web/src/document-docking.js`
- `web/src/agent-views.js`
- `web/src/agent-view-panel.jsx`
- `web/src/styles.css`
- relevant `tests/web_*.test.mjs`

### Identity / authority

- `crates/aw-store/src/identity_continuity.rs`
- `crates/aw-store/src/remote_view.rs`
- `crates/aw-store/migrations/035_remote_view_trust.sql`
- `crates/aw-store/src/auth_session.rs`
- `crates/aw-store/src/authority.rs`
- `src/awnet/identity_continuity.rs`
- `src/awnet/services.rs`
- `src/actor_owner.rs`
- `src/agent_jobs.rs`
- `crates/aw-agent-core/src/tool.rs`

### Native Agent / Files / Git

- `src/sessions/branch_view.rs`
- `src/workspace/reader.rs`
- `src/workspace/ui/agent_views.rs`
- `src/workspace/files.rs`
- `src/workspace/ui/files_views.rs`
- `src/workspace/ui/async_work.rs`
- `src/workspace/git.rs`
- `src/workspace/owner.rs`
- `crates/aw-surfaces/`
- `crates/aw-git/`
- `crates/aw-workspace/src/layout.rs`

### Terminal

- `src/workspace/docking.rs`
- `src/workspace/ui/terminal.rs`
- `crates/aw-terminal/`
- `crates/aw-platform-unix/`
- `tests/native_live_migration.py`
- `tests/native_migration_protocol.py`
- `tests/native_persistent_pty.py`

---

## 16. Recommended next implementation sequence

The current delegated File/Review slice is accepted but uncommitted. Preserve it
exactly; do not reopen its authority or UX unless new source/test evidence
contradicts the acceptance above.

1. Revalidate HEAD/status/staged/worktrees and preserve both accepted current
   slices: delegated File/Review plus the remote-view backend/protocol checkpoint.
2. Do not commit/push/tag/release/deploy without explicit user authorization.
3. Next, wire shipping remote Workspace Files/Git views through the existing
   V1.5 target/lens and exact registered root/worktree bindings, consuming the
   v35 grant and protected AwNet `View*` path rather than creating another
   permission engine.
4. When that product layer supplies real Domain/Machine context, execute the
   production-authorized two-node continuity→grant→reconnect→read→revoke gate.
   Do not fabricate it through a hand-authored continuity snapshot.
5. Keep remote Agent/member views, post-ACK arbitrary process/network
   partitions, remote/distinct-Actor PTY custody, macOS/Windows custody/broker
   restoration and Windows retained-directory runtime acceptance as separate
   boundaries unless explicitly taken on.
6. Keep V1.5 UX authority read-only and do not reimplement same-UID Linux
   `terminal-promote`.
7. At the next coherent checkpoint, update durable docs and pause.

---

## 17. What not to do

- Do not write to `.agents/plans/ux/**`.
- Do not create a new UX design that contradicts V1.5 without explicit user
  approval/versioned successor.
- Do not resurrect a third Agents mode.
- Do not invent another global Quick Switch.
- Do not globally steal Ctrl+P from composer/PTY.
- Do not turn a delegated File into a Files root.
- Do not turn a delegated Review into Git inventory/write authority.
- Do not let Team membership alone grant a Surface.
- Do not let browser paths/labels/tab IDs/tickets become resource authority.
- Do not move delegated documents into unrelated ordinary scopes.
- Do not re-delegate a delegated document unless a separately designed authority
  chain explicitly permits it.
- Do not make same-owner browser pairing equivalent to distinct-user delegation.
- Do not retarget current Agent or execution site from view actions.
- Do not reimplement Linux `terminal-promote`.
- Do not replay shell/input/process effects after ambiguous custody.
- Do not promise remote-device trust, broker reboot restoration or native
  macOS/Windows acceptance without executing those gates.
- Do not claim visual/browser acceptance from Rust/Node tests alone.

---

## 18. Accuracy / provenance rule

When claims conflict:

1. live source;
2. executed current tests;
3. direct user instruction for this effort;
4. UX subtree for intended presentation/interaction;
5. active workspace-views TODO/spec;
6. this handoff;
7. dated historical prose.

Important examples:

- historical SQLite v33 statements belong to their commits; current Store is v34;
- historical PTY “migration not implemented” statements are superseded for
  same-UID Linux by `9d82f1a`;
- `workspace-views/TODO.md` may lag a just-landed commit until reconciled;
  `c9aaf0e` is committed source truth;
- the current dirty File/Review UI code is accepted by focused tests plus real
  two-profile Chrome evidence, but remains uncommitted; do not confuse accepted
  working-tree state with a landed commit.

---

## 19. Starting prompt

The maintained pasteable next-agent prompt is:

`.agents/plans/workspace-views/NEXT-AGENT.md`

It must carry the explicit user override:

> Read `.agents/plans/ux/**` as the UI/UX source of truth, but never modify,
> stage or commit that subtree.

A new agent should be able to start from that prompt plus this handoff without
replaying the conversation.
