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

Native layout v2 explicitly migrates consistent v1 layouts: existing Conversation
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
