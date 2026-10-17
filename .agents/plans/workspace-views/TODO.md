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

## Uncommitted

- [~] Compose native Git using existing repository contexts and fresh exact-review/index admission; worktree selection cannot retarget Agent/Files — backend and native UI agents.
- [~] Extend flat ownership with independent Files and per-navigator Git bindings; migrate native v1/v2/v3 to v4, preserve geometry/IDs/limits and refuse unavailable container allocation atomically — model/persistence agent.
- [~] Retain exact checkout contexts across Host presentation changes; explicit open/reconnect, worktree selection, label-only saved hints and final-reference revocation — backend/native agents.
- [~] Fence origin-bound Git completions and queued owner index requests across close/rebind/restore; retain intentionally open immutable Diff contexts and existing fresh-review/HEAD checks.
- [~] Validate retained repository directory identities and Git mappings before operations/index admission without claiming exclusion of concurrent external writers — backend/platform helper agent.
- [ ] Add browser counterparts through existing capability APIs, with browser-specific acceptance and migration before any shared serialized layout.
- [ ] Add docking last with explicit cross-window ownership, reparenting, origin close and focus/lifetime transfer; moving never starts/clones resources.
- [ ] Evaluate left-right publication only at measured read-heavy shared ownership seams; benchmark before migrating locks and drop guards before render/I/O/await.

## Tests and verification

- [x] Store exact-ID/authority/revocation/active-run/history/no-effect regressions — `cargo test --workspace --locked`, pinned Nix shell on `3851dbf`, passed.
- [x] Layout migration, independent descriptors, split/move/global limits and native persistence protections — same executed workspace Cargo gate on `3851dbf`, passed.
- [x] Native real-Reader/executor/TestBackend cases: two targets, independent viewport/find/selection, owner/draft/caret, duplicate identities, expiry/revocation and unavailable states — same executed workspace Cargo gate on `3851dbf`, passed.
- [x] Actual native PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, synthetic private fixtures. Broad two-view/Unicode/resize/`o` acceptance passed on `1c2ec6e`; focused Searching→1 match/No matches/80 matches, exact pins, draft/caret, zero effects, quit 0 and identical stty passed with the immutable Nix executable on `3b0dbb5`.
- [x] `cargo check --locked -p aw-cli --no-default-features --features workspace` — pinned Nix shell on `3851dbf`, passed.
- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `3851dbf`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `3851dbf`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `3851dbf`, 1,608 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `3851dbf`, 107 passed.
- [x] Changed-content committed-Find and close/restore ABA regressions, including inactive footer/focus and consumed flash selection — executed by the workspace Cargo gate on `3851dbf`, passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=3851dbffd808491916e07a3642422bd0f7b465b0#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Further native narrow/maximize, modal-focus and child-PTY acceptance; browser/container/docking-specific checks after those slices exist — remain open beyond this checkpoint.

- [x] Scoped Files layout regressions: two same-path scopes, container membership/global limits, legacy full/exhausted layouts, malformed/mixed versions and split/move/close/save/load — `cargo test --workspace --locked`, pinned Nix shell on `3851dbf`, passed.
- [x] Actual retained-root backend regressions: distinct read/list/search results, unknown/revoked/reconnected scopes, root replacement and capability cleanup — same executed workspace Cargo gate on `3851dbf`, passed.
- [x] Native Browser/executor/TestBackend acceptance: independent same-path content/find/viewport, invoking-container preview/Explorer focus, stale reads/search/highlights, explicit refresh/reconnect races, moved-tab labels and unchanged Agent/draft/effect admission — same executed workspace Cargo gate on `3851dbf`, passed.
- [x] Actual scoped-Files PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, immutable Nix package on `3851dbf`, private synthetic fixtures: distinct same-path Unicode roots, independent Find/scroll, Explorer focus, exact draft/caret, save/split/load, restart with unavailable scope, explicit same-ID reconnect and 100×30→220×64 resize passed. Both app exits were 0 with identical before/after stty; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs stayed zero.

- [x] Windows scoped-directory crate and four real-filesystem tests compile — `cargo check --locked -p aw-platform-win --target x86_64-pc-windows-gnu --tests`, pinned Nix shell with Rust 1.98.1 Windows target on `3851dbf`, passed.
- [ ] Windows scoped-directory handle tests — execute on an actual Windows host; reparse-point case requires symlink privilege or Developer Mode. Compilation does not supply this runtime result.

- [ ] Git-container layout/persistence regressions: legacy migration, independent bindings, shared limits, split/move/close and malformed ownership/versions.
- [ ] Actual Git backend/executor regressions: distinct worktree contexts, delayed/closed/rebound replies, review expiry and selected-index-only fresh staging; stale contexts never mutate.
- [ ] Native Git/executor/TestBackend acceptance: origin-container routing/focus, independent filters/diff state, explicit open/reconnect, save/load and unchanged Agent/Files/draft/effect admission.
- [ ] Exact Git-container candidate: pinned product gate, Workspace-only feature boundary, native Nix package/version and real-terminal two-worktree acceptance.

## Deferred

- [ ] Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.
