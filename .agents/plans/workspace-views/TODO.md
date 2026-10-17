# TODO — Read-only Agent views and composable Files/Git

Status: active
Provenance: 5a8e90ae5000fdf5edeaea3fb90be21fec086de6
Accepted checkpoint: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7

Design and binding decisions: [README.md](README.md).

## Committed

### `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` — accepted design checkpoint

- [x] Choose native read-only pinned Agent views before Files/Git containers, browser counterparts and docking.
- [x] Reuse stable tab IDs, bounded layout, the transcript renderer and existing Agent/SurfaceHost owners.

### `1c2ec6eff437f506fd2ea404ed6dcc3cf66c22a3` — native read-only Agent views

- [x] Extract strict exact-ID `SessionStore::read_branch_view` without minting a SessionHandle; activation retains ID-or-name compatibility.
- [x] Publish a private authorized Host reader without creating an Agent/Session or entering the execution queue.
- [x] Reuse one Host-owned SQLite Store for unsaved browsing, activation and first prompt; retain exclusive owner.lock and avoid per-view recovery or absent-State/JSONL fallback.
- [x] Carry exact Store, Session and branch identities through owner/navigation projections; titles remain labels.
- [x] Add bounded AgentView descriptors and explicit consistent v1→v2 migration; preserve IDs, pins, geometry, root binding, atomic/no-follow and 64 KiB protections.
- [x] Add explicit Open read-only Agent view and Sessions `o`; retain Enter activation, one current Agent and composer-free pinned views.
- [x] Keep independent per-view scroll/follow/find/selection, bounded recent render state and factual display limits; immutable history/export stays authoritative.
- [x] Fence view reads by exact resource, tab and monotonic request lifetime; bound pending work and expire from request start.
- [x] Revalidate authority independently of head changes and immediately hide denied/revoked/removed-source content and derived state.
- [x] Update live Workspace/Build documentation and retain Files/Git/browser/docking/left-right work as later slices.
- [x] Exercise two exact targets in an actual native PTY on this commit: independent scroll/find, Sessions `o`, Unicode/resize, unchanged unsaved Agent/draft/caret and zero execution rows; quit exited 0 and restored stty. This is separate from TestBackend coverage.

### `2809fa58902df4dd9a23fb5a9a8fbd5e57d43189` — asynchronous Find completion

- [x] Show Searching until a validated Find completion; report hit/miss/failure truthfully and reject stale/duplicate/inactive footer updates.
- [x] Pass the pinned full product gate, native Nix package/version and focused real-terminal Find acceptance on this commit; follow-up refresh/restore corrections are recorded separately below.

### `3b0dbb502005570cca30e2cd52cba60c052f3587` — Find intent and request lifetimes

- [x] Preserve committed Find intent across changed Agent content, pending/missing results and inactive-pane refresh; n/N and flash consume selection mode without later key capture.
- [x] Allocate checked UI-lifetime Find request IDs so close/restore of a tab cannot accept another lifetime's actual reply; keep cancellation/source/generation admission intact.
- [x] Two actual-executor regressions failed before the fix and passed afterward; nearest native suite passed 153 tests with one existing ignored benchmark. Final exact-commit gate, feature boundary, native package/version and real PTY acceptance passed below.

### `6b96f010995ffa49414486ae63234a90ec9102e6` — retained Windows directory bindings

- [x] Pin canonical root, ancestor and per-operation descendant directories with opened metadata handles that deny delete sharing; preserve leaf no-follow/hardlink, bounded read, cancellation and edit-CAS rules.
- [x] Compile four real-filesystem Windows regression tests on cumulative Files candidate `3851dbf`. Native Windows execution remains unchecked; Windows ancestor/directory rename or deletion is blocked while the corresponding handles live.

### `3851dbffd808491916e07a3642422bd0f7b465b0` — native scoped Files containers

- [x] Compose Files navigators and document groups in flat, stable-ID ownership under the existing pane/tab limits; retain invoking-container preview policy and Explorer focus, including split-group routing.
- [x] Bind File identity and all derived caches to exact scope/path; retain explicitly opened root capabilities and human launch roots across Agent presentation changes. Saved IDs and labels grant no access.
- [x] Add explicit folder open/reconnect through the existing palette and bounded executor; reconnect moved File tabs by their own scope, prune unused capabilities and preserve bounded display-only folder labels.
- [x] Migrate consistent native v1/v2 layouts to v3 without adding panes or changing geometry, focus, legacy IDs or exhausted allocators; reject malformed/mixed versions and preserve root binding, atomic/no-follow and 64 KiB protections.
- [x] Fence read/list/search/highlight/find work by capability binding, exact resource/owner and checked lifetime across close/rebind/restore. Preserve ordinary moved-view state; clear revoked content and reissue pending refresh/palette queries correctly.
- [x] Admit review feedback against the lossless canonical current Agent root independently of human Files roots; retain owner serialization and exact review/index admission.
- [x] Cover model/persistence, actual retained-root backend and real native Browser/executor/TestBackend regressions, including held stale replies and split-container routing.
- [x] Pass the pinned product gate, Workspace feature boundary, Windows test compilation and native Linux package/version on this exact commit; actual two-root terminal and restart/reconnect acceptance passed with zero effects and restored stty. Windows runtime remains open.

### `491f3665b4de7f9fce3cfb4b4a14066d80e205d2` — retained repository identities

- [x] Retain physical directory identities for checkout root, worktree Git directory and common directory; validate exact Git mappings before operations and fresh index application without claiming exclusion of concurrent external writers.
- [x] Exercise deterministic checkout/admin-directory/gitfile replacement with real Git; add Windows directory-identity tests with target compilation kept separate from actual Windows execution.

### `b33a4183f6032a024d1b73409d2c9134ea0aab24` — native scoped Git containers

- [x] Extend flat ownership with independent Files scope and per-navigator exact Git context bindings under existing limits; preserve dormant bindings across navigator-kind switches without retargeting another view.
- [x] Migrate consistent native v1/v2/v3 to v4 without changing geometry or reusing IDs; preserve old Files wire fields, root/atomic/no-follow/64 KiB protections, and refuse genuinely unavailable container allocation atomically.
- [x] Add explicit Git open/reconnect through the existing palette and executor; retain opened contexts through Host presentation changes. Saved IDs and bounded labels never grant access.
- [x] Route delayed reviews to the originating container's remembered group without changing focus/caret; fence closed/reconnected/restored lifetimes and keep worktree selection independent per navigator.
- [x] Retain immutable moved/open Diff snapshots against their original checkout; preserve completed Find/viewport/selection and fresh selected-index staging without changing Files/Agent roots or worktree HEADs.
- [x] Recheck queued index admission on the serialized owner; reconnect, restore and final-reference closure revoke old admission, with no replay of uncertain external effects.
- [x] Use exact focused Git/Diff context for explicit terminal launch and validate it on the executor before starting a shell; deny unknown, revoked or replaced context and preserve existing terminal bindings.
- [x] Keep Files work valid inside Git-only groups and independent Files reconnect from cancelling Git requests; update live Git/Workspace/Build documentation and remove stale phase-smoke/not-compiled claims.
- [x] Pass the exact pinned product gate, Workspace feature boundary, Windows platform/Git-library compilation and native Linux package/version; complete actual two-worktree terminal and restart/reconnect acceptance. Windows runtime remains unchecked.

### `a0ffa3ec9e3fb3d3c713d8cea1a8c694543d002e` — browser read-only Agent tabs

- [x] Retain one LocalWeb Store from startup for unsaved browsing, activation, provider custody and first-prompt creation; strict exact-ID reads never create or activate a Session.
- [x] Share the native authorized privacy projection with the browser; revalidate exact Store/owner lifetime/Session/branch, membership and local target on every read, including unchanged heads.
- [x] Add bounded memory-only views with independent Find/viewport/selection, checked request lifetimes, cancellation and expiry; scrub closed/denied pending entries before abort callbacks.
- [x] Integrate explicit read-only branch actions and composer-free Solid panels; preserve the actual composer node, draft and caret, fence stale owner observations and clear protected labels/metadata on denied access.
- [x] Resolve durable receipts before lazy creation or account selection; preserve memory-only deduplication and retained-Store owner-replacement retries after account revocation without another Session or inference.
- [x] Pass the exact pinned product gate: bootstrap, formatting, 1,649 Rust tests with 2 existing ignored, and all 125 browser tests; Workspace-only check and 132 web-only library tests passed. Native Nix package/version passed.
- [x] Actual Chrome 155 against immutable package and real Rust/SQLite owner: two branches retained independent 120-hit Find, 1,100/1,750-pixel scroll and selection; actual composer/draft/caret and owner were unchanged. Unchanged authorized refresh and a changed hidden response retained state, a held actual owner reply could not reopen its closed tab, and unchanged-head membership revocation cleared transcript/Find/selection/labels. This predates the draft-retarget follow-up below.

### `68629d8ddcdba4bb502ef21ea04d5bae006f5fdd` — explicit browser draft targets

- [x] Bind private unsent text to minimal owner/Session/branch identity; external retargeting preserves draft/caret and blocks Send and publication until explicit Use draft here, which does not submit.
- [x] Consume confirmed local branch-open consent only for unchanged text/origin and its exact canonical target, including matching SSE-before-HTTP; quiet updates and unrelated targets grant no consent.
- [x] Fence late navigation/reset UI replies by browser lifetime, preserve text rebound during accepted-send waits, and keep shared-draft exact target/protocol semantics.
- [x] Rebuild the production Solid bundle and correct canonical CLI/current LocalWeb documentation without changing runtime protocols or State schemas. Exact pinned bootstrap/format/workspace Cargo and six-suite Node gate passed: 1,649 Rust tests, 2 existing ignored, 125 Node tests; immutable Nix package/version passed. The notice clickability correction follows below.

### `94697a7899ae52eb11167519c9abc05ee3517756` — reachable draft consent and final browser acceptance

- [x] Keep the consent notice in composer flow with pointer events restored and long labels wrapped; ordinary Use draft here remains reachable at 390 px. Global error notices retain their placement.
- [x] Pass the exact pinned bootstrap/format/workspace Cargo and six-suite Node gate: 1,649 Rust tests, 2 existing ignored, 125 Node tests, no failures. Rebuild the production Solid bundle into a private directory and match all eight committed artifacts byte-for-byte.
- [x] Build the exact immutable Nix package and run `bin/aw --version`: aw 0.15.0, exit 0. Served JS/CSS matched committed bytes.
- [x] Actual Chrome 155 against that immutable binary and a private Rust/SQLite owner: two branches retained independent 120-hit Find, scroll and selection; composer node/draft/caret and active owner stayed unchanged. Unchanged authorized refresh kept selected DOM; a changed hidden response retained Find/viewport; a held real owner reply could not reopen a closed tab, and reopening allocated a fresh lifetime.
- [x] Actual unchanged-head membership revocation cleared protected transcript, Find, selection and labels and refreshed the catalog. A held real snapshot and queued old SSE could not revive cleared owner DOM after a simulated transport 401; the LocalWeb fixture has no paired-cookie expiry.
- [x] Actual final bundle at 390 px with long Session/branch labels: external retarget disabled Send/Enter, ordinary Use draft here enabled the preserved draft separately from submission, and no effectful POST occurred. Additional instrumented early DOM/transport checks covered publication consent, same-target revision, matching activation SSE-before-HTTP and unrelated-target/new-text races; these do not claim live paired/shared acceptance.
- [x] Final private fixture retained exactly two Sessions, identical branch heads/revisions and zero runs/run_events/invocation_receipts/pipeline_runs/team_runs. Chrome recorded no uncaught runtime errors; the private owner exited 0. Transient diagnostics and completed agent scratch are removed after reconciliation.

## Uncommitted

- [ ] Add memory-only scoped browser Files/Git views through retained SurfaceHost capabilities: launch-root Files and verified existing Git worktrees first, exact source/binding identity and independent state. Another Files root requires an explicit trusted-owner grant contract. Complete actual browser acceptance before any shared serialized layout.
- [ ] Add docking last with explicit cross-window ownership, reparenting, origin close and focus/lifetime transfer; moving never starts/clones resources.
- [ ] Evaluate left-right publication only at measured read-heavy shared ownership seams; benchmark before migrating locks and drop guards before render/I/O/await.

## Tests and verification

- [x] Exact follow-up product gate/package and actual Chrome draft-retarget/late-observation/closed-view/revocation acceptance — passed on `94697a7` as recorded above, with unchanged branch heads and zero execution effects.

- [x] Browser Agent HTTP regressions against real SQLite State: exact authority/identity, unsaved owner reuse, privacy, revocation and zero execution effects — `cargo test --workspace --locked`, exact pinned `94697a7`, passed.
- [x] Browser Agent view model regressions: delayed/closed/superseded replies, duplicate targets, bounded caches, independent state, expiry and logout — existing six-suite Node command from TODO-SPEC.md, exact pinned `94697a7`, 125 passed.
- [x] Shipping Solid production build — `npm --prefix web run build -- --outDir PRIVATE_TMP`, exact pinned `94697a7`, all eight artifacts matched committed bytes. `nix build git+file:///home/pvl/spaces/abird/src/aw?rev=94697a7899ae52eb11167519c9abc05ee3517756#aw --no-link --print-out-paths --no-update-lock-file --cores 8` and full pinned product gate passed.
- [x] Actual shipping browser against the Rust owner and two real SQLite branches: independent Find/scroll, unchanged active Agent/draft/caret, denied refresh clearing, stale close protection and zero execution effects — Chrome 155 on immutable `94697a7`, passed as scoped above.

- [x] Store exact-ID/authority/revocation/active-run/history/no-effect regressions — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Layout migration, independent descriptors, split/move/global limits and native persistence protections — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native real-Reader/executor/TestBackend cases: two targets, independent viewport/find/selection, owner/draft/caret, duplicate identities, expiry/revocation and unavailable states — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual native PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, synthetic private fixtures. Broad two-view/Unicode/resize/`o` acceptance passed on `1c2ec6e`; focused Searching→1 match/No matches/80 matches, exact pins, draft/caret, zero effects, quit 0 and identical stty passed with the immutable Nix executable on `3b0dbb5`.
- [x] `cargo check --locked -p aw-cli --bin aw --no-default-features --features workspace` — pinned Nix shell on `b33a418`, passed.
- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `b33a418`, 1,638 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `b33a418`, 107 passed.
- [x] Changed-content committed-Find and close/restore ABA regressions, including inactive footer/focus and consumed flash selection — executed by the workspace Cargo gate on `b33a418`, passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=b33a4183f6032a024d1b73409d2c9134ea0aab24#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Further native narrow/maximize, modal-focus and child-PTY acceptance; browser/container/docking-specific checks after those slices exist — remain open beyond this checkpoint.

- [x] Scoped Files layout regressions: two same-path scopes, container membership/global limits, legacy full/exhausted layouts, malformed/mixed versions and split/move/close/save/load — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Actual retained-root backend regressions: distinct read/list/search results, unknown/revoked/reconnected scopes, root replacement and capability cleanup — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native Browser/executor/TestBackend acceptance: independent same-path content/find/viewport, invoking-container preview/Explorer focus, stale reads/search/highlights, explicit refresh/reconnect races, moved-tab labels and unchanged Agent/draft/effect admission — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual scoped-Files PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, immutable Nix package on `3851dbf`, private synthetic fixtures: distinct same-path Unicode roots, independent Find/scroll, Explorer focus, exact draft/caret, save/split/load, restart with unavailable scope, explicit same-ID reconnect and 100×30→220×64 resize passed. Both app exits were 0 with identical before/after stty; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs stayed zero.

- [x] Windows platform scoped-directory and DirectoryIdentity tests compile — `cargo check --locked -p aw-platform-win --target x86_64-pc-windows-gnu --tests`, full pinned Nix shell with Rust 1.98.1 Windows std on `b33a418`, passed.
- [ ] Windows scoped-directory and DirectoryIdentity filesystem tests — execute on an actual Windows host; reparse-point case requires symlink privilege or Developer Mode. Compilation does not supply this runtime result.

- [x] Git-container layout/persistence regressions: legacy/mixed migration, independent and dormant bindings, shared limits, split/move/close, malformed ownership/versions and atomic allocation refusal — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Actual Git backend/executor regressions: distinct worktrees, delayed/closed/rebound/restore replies, launch discovery replacement, retained immutable reviews, context bounds/pruning and selected-index-only fresh staging — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native Git/executor/TestBackend acceptance: originating remembered-group/focus, independent Find/viewport, moved completed selection, explicit open/reconnect, mixed File views, unchanged Agent/draft, and queued terminal denial after replacement/revocation — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual scoped-Git PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, immutable Nix package on `b33a418`, private two-worktree fixture and trusted real-Git wrapper with an explicitly armed 3-second diff delay: origin routing/Explorer focus, distinct Unicode reviews with independent 120-hit Find/scroll, selected-index stage/unstage and stale refusal, unchanged Files/Agent/draft/caret/HEADs, save/load, 100×30→220×64 resize, unavailable restart contexts and exact same-ID reconnects passed. Both app exits were 0 with byte-equal before/after stty; both indexes ended clean; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs stayed zero.

- [x] Windows Git library compiles — `cargo check --locked -p aw-git --target x86_64-pc-windows-gnu --lib`, full pinned Nix shell with Rust 1.98.1 Windows std on `b33a418`, passed.

## Deferred

- [ ] Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.
