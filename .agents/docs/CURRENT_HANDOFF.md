# CURRENT HANDOFF — AW project state, workspace-views next

Author: coordinating implementation agent
Owner: @pvl
Prepared: 2026-11-14
Source authority host: **L5 only**
Canonical AW checkout: `/home/pvl/spaces/abird/src/aw`
Canonical dotlink checkout: `/home/pvl/spaces/abird/src/dotlink`

> Start here for the current AW project state.
>
> This handoff deliberately spans the finalized unified Jobs + Agent runtime,
> release-readiness, and the active workspace-views program. It records the
> design lineage, implementation checkpoints, permanent decisions, current
> validation, exact pending work, and next-agent reading order.
>
> **Authority order:** live source/tests/executed results > `AGENTS.md` >
> current plan index/queue > selected plan README/TODO/specs > this handoff >
> older handoffs/history.
>
> Never treat historical unchecked milestone-local bullets as current product
> truth without reconciling them against later commits and the live TODO.

---

## 0. Immediate state — read before editing

### AW

- Repo: `/home/pvl/spaces/abird/src/aw`
- Branch: `main`
- Current HEAD: `88cd9655d402ea325873470202fd12133b1d8199`
  - `Record finalized unified runtime pins`
- Final unified runtime implementation:
  `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
  - `Complete unified Jobs and Agent runtime`
- Normative unified-runtime design authority remains:
  `890e9a383fe2edc3e0bd0aa34a2b217f2724475c`
- Unified Jobs + Agent runtime is finalized across AW and dotlink. Do not reopen
  D1/D2/D3, resurrect removed model routes, or repeat those qualification suites
  unless a changed owner/dependency requires it.
- Current live protocol/version facts from source: AW 0.15.0, SQLite schema **v34**
  (`crates/aw-store/src/lib.rs`), JSONL v1, native Workspace layout v4. Historical
  workspace-view milestones that say SQLite v33 are correct for those commits,
  not the current Store. The repo guidance still contains an older v33 sentence;
  live source outranks it. Do not downgrade or rewrite history to make them match.
- Current active plan queue:
  - P1 `release-readiness`
  - P2 `workspace-views`
  - P2 `dotlink-tool-port` remains listed separately in the queue/lineage; do
    not conflate that queue entry with the already finalized unified runtime.
- The next substantive implementation boundary is **workspace-views distinct
  user/Actor delegation + individually narrowed/revocable browser authority**.
- At the beginning of this handoff refresh, the only pre-existing owned dirty
  paths were:
  - `.agents/plans/workspace-views/TODO.md`
  - `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
- This handoff refresh additionally updates/creates:
  - `.agents/docs/CURRENT_HANDOFF.md`
  - `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
  - `.agents/plans/workspace-views/NEXT-AGENT.md`
- Nothing should be staged, committed, pushed, tagged, released or deployed
  without explicit user authorization.
- Another owner reserves `.agents/plans/ux/**`. Never list/read/write/stage/
  reset/stash/commit that subtree or inspect its staged entries.

### dotlink

- Repo: `/home/pvl/spaces/abird/src/dotlink`
- Branch: `main`
- Final unified bridge:
  `c90a6603ccef0bbf626ce10d95bedf59877747b8`
  - `Finalize dotlink unified Jobs bridge`
- dotlink pins `aw-tools`, `aw-tools-fs-unix`, and `aw-tools-fs-win`
  exactly to AW `0a6c6a0...`.
- Temporary remote-Start digest mirroring is gone; dotlink consumes
  `aw_tools::remote_start::arguments_sha256` from the exact Git pin.
- Final locked qualification:
  - all-target/all-feature check green;
  - 651 passed / 0 failed / 1 intentional external-fixture ignore;
  - integrations 1/1, 8/8, 1/1, 12/12, 4/4;
  - strict all-target/all-feature Clippy green;
  - fmt/diff green.
- Only dirty dotlink path is unrelated pre-existing
  `docs/CHATGPT_PLUGIN.md`; preserve it.
- Separate worktree `/home/pvl/spaces/abird/src/dotlink-016-fixes` remains
  untouched. Do not create another worktree.

### User/process constraints

- Use **dotlink---l5 only** for implementation/source work.
- Do not use A1.
- x2 may be used as an inference host only; it is not source authority.
- Preserve other work.
- Keep each model/user implementation turn near ~30 minutes and stop at a
  coherent checkpoint.
- Requalify by changed owner/dependency, not by habit.
- No destructive Git operation, commit/push/tag/release/deploy without explicit
  authorization for that action.

---

## 1. Exact reading order for the next agent

Do this before editing.

### Repository/core

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. this file
5. live `git status --short`, HEAD, staged paths and worktrees

### Active workspace-views plan

6. `.agents/plans/workspace-views/README.md` — current design and decisions
7. `.agents/plans/workspace-views/TODO.md` — commit chronology, current scope,
   test evidence and deferred work
8. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md` — current Linux
   original-PTY custody protocol
9. `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md` — pidfd/parent/
   reaping/adoption lifecycle
10. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md` — historical
    prerequisites plus current Linux supersession notes
11. live source/tests for the exact owner you intend to touch

### Release readiness

12. `.agents/plans/release-readiness/TODO.md`
13. `.agents/plans/release-readiness/CLI-COMPATIBILITY.md`
14. `.github/workflows/native-release.yml` only if working on release gates

### Finalized unified runtime lineage

15. `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
16. `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/README.md`
17. all of its `DESIGN.md` and `TODO.md`
18. predecessor plans only when needed:
    - `ai-native-operations-agents-jobs-transcript-2026-11-10/`
    - `ai-native-job-control-wake-2026-11-10/`
    - `dotlink-tool-port/`

### Older product/runtime history

19. `.agents/docs/HANDOFF.md`
20. `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md`
21. Git history and the selected plan's committed groups

---

## 2. Current queue and practical priority

### P1 release-readiness

The local/source work is largely complete. Remaining items are genuinely
external or owner-decision gates:

- execute exact release assets on:
  - Linux ARM64;
  - macOS ARM64;
  - Windows x86_64;
  - Windows ARM64;
- decide version/release policy after compatibility review:
  - source SDK revision pinning;
  - one-way native layout upgrade;
  - mixed-version capabilities policy;
  - 0.16.0 remains a proposal; source is still 0.15.0;
- configure Developer ID / Authenticode identity signing and notarization;
- run live-provider/billing compatibility only when separately authorized and
  configured;
- obtain independent security review.

Do not simulate these with cross-compilation/emulation and call them complete.

### P2 workspace-views

This is the next substantive implementation program.

Current genuine unimplemented scope:

- distinct user/Actor delegation;
- individually narrowed and revocable browser grants;
- remote-device trust;
- post-ACK browser crash/partition semantics beyond inspect-only state;
- broker crash/reboot execution-restoration semantics;
- remote/distinct-Actor PTY custody;
- macOS/Windows PTY custody/runtime acceptance;
- actual Windows scoped-directory/DirectoryIdentity filesystem runtime tests;
- remote views with exact machine/member/revision/expiry/offline authority tests;
- separately owned live browser screenshot/visual acceptance.

Same-UID Linux live PTY migration is **not** unfinished work.

---

## 3. Workspace-views: original design

The accepted design began at `47ee8a0...` with one principle:

> Multiple views/resources inside one Workspace must project existing owners and
> exact capabilities. A view is never a second runtime and never creates
> authority from labels, paths, layout state or browser authentication.

### 3.1 One current Agent and composer

- `Conversation` follows the current Agent.
- Read-only `AgentView` pins exact Store/Session/branch identity.
- Opening a view does not activate an Agent.
- Activation remains a separate action and passes the existing busy/recovery/
  draft checks.
- An unsaved Agent is not persisted merely to create a view.
- Closing a view never implicitly stops its Agent or PTY.

### 3.2 Read-only projection

- Reuse the strict Store read path shared with activation.
- Do not mint a `SessionHandle` for inspection.
- Reuse authorization, coordination and history validation.
- Viewing uncertain committed history labels it; it never acknowledges/replays it.
- Reader refresh revalidates authority even when branch head is unchanged.
- A denied/revoked view clears plaintext plus derived Find/selection state.
- Async work carries tab ID + exact target + generation; late/superseded replies
  are discarded.

### 3.3 Presentation identity is not authority

- Tab/container/pane IDs identify presentation state.
- Names, labels, positions and paths are never authority.
- Saved descriptors are requests to recover/reconnect authorization, not grants.
- View state must not retarget the current Agent.
- Moving/reparenting preserves exact resource identity.

### 3.4 Bounded layout/container model

- Flat stable-ID graph.
- One parent/container level.
- Cycle/orphan checks.
- Global limits:
  - 8 panes/containers where applicable;
  - 32 total tabs/documents.
- Existing layout migrations preserve geometry, IDs, focus and exhausted
  allocators or fail atomically.
- Old readers reject newer layout versions rather than silently dropping fields.

### 3.5 Publication/concurrency decision

- Prefer the existing `aw_agent_core::publication` only when measured immutable,
  read-heavy shared state benefits.
- One publisher, one registered reader per long-lived consumer, owned Arc
  snapshots outside guards.
- Never put a registered reader behind another shared Mutex.
- Keep admission/receipt/PTy/queue/resource locks when their invariants require
  them.
- Current single-owner view caches do not justify another publication layer.
- Browser Session snapshot and multiplayer presence were explicitly selected for
  measurement before any migration.

---

## 4. Workspace-views implementation chronology

The canonical TODO is the exhaustive record. The milestones below are the
high-level implementation sequence and why each exists.

### 4.1 Native read-only Agent views

- `47ee8a0` — accepted design checkpoint.
- `1c2ec6e` — native read-only Agent views.
  - exact Store/Session/branch projection;
  - independent viewport/Find/selection;
  - no activation/effect side effects.
- `2809fa5` — asynchronous Find completion.
- `3b0dbb5` — Find intent/request lifetimes, including stale completion fencing.

### 4.2 Scoped native Files

- `6b96f01` — retained Windows directory bindings.
- `3851dbf` — native scoped Files containers.
  - independent `ScopeId`;
  - retained `WorkspaceFiles` capability;
  - explicit external-folder reconnect;
  - same relative path under different scopes remains distinct;
  - capability registry owned by UI/Host, not Agent root.
- Layout v3 migrated earlier layouts without adding SQLite/JSONL schema change.

### 4.3 Native Git containers

- `491f366` — retained repository identities.
- `b33a418` — native scoped Git containers.
  - exact authorized repository/worktree context IDs;
  - immutable Review targets;
  - selected-index-only staging;
  - reconnect/replacement checks;
  - Files scope and Git context remain independent.
- Layout v4 retained old Files wire fields and added Git ownership.

### 4.4 Browser read-only Agent views

- `a0ffa3e` — browser read-only Agent tabs.
- `68629d8` — explicit browser draft targets.
- `94697a7` — reachable draft consent and final browser acceptance.
- Shipping Solid shell, not the old historical browser reducer, is the product
  surface.
- Browser authentication is necessary but never sufficient Store authority.
- Current Agent/draft/caret and execution counts stay unchanged while browsing.

### 4.5 Scoped browser Files/Git

- `57a6059` — scoped browser Files/Git views.
- `0159f0b` — bounded merged editors and final browser acceptance.
- Browser scoped requests retain owner lifetime + immutable scope/context.
- Git inventory handles grant no Files capability.
- Scoped wrappers are read-only and cannot wrap terminal/mutation commands.
- Late close/refresh/source replies cannot redirect content into another view.

### 4.6 Publication measurement

- `2c07425` — measured real HTTP publication/shared-draft presence correctness.
- Real release-mode benchmark compares existing publication implementation with
  actual Mutex read shapes, multiple reader counts and read/write ratios.
- Decision remains evidence-driven; publication does not replace fresh authority,
  TTL or sequence checks.

### 4.7 Browser docking

- `e06ffa7` — same-window reparenting substrate.
- `7de8658` — owner-mediated File-only cross-window docking.
- `00559eb` — immutable Git Review cross-window docking.
- `41e66c4` — bounded read-only Find/scroll/selection/focus continuity.
- `e558919` — receiver-confirmed browser docking lifecycle.
- `0efea351` — explicit same-owner, independent Chrome-profile pairing.

Permanent meaning:

- transfer presentation/read authority only;
- no Agent/execution ownership moves;
- receiver revalidates exact owner/source/content;
- short-lived receiver proof is a safety check, not proof of perpetual browser
  liveness;
- future distinct-user/Actor work must not reuse same-owner pairing as if it were
  cross-user delegation.

### 4.8 Native same-machine terminal docking and persistent PTYs

- `c98987a` — same-owner Unix second-terminal docking.
  - existing Workspace continues to own the PTY;
  - presentation/control moves temporarily;
  - same shell PID;
  - exclusive input.
- `bd87e5d` — opt-in owner-independent Unix PTY broker for newly launched PTYs.
  - new broker-owned shell;
  - survives launcher/frontend exit;
  - one-use control tickets;
  - explicit stop.

These were deliberately separate from migration of an already-running
Workspace-owned PTY.

### 4.9 Live migration prerequisites

Historical prerequisite milestones:

- `635e1ec` — reversible original reader quiescence.
- `a1100a9` — atomic PTY master + child pidfd escrow.
- `2feaa78` — Linux live-migration prerequisites.
- `207c552` — bounded parser checkpoint + staged owner FD escrow.
- `62f3735` — exact pidfd exit facts.
- `7ced795` — guarded local source release/passive parent custody.
- `2b8d769` — reproducible independent native fixtures.
- `9f1cc28` — inert adopted PTY reactor.
- `11999aa` — reader retirement when descendants retain slave.
- `a0c64c9`, `c51c681` — strict native lint coverage.

Their local “production migration still unimplemented” statements were true at
those commits only and are now historical.

### 4.10 Same-UID Linux original-shell custody complete

- `9d82f1a` — Linux original-shell custody transaction.
- `65f04ca` — actual different-UID FD admission refusal.

Current Linux truth:

- `terminal-promote` migrates an already-running Workspace shell.
- Exact shell PID survives.
- Full bounded parser state survives.
- Broker becomes sole reader/controller only after authenticated committed
  release.
- No command/input replay.
- Exact-pidfd signaling; no naked numeric-PID fallback.
- Source never resumes after committed release.
- Source crash before release does not authorize receiver activation.
- Different-UID FD admission is explicitly rejected in an isolated user
  namespace test.

Do **not** reopen same-UID Linux migration because earlier prerequisite sections
contain unchecked historical bullets.

---

## 5. Permanent Files/Git authority decisions

### Files

- A `ScopeId` names a requested human binding, not a path grant.
- Only an explicitly retained `WorkspaceFiles` capability authorizes access.
- Launch root is retained independently from any Agent resumed elsewhere.
- External folders require explicit reconnect after restart.
- Closing the last reference revokes unused external capability.
- Every read/list/search/highlight result carries scope/path/binding/lifetime.
- Reconnect invalidates old plaintext/render/Find/selection before new content.
- Unix relies on retained root descriptor traversal.
- Windows must retain directory/ancestor handles denying delete sharing; native
  runtime tests remain open.

### Git

- Git context ID names exact authorized checkout identity.
- Worktree selection mints a new context; it never retargets another context by
  pathname.
- Diff retains immutable Review + original context.
- Fresh staging rechecks exact review/HEAD/binding at serialized owner admission.
- Reconnect/final-reference closure/restore invalidates queued admission.
- Existing retained Diff may still stage its exact original checkout if fresh.
- Already admitted external mutation is not silently cancelled/replayed.
- Git-derived terminal launch rechecks captured binding before starting shell.
- These checks do not claim subprocess sandboxing or exclusion of external Git
  writers.

---

## 6. Browser authority and docking decisions

- Browser authentication does not grant Store, filesystem, Git or execution
  authority.
- LocalWeb owns/reuses one owner Store from startup.
- Browser reads name exact Store/Session/branch identities.
- Browsing never creates Agent/Session execution bindings or durable effect
  intent.
- Unsaved creation uses durable receipt-first semantics.
- File/Git browser scopes are retained owner capabilities, not client paths.
- Every long-running response must revalidate source/scope/lifetime before
  publication.
- Logout/revocation clears protected content and derived caches.
- Browser docking tickets transfer bounded presentation/read continuity, not
  Runtime/Agent/tool authority.
- Current same-owner pairing is not a distinct-user delegation mechanism.
- The next design must give another user/Actor individually narrowable,
  independently revocable authority rather than extending same-owner browser
  cookies/tickets.

---

## 7. Linux PTY custody — current contract

There are three distinct supported concepts.

### `terminal-dock`

- Moves presentation/control to another terminal.
- Original Workspace process retains PTY ownership.

### Workspace persistent start

- Launches a **new** broker-owned PTY.
- It can survive launcher/frontend exit.
- It is not migration of an existing shell.

### `terminal-promote`

- Deliberately migrates an **already-running** Workspace shell on same-UID Linux.
- Exact shell PID and parser-visible state survive source exit/SIGKILL.
- Transaction stages:
  1. local control fence;
  2. sole reader quiesced;
  3. bounded checkpoint + one master/pidfd escrow;
  4. release pending;
  5. source reader joined, master/writers closed, guard disarmed into passive
     parent waiter;
  6. broker activation only after exact signed release publication.

### Critical failure semantics

- FD receipt/readiness/source death does not grant activation.
- Pre-release failure can resume the exact source.
- Pending/Released is irreversible to the caller.
- Lost/duplicate ACK cannot reactivate source.
- Death before publication is UNKNOWN.
- Broker loss never causes input/shell replay or source reactivation.
- Broker SIGKILL/reboot does **not** currently promise execution restoration.
- A sibling broker does not become the child's parent.
- Preserve unknown/unavailable exit status for nonchildren.
- Source private signing key is ephemeral and zeroized; only public key is
  staged.
- Broker never creates Agent/State/provider/tool authority.

---

## 8. Fresh current-HEAD validation on `88cd965`

The current workspace-views revalidation after unified-runtime finalization is
green for the native/current-owned scope.

### Native layout/focus

Passed:

- `narrow_frame_keeps_desired_layout`
- `maximize_roundtrip_preserves_layout_draft_and_scroll`
- `maximizing_another_pane_leaves_the_composer_without_clearing_its_draft`
- `unified_palette_cancel_and_resize_preserve_invoking_focus_and_editor`
- `focusing_the_current_agent_preserves_busy_state_and_unsent_input`

Meaning:

- narrow frames preserve intended layout;
- maximize/restore preserves layout, viewport and draft;
- maximizing another pane does not clear composer draft;
- modal/palette cancellation and resize preserve invoking focus/editor;
- returning focus to current Agent preserves busy state and unsent input.

### Browser docking owner tests

- `AW_GIT=... cargo test --locked -p aw-cli --lib docking`
- **24/24 passed**

### Real current binary PTY migration

Freshly built current `target/debug/aw` passed all four:

- `parser`
- `vim`
- `less`
- `source-crash`

Verified:

- identical original shell PID;
- exclusive input/no replay;
- foreground Ctrl+C;
- parser state and alternate-screen behavior;
- recipient terminal restoration;
- explicit stop;
- source-crash does not falsely claim source terminal restoration after SIGKILL.

### Not claimed

- live browser visual/screenshot acceptance;
- macOS/Windows native PTY custody;
- remote-device custody;
- broker reboot/SIGKILL restoration;
- actual Windows retained-directory runtime tests.

---

## 9. Genuine remaining workspace-views implementation scope

### 9.1 Next substantive boundary — distinct user/Actor delegation

Design goal:

A receiver that is not merely another same-owner browser/profile must receive an
explicit delegated authority bound to exact Actor/user/source/site/resource and
lifetime.

Requirements:

- delegation must originate from a current authorized owner;
- exact recipient user/Actor identity;
- narrow resource set, not all browser capabilities;
- explicit grant revision/generation;
- independent revocation;
- revocation clears protected receiver content/state;
- stale/held replies cannot resurrect revoked authority;
- reconnect/reload does not silently regrant;
- no path/label/ticket fallback;
- no implicit Agent execution retargeting;
- no provider/tool authority unless separately and explicitly granted.

### 9.2 Individually narrowed/revocable browser grants

Current same-owner pairing and docking are insufficient for cross-user use.

Need:

- capability records scoped per recipient and resource;
- grant/revoke lifecycle with exact current authority check;
- independent File/Review/Agent-view grants;
- no inherited local Files root just because browser is authenticated;
- receiver session lifetime and logout/revocation cleanup;
- post-ACK crash/partition inspection semantics;
- do not treat recent ACK as perpetual liveness.

### 9.3 Remote-device trust

Still unavailable.

Before enabling remote views:

- exact machine/device identity;
- user/Actor membership;
- revision/security epoch;
- expiry;
- offline/reconnect behavior;
- no local fallback;
- no execution retargeting;
- held/stale reply tests;
- device removal/revocation tests.

### 9.4 Remaining PTY/platform acceptance

Still open:

- remote/distinct-Actor PTY custody;
- macOS live-custody acceptance;
- Windows live-custody acceptance;
- broker crash/SIGKILL/reboot execution restoration;
- arbitrary terminal-application compatibility beyond current fixtures.

### 9.5 Windows scoped-directory runtime

Compilation is green. Actual Windows filesystem execution remains open:

- retained directory handle behavior;
- directory identity/reparse-point replacement;
- symlink privilege/Developer Mode case.

### 9.6 Remote views

Remain disabled until exact authority/lifetime matrix exists.

### 9.7 Visual/browser acceptance

The current plan explicitly treats live browser screenshot/visual acceptance as
separately owned. Do not claim it from Rust/Node/browser-owner logic tests.

---

## 10. Recommended next implementation design

Do not start by changing UI.

Start from authority.

### Phase A — define delegated browser capability

First agree a small typed record, likely at Store/host authority layer, containing:

- grant ID;
- owner principal;
- recipient user/Actor;
- exact source/site;
- resource kind + immutable resource identity;
- grant revision/generation;
- issued/expiry;
- revocation state;
- optional parent authority evidence.

Do not use pathname, tab ID, browser cookie or display name as identity.

### Phase B — admission and revalidation

Add one owner-side path:

- owner grants exact receiver capability;
- every receiver read revalidates current grant;
- held response revalidates before publish;
- revoke invalidates receiver caches/content immediately;
- regrant creates a new generation so stale work cannot regain authority.

Reuse existing generation/revocation patterns where possible rather than
inventing another replay/authority system.

### Phase C — browser docking integration

Extend existing owner-mediated docking so a different authorized user/Actor can
receive only the delegated read capability.

Preserve:

- source content hash/identity checks;
- exact recipient;
- expiry;
- bounded one-use transaction semantics;
- no execution authority;
- no implicit Store/Files/Git grants.

### Phase D — negative matrix before UI polish

Test:

- wrong recipient;
- wrong Actor/user;
- revoked grant;
- revoke after request but before publish;
- grant reissued with new generation;
- browser reload after revoke;
- receiver crash/partition after ACK;
- stale ticket;
- same path under different scope;
- unchanged resource with revoked authority;
- logout/global security epoch;
- no Agent/root/draft/caret/model/tool side effects.

Only then add/adjust user-facing controls.

---

## 11. Source map for the next implementation agent

Read live source before editing; this is a navigation map, not a substitute.

### Browser / HTTP owner

- `src/web.rs`
- `src/web/dock.rs`
- `src/web/team/`
- `src/web/team/multiplayer.rs`
- browser auth/session boundary in current web modules

### Actor/user/authority

- `src/actor_owner.rs`
- `src/agent_jobs.rs`
- `crates/aw-agent-core/src/tool.rs`
- `crates/aw-agent-core/src/accounts.rs`
- `crates/aw-store/src/auth_session.rs`
- `crates/aw-store/src/authority.rs`
- Store membership/participation/grant code used by current browser/team paths

### Read-only Agent views

- `src/sessions/branch_view.rs`
- `src/workspace/reader.rs`
- `src/workspace/ui/agent_views.rs`
- `crates/aw-surfaces/src/lib.rs`
- `crates/aw-surfaces/src/contract.rs`

### Files

- `src/workspace/files.rs`
- `src/workspace/ui/files_views.rs`
- `src/workspace/ui/async_work.rs`
- `crates/aw-tools/src/browse.rs`
- `crates/aw-platform-win/src/scoped_fs.rs`

### Git

- `src/workspace/git.rs`
- `crates/aw-git/src/lib.rs`
- `src/workspace/owner.rs`

### Workspace layout/persistence

- `src/workspace/ui.rs`
- `src/workspace/persistence.rs`
- `crates/aw-workspace/src/layout.rs`

### Terminal/docking/custody

- `src/workspace/docking.rs`
- `src/workspace/ui/terminal.rs`
- `crates/aw-terminal/`
- `crates/aw-platform-unix/`
- native migration tests:
  - `tests/native_live_migration.py`
  - `tests/native_migration_protocol.py`
  - `tests/native_persistent_pty.py`

---

## 12. Finalized unified Jobs + Agent runtime — permanent context

The full detailed record is preserved at:

`.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`

Key final pins:

- AW runtime implementation:
  `0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`
- AW handoff/pin sync:
  `88cd9655d402ea325873470202fd12133b1d8199`
- dotlink unified bridge:
  `c90a6603ccef0bbf626ce10d95bedf59877747b8`

Permanent runtime decisions relevant to workspace work:

- one canonical Agent/Session/Actor/Turn/Job runtime;
- one shared `aw-tools::JobManager`;
- flat model-facing tools;
- canonical `agent` constructor;
- canonical `jobs_list/get/read/ack/send/cancel/subscribe` where host supports them;
- no old `subagent/team/plan/review_release/job_*` model routes;
- ToolBinding/capability authority carries source/effect/site + generation;
- stale revoke/regrant cannot regain authority from matching names;
- replay owner is checked before mutable current-state rechecks;
- contextual refs resolve before persistence; persisted facts are canonical;
- views must not become a second authority/runtime owner.

Why this matters for workspace-views:

Distinct browser/user delegation should reuse the same exact authority-generation
and fail-closed principles, not invent a parallel loose browser permission model.

---

## 13. Release-readiness context

Current external gates only:

- native release-asset execution on missing hosts;
- version/release decision;
- Developer ID / Authenticode / notarization;
- live provider/billing compatibility when authorized/configured;
- independent security review.

Already completed in that plan include:

- Linux x86_64 native package/runtime;
- musl/glibc gates;
- portable asset/installer checks;
- five-target build/assembly;
- macOS CodeDirectory page-hash integrity check;
- CLI/source compatibility review;
- prepared native-release workflow;
- local Linux execution of workflow builder/probe.

Do not conflate “cross-target compiled/assembled” with “native host executed.”

---

## 14. Git / plan ownership safety

- Coordinator owns shared queue reconciliation unless explicitly delegated.
- Plan owner owns its plan directory.
- Preserve dirty workspace-view plan edits.
- Never inspect or touch `.agents/plans/ux/**`.
- Never include unrelated dotlink `docs/CHATGPT_PLUGIN.md`.
- Do not touch `dotlink-016-fixes`.
- Before any commit:
  - inspect exact owned paths;
  - inspect staged stat;
  - stop if foreign paths are staged.
- Commits are signed.
- Commit is not push authorization.
- Push/tag/release/deploy each require explicit authorization.

---

## 15. Validation discipline

Do not rerun the world after every edit.

For workspace-views authorization work:

1. unit tests for typed grant/revoke state;
2. Store/owner race tests:
   - revoke before publish;
   - regrant generation;
   - stale held response;
3. authenticated Rust HTTP owner tests;
4. Node state tests for receiver cache/lifetime;
5. real browser acceptance only when that visual/browser owner is in scope;
6. full product gate only at coherent checkpoint or when a changed dependency
   invalidates prior evidence.

Carry prior green Linux PTY, Files/Git, runtime and dotlink evidence unless the
owner/dependency changed.

For platform-specific claims:

- compile is not native execution;
- cross-build is not host certification;
- skipped/ignored host gate is not execution;
- preserve explicit “unavailable/pending” state.

---

## 16. What not to do next

Do not:

- reimplement Linux `terminal-promote`;
- treat `207c552` historical unchecked bullets as current Linux TODOs;
- infer remote/distinct-Actor authority from same-UID FD transfer;
- make browser cookies/pairing codes into Store/Files/Git authority;
- grant Files access from a path supplied by browser;
- retarget a saved scope/context by pathname;
- let recent docking ACK imply perpetual receiver liveness;
- auto-replay input or recreate a shell after ambiguous custody failure;
- promise broker reboot restoration without a durable execution design;
- revive old `subagent/team/plan/review_release` model surfaces;
- create another Runtime/Agent owner for views;
- change SQLite/JSONL/layout versions unless the selected design actually
  requires it and the migration is explicit;
- call external release/security/provider gates complete without executing them.

---

## 17. Next coherent work sequence

1. Revalidate L5 HEAD/status and preserve the current workspace-plan edits.
2. Read workspace-views README/TODO and the three PTY lifecycle specs.
3. Inspect current browser docking/team/authority source and tests.
4. Write a short design note for **distinct user/Actor delegated browser read
   capability**:
   - resource identity;
   - recipient identity;
   - generation/revocation;
   - expiry;
   - publish-time revalidation;
   - cache invalidation;
   - crash/partition semantics.
5. Reuse existing Store/Actor/ToolBinding authority patterns where they fit.
6. Implement the smallest owner-side grant/revoke/read path first.
7. Add negative/race tests before UI changes.
8. Integrate with existing browser docking only after owner tests are green.
9. Checkpoint and pause.
10. Leave remote-device trust and PTY cross-user custody separate until the
    delegated browser authority model is proven.

---

## 18. Starting prompt

Use the maintained prompt at:

`.agents/plans/workspace-views/NEXT-AGENT.md`

That prompt should be enough to start a new agent without replaying this entire
conversation.

---

## 19. Accuracy rule

When prose conflicts:

1. live source;
2. executed current tests;
3. current plan TODO;
4. current design README/spec;
5. this handoff;
6. dated milestone prose.

Historical “not implemented” statements describe their commit unless the current
plan still carries the item open. Historical schema/version statements likewise
belong to their commit; current source is AW 0.15.0 / SQLite v34 / JSONL v1 /
native layout v4.

Likewise, a later green test does not retroactively certify a different host,
browser, authority model or external release gate.
