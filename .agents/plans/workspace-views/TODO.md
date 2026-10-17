# TODO — Read-only Agent views and composable Files/Git

Status: active
Provenance: 5a8e90ae5000fdf5edeaea3fb90be21fec086de6
Accepted checkpoint: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7

Design and binding decisions: [README.md](README.md).

## Committed

### `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` — accepted design checkpoint

- [x] Choose native read-only pinned Agent views before Files/Git containers, browser counterparts and docking.
- [x] Reuse stable tab IDs, bounded layout, the transcript renderer and existing Agent/SurfaceHost owners.

## Uncommitted

- [~] Extract a strict `SessionStore::read_branch_view` reader from activation; keep handle creation in activation and exact branch IDs in pinned reads — backend agent.
- [~] Capture a private host reader without creating an Agent/Session or using the execution queue — backend agent.
- [~] Reuse one Host-owned SQLite Store for unsaved browsing and later activation/first prompt; preserve exclusive owner.lock and avoid reopening/recovery per view — backend agent.
- [~] Retain exact Store, Session and branch identities in owner/navigation projections; titles remain labels — backend and native agents.
- [~] Add bounded `AgentView` descriptors and explicit v1→v2 layout migration; preserve IDs, pins, geometry, root binding, atomic/no-follow and 64 KiB protections — layout agent.
- [~] Add explicit Open read-only view while retaining Enter activation and one current Agent/composer — native agent.
- [~] Keep per-view scroll/follow/find/selection/focus and bounded transcript/render caches — native agent.
- [~] Fence reads by tab, exact resource and generation; close discards owned work and stale replies — native agent.
- [~] Revalidate authorization independently of head changes; clear denied/revoked/removed content and derived view state — backend and native agents.
- [ ] Exercise two differently targeted views together without changing Agent identity, draft/caret or provider/tool/Queue intents — coordinator.
- [ ] Reconcile implementation, current documentation and actual test results; review the combined diff — coordinator.
- [ ] Compose native Files as a rooted navigator and scoped document group; preserve separate File/Diff preview policy and Explorer focus.
- [ ] Compose native Git using existing repository contexts and fresh exact-review/index admission; worktree selection cannot retarget Agent/Files.
- [ ] Add a flat stable-ID container graph with one parent, cycle/orphan validation and one container level, within global 8-pane/32-tab limits.
- [ ] Add browser counterparts through existing capability APIs, with browser-specific acceptance and migration before any shared serialized layout.
- [ ] Add docking last with explicit cross-window ownership, reparenting, origin close and focus/lifetime transfer; moving never starts/clones resources.
- [ ] Evaluate left-right publication only at measured read-heavy shared ownership seams; benchmark before migrating locks and drop guards before render/I/O/await.

## Tests and verification

- [ ] Store reader tests: exact IDs, read-only/denied/revoked authority, active-run fences, readable unresolved history, malformed/oversized history and no execution effects.
- [ ] Layout tests: v1 migration, rejected mixed/unsupported versions, independent descriptors, split/move and existing global limits — `cargo test --locked -p aw-workspace` plus native persistence tests.
- [ ] Native tests: two targets, independent viewport/find/selection, unchanged owner/draft/caret, same-title identities, duplicate views and unavailable states.
- [ ] Async tests: close/supersede/revocation cannot resurrect content, steal focus or accept another view's reply.
- [ ] Native rendering/input checks: narrow/Unicode/resize/maximize, modal focus restoration and child-PTY key ownership; distinguish TestBackend checks from actual PTY execution.
- [ ] `cargo fmt --all -- --check` — pinned Nix development shell, on the candidate commit.
- [ ] `cargo test --workspace --locked` — pinned Nix development shell, on the candidate commit.
- [ ] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — on the candidate commit.
- [ ] Browser/container/docking acceptance — only after those later slices exist; the first native slice does not close this plan.

## Deferred

- [ ] Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.
