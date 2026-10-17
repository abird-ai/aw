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

## Uncommitted

- [ ] Compose native Files as a rooted navigator and scoped document group; preserve separate File/Diff preview policy and Explorer focus.
- [ ] Compose native Git using existing repository contexts and fresh exact-review/index admission; worktree selection cannot retarget Agent/Files.
- [ ] Add a flat stable-ID container graph with one parent, cycle/orphan validation and one container level, within global 8-pane/32-tab limits.
- [ ] Add browser counterparts through existing capability APIs, with browser-specific acceptance and migration before any shared serialized layout.
- [ ] Add docking last with explicit cross-window ownership, reparenting, origin close and focus/lifetime transfer; moving never starts/clones resources.
- [ ] Evaluate left-right publication only at measured read-heavy shared ownership seams; benchmark before migrating locks and drop guards before render/I/O/await.

## Tests and verification

- [x] Store exact-ID/authority/revocation/active-run/history/no-effect regressions — `cargo test --workspace --locked`, pinned Nix shell on `3b0dbb5`, passed.
- [x] Layout migration, independent descriptors, split/move/global limits and native persistence protections — same executed workspace Cargo gate on `3b0dbb5`, passed.
- [x] Native real-Reader/executor/TestBackend cases: two targets, independent viewport/find/selection, owner/draft/caret, duplicate identities, expiry/revocation and unavailable states — same executed workspace Cargo gate on `3b0dbb5`, passed.
- [x] Actual native PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, synthetic private fixtures. Broad two-view/Unicode/resize/`o` acceptance passed on `1c2ec6e`; focused Searching→1 match/No matches/80 matches, exact pins, draft/caret, zero effects, quit 0 and identical stty passed with the immutable Nix executable on `3b0dbb5`.
- [x] `cargo check --locked -p aw-cli --no-default-features --features workspace` — pinned Nix shell on `3b0dbb5`, passed; the new unused binding warning is gone.
- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `3b0dbb5`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `3b0dbb5`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `3b0dbb5`, 1,572 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `3b0dbb5`, 107 passed.
- [x] Changed-content committed-Find and close/restore ABA regressions, including inactive footer/focus and consumed flash selection — executed by the workspace Cargo gate on `3b0dbb5`, passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=3b0dbb502005570cca30e2cd52cba60c052f3587#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Further native narrow/maximize, modal-focus and child-PTY acceptance; browser/container/docking-specific checks after those slices exist — remain open beyond this checkpoint.

## Deferred

- [ ] Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.
