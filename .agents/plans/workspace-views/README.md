Author: GPT-6 (Codex)

# Read-only Agent views and composable Files/Git

Let people inspect several saved branches without changing the Agent they are
working with. The first slice is native read-only Agent views. Stop that slice
when two differently targeted views coexist with independent viewport and find
state while the current Agent, draft and effect admission remain unchanged.
Files/Git composition, browser support and docking follow as separate slices.

## Design

`Conversation` follows the current Agent. `AgentView` pins exact Store, Session
and branch IDs. A tab ID identifies a particular view; names and pane positions
are labels and placement, never resource identity or authority. An unsaved Agent
is not persisted just to create a view.

Extract the strict Store read path shared with activation, without minting a
SessionHandle. Reuse current authorization, coordination, history validation and
active-run fences. Uncertain committed history remains labelled for inspection;
viewing never acknowledges or replays it. A private host adapter captures an
existing authorized store/actor and returns only owned transcript data. It must
not call Host::ensure, change the owner or enter the effect-command queue.
Unsaved browsing and later activation reuse one Host-owned SQLite Store so the
browsing capability cannot self-conflict with exclusive owner.lock. Reader
refreshes never reopen the database or run recovery per view.

Each view owns its scroll/follow/find/selection and bounded read/render state.
Async work carries its tab ID, exact target and generation. Closed or superseded
views discard replies. Access must be revalidated even when a branch head has
not changed; denial invalidates cached content and derived selection/find state.
Only an exact current-owner identity can use its authorized live projection.

The initial Agent-view layout v2 explicitly migrated consistent v1 layouts: existing Conversation
means Follow current. Preserve geometry, IDs, pins and root binding, validate
before installation, retain no-follow/atomic/64 KiB protections and reject
unsupported or mixed versions. Presentation adds no SQLite/JSONL schema change.
Old readers must reject v2 rather than silently dropping pinned targets.

Later containers use a flat stable-ID graph, one parent and one container level,
with cycle/orphan checks and global 8-pane/32-tab limits. A view never embeds a
second Workspace runtime. Moving/reparenting keeps resource identity; closing a
view never implicitly stops an Agent or PTY.

## Decisions

- Keep one current Agent and composer. Open read-only view is explicit; activation
  remains a separate action through current busy/recovery/draft checks.
- Finish native views before Files/Git containers, then browser counterparts,
  then docking. Do not claim browser parity from native tests.
- Preserve Domain → Workspace → Session and exact authority. Saved descriptors
  are requests for authorization, never grants or path-based fallback authority.
- Prefer left-right publication where immutable, read-heavy shared state makes
  it a measured fit. Single-owner view state needs no new concurrency primitive;
  keep admission/receipt/resource locks when their invariants require them.
  For future shared bounded branch/catalog projections, reuse
  `aw_agent_core::publication`: one publisher, one registered reader per
  long-lived consumer, and owned Arc snapshots outside guards. Benchmark actual
  read/write ratios and retained bytes before migration. Keep fresh authority
  checks and view expiry independent of publication. The current native cache
  has one UI owner and workers return owned replies; its tiny optional-reader
  Mutex does not show a measured need for a second publication layer.
- Remote views remain unavailable until their exact authority/lifetime tests pass.

## Scoped native Files containers

A flat container owns Files navigator views and document groups within the
existing Workspace runtime, eight-pane/32-tab limits and navigator limits.
Splitting a document group preserves membership; moving a File tab preserves its
exact scope/path target. Closing a navigator detaches its groups rather than
closing unrelated Agent views. Legacy navigators share one launch container;
migration adds no panes and preserves geometry, IDs, focus and exhausted pane
allocators. Containers have their own checked ID allocator.

`ScopeId` identifies a requested human Files binding. The UI resolves it only
through an explicitly opened retained `WorkspaceFiles` capability. Saved IDs,
paths and bounded display labels create no grants. Additional folders open on
the existing bounded executor; saved external folders remain unavailable after
restart until the user explicitly reconnects them. A moved File tab can reconnect
its own scope even after its original navigator closed. A launch reconnect must
retain the original canonical launch root.

The UI owns the capability registry; Host retains it across Agent/Workspace
presentation transitions. Human Files/Git roots therefore stay independent of
an Agent resumed in another directory. Same-root review disclosure uses the
owner's lossless canonical current Agent-root projection, never a display label.
Unused external capabilities are revoked after their last container/File view
reference disappears; a failed layout addition releases its new capability.

File caches and all read/list/search/highlight work use scope plus relative
path, exact capability binding and view/container lifetime. A conservative
checked UI Files-work epoch invalidates pending work on ownership changes and
explicit restore/reconnect; focus, scrolling and geometry remain local. Completed
viewport/find state survives ordinary moves. Reconnect discards old plaintext,
render/find/selection state before replacement content can appear. Preview goes
to the invoking container's document group and preserves Explorer focus.

On Unix the existing root descriptor pins filesystem traversal. Windows scoped
roots must pin directories and ancestors with handles that deny delete sharing;
this prevents path replacement while allowing ordinary file edits. Directory
rename/delete is unavailable while those handles live. Windows source/target
checks and native Windows execution are separate gates.

Native layout v3 migrates consistent v1/v2 inputs only against the already
authorized launch scope. Mixed/unsupported versions and malformed ownership are
rejected before installation; root binding, no-follow/atomic publication and the
64 KiB limit remain. SQLite v33 and JSONL v1 are unchanged. Older readers reject
v3; returning to an older binary requires an explicitly selected compatible
layout or starting without saved layout.

Acceptance: two authorized folders containing the same relative path display
different content with independent view state; closed/rebound scopes reject
late read/search/highlight replies; split/move/preview/save/load retain identity,
limits and Explorer focus while Agent/root/draft/caret and effect admission stay
unchanged. Browser/Git container counterparts and docking remain later slices.

Source seams: `src/workspace/files.rs`, `src/workspace/ui/files_views.rs`,
`src/workspace/ui/async_work.rs`, `src/workspace/persistence.rs`,
`crates/aw-workspace/src/layout.rs`, `crates/aw-tools/src/browse.rs`,
`crates/aw-platform-win/src/scoped_fs.rs`.

## Native Git containers — implementation in progress

Reuse one flat container for navigator placement and document groups. Its
optional Files scope and per-navigator Git context map are independent resource
bindings. Mixed containers preserve old shared document groups and navigator
kind switching; explicit folder/repository opens normally create a single-kind
container. Preserve stable IDs, global limits, split/move/close rules and the
invoking container's remembered document group.

A Git context ID names an exact authorized checkout binding. A verified worktree
selection mints a new context and changes only the invoking navigator's map
entry. Another navigator, Files scope and the Agent remain unchanged. A moved
or still-open Diff retains its exact original Repository/Review/context;
`Target::Diff { review }` stays opaque. Saved IDs and bounded display labels
supply no grants. External contexts require explicit reconnect after restart;
launch discovery stays lazy and uses the original retained human launch root.

`git-open PATH` / Open Git repository… and `git-reconnect PATH` / Reconnect Git
repository… use the existing palette and bounded executor. Reconnect targets
the focused Git navigator. Carry origin container, navigator, exact binding and
checked request lifetime through every completion. A delayed review opens only
in that container's current remembered document group and preserves Explorer
focus. Closing, restoring or replacing its request binding rejects late replies.

Keep fresh exact-review/HEAD checks and the existing serialized owner before
index application. An owned index request rechecks its binding at owner
admission; reconnect, final-reference closure and restore invalidate queued
admission. An explicitly retained Diff may still stage its original checkout
when its snapshot is fresh. An already admitted external mutation is not
silently cancelled or replayed. Feedback still requires the canonical current
Agent root. Repository identity checks detect deterministic root/git-dir/common
directory replacement. Explicit Git-derived terminal launches revalidate their
captured binding on the executor before starting a shell. These checks do not
lock out concurrent external Git/filesystem writers or create a subprocess
sandbox.

Native layout v4 preserves the old Files wire fields and migrates consistent
v1/v2/v3 inputs. Legacy Git navigators join an existing document group owner
without changing Files bindings, geometry or pane IDs. Unowned groups need a
new container identity; if that allocator is exhausted, migration returns an
explicit limit error atomically, retaining the current layout and saved file.
Never reuse old IDs or add a special legacy routing mode. SQLite v33 and JSONL
v1 are unchanged; old binaries reject the new native layout version.

Stop when two Git containers inspect the same relative path in different
existing worktrees independently, delayed results stay with their origin,
and explicit fresh staging changes only the selected index. Stale reviews
cannot stage; Files bindings, Agent root, draft/caret and worktree HEADs remain
unchanged. Browser counterparts and docking remain later slices.

Parallel ownership: model/persistence, Git backend/context retention, and native
UI are separate implementation assignments. Agree container actions/lookups,
opaque Git context ID, binding epoch and completion identity before editing.
The coordinator owns shared plans and API reconciliation; keep owner/index
changes limited to exact queued-request admission.

Source seams: `src/workspace/git.rs`, `src/workspace/ui.rs`,
`crates/aw-git/src/lib.rs`, `src/workspace/owner.rs` and
`crates/aw-workspace/src/layout.rs`.

## References

- [Live Workspace behavior](../../docs/WORKSPACE.md)
- [Domain authority](../../docs/DOMAIN-MODEL.md)
- [Store reader and activation](../../../crates/aw-store/src/lib.rs)
- [Workspace descriptors](../../../crates/aw-workspace/src/layout.rs)
- [Native owner](../../../src/workspace/owner.rs)
- [Checklist specification](../TODO-SPEC.md)
- [Existing immutable publication helper](../../../crates/aw-agent-core/src/publication.rs)
- [User-requested left-right primitive](https://github.com/jonhoo/left-right)
- [Upstream publication and reader constraints](https://docs.rs/left-right/0.11.8/left_right/)
