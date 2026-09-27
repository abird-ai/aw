# Read-only Agent views and composable Files/Git

Accepted/provenance checkpoint: `9f90e3df52ef684674ee9c103f157a2f5fbc07d3`.

Design only; no independent viewer/container implementation is claimed. Preserve [Domain authority](../../docs/DOMAIN-MODEL.md), [Workspace behavior](../../docs/WORKSPACE.md) and the existing Agent/SurfaceHost owners.

## Completed

- [x] Chosen first slice: native read-only pinned Agent views, before compound Files/Git containers, Solid counterparts or docking.
- [x] Existing stable tab IDs, bounded layout, shared transcript renderer and generation-fenced background executor provide the implementation seams.

## Uncommitted

- [ ] Retain exact durable Session and branch IDs in navigation; distinguish view ID, scoped resource key and container ID from title/name/pane position. Do not create a durable Session merely to display an unsaved Agent.
- [ ] Extend the existing strict `read_branch_view` seam as a bounded host-authorized read adapter, reusing current store policy without returning an effect-capable SessionHandle, activating a Session or bypassing active/unreconciled-run fences. Display unavailable/live/snapshot states factually.
- [ ] Keep one current Agent/composer. Explicit Open view creates read-only presentation; explicit activation still uses the existing busy/recovery/draft checks and saved binding. Failed activation retains the old owner and draft.
- [ ] Give each view independent scroll/follow/find/selection/focus state; matching current-owner views may observe its live projection. Revalidate access and invalidate revoked/removed cached content; snapshots are never grants.
- [ ] Fence every asynchronous result by owning view/container, exact scope/resource and generation. Close cancels owned work; late results cannot reopen, replace a newer preview or steal focus.
- [ ] Migrate view-layout data explicitly: read v1 as Follow current Agent, retain IDs/pins/geometry/human scope, validate before install, preserve atomic/no-follow/64KiB protections, reject unsupported downgrade. Do not bump SQLite/JSONL for presentation.
- [ ] Stop the first slice when two differently targeted read-only views coexist with independent viewport/find state and no current-Agent/draft change or provider/tool/Queue intent.
- [ ] Then compose native Files: one rooted navigator plus scoped document group; separate pinned File/Diff preview policy and ordinary Explorer focus from explicit palette navigation. Same paths in different scopes cannot alias.
- [ ] Then compose native Git: reuse repository contexts, exact reviews and fresh index admission. Selecting a worktree cannot retarget Agent/Files; review→file requires authorized path mapping.
- [ ] Use a flat stable-ID graph with one parent, cycle/orphan validation and one container level; count all placements against existing global 8-pane/32-tab limits. Do not embed another Workspace or owner runtime inside a tab.
- [ ] Add Solid counterparts through current capability APIs; do not promise unsupported browser resources or publish a shared serialized layout before both adapters/migration exist.
- [ ] Dock/breakout last: reparent the same descriptor/state with explicit cross-window ownership, close-origin and focus/lifetime transfer. Closing a view never implicitly stops an Agent/PTY; moving never starts/clones resources.
- [ ] Restore exact surviving focus/draft/caret on modal cancel; fallback only to its owned parent/empty state. Preserve palette/history/child-PTY keys and IME protections.
- [ ] Prove denied/revoked/same-title branches, stale replies, owner busy/UNKNOWN, duplicate views, narrow/Unicode/resize/maximize and actual PTY restoration, without new provider calls. Test actual native/browser cuts separately.
- [ ] Bound retained history/cache bytes across views. Prefer left-right publication where measured read-heavy ownership fits; retain existing projection/admission/receipt/resource locks where it does not. Benchmark the actual workload before another migration. Register readers once and release guards before callbacks/render/I/O/await.
- [ ] Remote views remain unavailable until authorized machine/member/revision/expiry/offline tests exist; no local fallback, root inference from saved paths or implicit execution retargeting.
