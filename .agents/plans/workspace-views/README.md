Author: GPT-6 (Codex)

# Read-only Agent views and composable Files/Git

Read-only Agent tabs, scoped Files and native Git containers let people inspect
independent resources inside one Workspace without changing the current Agent.
The native slices preserve independent view state, exact authority and one
execution owner. Browser read-only Agent tabs use the same strict reader; scoped
browser Files/Git containers use exact launch-root and verified-worktree
capabilities. Docking remains planned.

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

Native containers use a flat stable-ID graph, one parent and one container level,
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
- The next left-right redesign pass first measures the browser Session snapshot
  (`src/web.rs`) and bounded multiplayer presence projection
  (`src/web/team/multiplayer.rs`). Reuse the existing publication helper; compare
  long-lived readers with cold registration, read-heavy and write-heavy ratios,
  snapshot sizes and held-Arc retention. Record reader/publisher latency, CPU,
  allocations and peak bytes; migrate only for an end-to-end improvement with
  fresh authority, sequence and presence TTL checks preserved. Never put one
  registered Reader behind another shared Mutex. Keep admission/receipt/State,
  PTY/lease/queue locks and single-owner caches until measurements justify a
  different design.
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

The initial scoped Files layout v3 migrated consistent v1/v2 inputs only against the already
authorized launch scope. Mixed/unsupported versions and malformed ownership are
rejected before installation; root binding, no-follow/atomic publication and the
64 KiB limit remain. SQLite v33 and JSONL v1 are unchanged. Older readers reject
v3; returning to an older binary requires an explicitly selected compatible
layout or starting without saved layout.

Acceptance: two authorized folders containing the same relative path display
different content with independent view state; closed/rebound scopes reject
late read/search/highlight replies; split/move/preview/save/load retain identity,
limits and Explorer focus while Agent/root/draft/caret and effect admission stay
unchanged. Docking remains a later slice.

Source seams: `src/workspace/files.rs`, `src/workspace/ui/files_views.rs`,
`src/workspace/ui/async_work.rs`, `src/workspace/persistence.rs`,
`crates/aw-workspace/src/layout.rs`, `crates/aw-tools/src/browse.rs`,
`crates/aw-platform-win/src/scoped_fs.rs`.

## Native Git containers

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
unchanged. Docking remains a later slice.

Parallel ownership: model/persistence, Git backend/context retention, and native
UI are separate implementation assignments. Agree container actions/lookups,
opaque Git context ID, binding epoch and completion identity before editing.
The coordinator owns shared plans and API reconciliation; keep owner/index
changes limited to exact queued-request admission.

Source seams: `src/workspace/git.rs`, `src/workspace/ui.rs`,
`crates/aw-git/src/lib.rs`, `src/workspace/owner.rs` and
`crates/aw-workspace/src/layout.rs`.

## Browser read-only Agent views

Start in the shipping Solid Explorer/tab shell (`web/src/main.jsx`), whose
active document tabs are memory-only. The separate historical browser v1
four-group reducer is not the shipping shell; do not treat its Node coverage as
implementation of these views or share native serialized layout prematurely.

Reuse the exact-ID Store branch reader and native reader authority/privacy
policy through an authenticated read-only endpoint. A request names exact
Store/Session/branch identities, never a database path or an activation action.
Keep existing Session activation separate; browsing creates no Agent/Session,
execution binding, model/tool call or durable effect intent.

LocalWeb retains one owner Store from startup and reuses it for browsing,
activation, provider custody and later Session creation. Reads never call the
effectful initializer or reopen State. Durable receipt lookup precedes creation,
so retrying an accepted unsaved request cannot create another Session or call a
revoked provider. Memory-only owners retain their receipt-first semantics.
Reuse the existing route boundary's host/origin/paired-browser authentication,
then independently check exact Store identity and trusted actor Session/branch
policy on every read. Browser authentication supplies no Store authority.

Each tab owns bounded content, checked request lifetime, Find, selection and
viewport. Revalidate permission on refresh even when the branch head is
unchanged; close/logout/revocation clears protected content and derived caches.
Use existing browser authentication and capability APIs, keep descriptors
memory-only and do not grant native local-folder authority to browser paths.

Stop when two real SQLite branches coexist with independent Find/scroll in the
shipping browser; held replies cannot reopen closed tabs, revocation clears
unchanged-head content, and current Agent/draft/caret/execution counts remain
unchanged. Execute actual Rust-owner HTTP tests and real-browser acceptance,
separately from mocked transport/Node tests.

Parallel ownership: authenticated backend reader/HTTP tests, pure browser
view-state module/Node tests, and Solid integration/editor lifecycle/browser
acceptance. The coordinator reconciles shared contracts, bundle and product
gates. Scoped browser Files/Git are described below; docking remains a
subsequent slice.

Source seams: `web/src/main.jsx`, `crates/aw-surfaces/src/lib.rs`,
`crates/aw-surfaces/src/contract.rs`, `crates/aw-store/src/lib.rs`,
`src/sessions/branch_view.rs`, its native reexport and `src/web.rs` (session routes, authenticated
boundary and LocalWeb persistence ownership).

## Scoped browser Files/Git containers

Use the shipping Solid Explorer and document strip, with memory-only flat
containers. Files and Git keep the existing category buttons; each category
selects an independent navigator through a view selector and explicit New/Close
view controls. Retain at most eight containers and 32 total document tabs,
including the permanent current Agent and its bounded read-only pins. Closing
a browser view releases only that view's state, never another browser's owner
resources. Docking and shared serialized layouts follow separately.

Retain the owner's explicitly granted launch `WorkspaceFiles` and physical
directory binding once. Files reads remain under that launch capability.
Git may select verified existing worktrees through opaque inventory handles;
that grants no Files capability to the selected path. Another Files root needs
an explicit trusted-owner grant contract. Browser paths, labels, native scope
IDs and cached descriptors never grant server-folder access.

Add a strict read-only scoped wrapper to the existing workspace action API.
Requests and replies carry the SurfaceHost lifetime and immutable scope ID;
Git opens additionally name the exact originating inventory and item. Context
IDs never retarget an old binding by pathname. Keep existing SDK limits,
retained-root and Repository identity checks, serialized resource admission,
four bounded HTTP workers and pre/post-operation authentication. Legacy action
clients remain compatible; scoped requests cannot wrap terminal or mutation
commands. No SQL/JSONL/version change is needed.

Each navigator retains tree/query/mode/context independently. File identity
includes owner source, Files scope and relative path; a Diff retains its exact
original scope and immutable Review. Stable document IDs distinguish separate
views of the same resource. Reads allocate their destination before awaiting;
late replies never select another tab or populate a closed/rebound view.
Preserve per-view editor Find/viewport/selection and clear plaintext and derived
editor state on denied scope, source replacement or logout. File/Diff content
is an explicit read snapshot; refresh revalidates its binding. This does not
inherit the Agent reader's separate fifteen-second membership freshness rule.

Acceptance: duplicate launch-root Files views and two existing Git worktrees
retain independent navigator/editor state. Forged scopes, physical binding
replacement and held close/refresh/source replies fail without redirecting a
read. The same Agent/root/draft/caret, Git heads/indexes and execution counts
remain unchanged in actual Rust HTTP and immutable-package Chrome acceptance.

## Publication screening and disposition — 2026-10-17

A reproducible, source-controlled **release-mode candidate benchmark** is now
available at `crates/aw-agent-core/examples/publication_measure.rs`:

```sh
cargo run --locked -p aw-agent-core --release --example publication_measure -- --held
# --quick selects small Session and presence cases with 1/4 readers;
# omit --held to compare without an owned historical snapshot.
```

It uses the existing `aw_agent_core::publication` implementation (not an
imitated lock-free structure), a cloned `Mutex` Session read against an
immutable Arc publication read, and the **actual** presence read shape:
prune/filter/project rows under its Mutex rather than clone the whole map.
The matrix covers Session payloads 32 KiB / 256 KiB / 4 MiB, presence 8 / 128
rows, one/four/eight registered readers, and write/read ratios 1:4, 1:16,
4:1. Output is one JSON object per case (90 cases with paired strategies),
with wall time, reader/writer p50/p95, cold reader registration time, Linux
process CPU *ticks*, payload-clone allocations and estimated peak live payload
bytes. Held-old-snapshot cases check reclamation when consumers retain Arc
ownership across publication.

Medians from **three complete optimized 90-case runs** in the pinned
2026-10-17 Nix developer shell, expressed as **Mutex wall time →
publication wall time** for the same isolated work:

| Workload, eight readers | 1:16 write/read | 4:1 write/read |
| --- | --- | --- |
| 256 KiB Session | 3.58 → 0.36 ms (9.8× faster) | 3.76 → 10.28 ms (2.7× slower) |
| 4 MiB Session | 26.78 → 5.47 ms (4.9× faster) | 28.96 → 117.56 ms (4.1× slower) |
| 8-row presence | 3.73 → 0.75 ms (5.0× faster) | 5.24 → 14.67 ms (2.8× slower) |
| 128-row presence | 38.58 → 7.18 ms (5.4× faster) | 42.48 → 230.36 ms (5.4× slower) |

This is **microbenchmark screening, not a production improvement claim**:
the Session payload is representative bytes rather than real `Snapshot`
JSON; real `/api/session` separately waits for exact authorized Store
recovery hints and current account visibility, and SSE subscribes before
its high-water snapshot. The hot `Session::delta` path appends per-token
while holding the existing mutable State mutex; replacing it with full
per-token immutable payload cloning is potentially quadratic in growing
answer length. Presence list has fresh Store authorization and mandatory
30-second TTL semantics; every publication must keep grants, expiry,
bounds and request lifetimes independent of the cache. There is no valid
single shared Reader behind a Mutex. Timings are short, sensitive to host
scheduling and do not include JSON encoding, transport, hardware counters
or a realistic service arrival distribution. The estimated allocation
count/peak records payload clones only, not the complete allocator heap;
CPU tick resolution is too coarse for the shortest scenarios.

**Decision: do not migrate either production Mutex yet.** Read-heavy
throughput improvements can be substantial, but the 4:1 loss, additional
retained payloads and current per-delta mutation lifecycle mean no
measured end-to-end performance win has been established. Retain one
durable `SessionState` authority and the bounded presence Mutex; avoid
a second truth/cache and new synchronization invariants. The authenticated
HTTP/SSE and presence measurements below strengthen this decision; a future
candidate must demonstrate an *end-to-end* improvement with fresh authority,
sequence, TTL and exact effect admission preserved. A previously published
snapshot never supplies permission.

### Authenticated HTTP and real presence — signed source `2c0742506bb9e2620c4152585021262c3c7345b5`

Opt-in benchmark tests on the actual shipping Axum routes, not a replacement
left-right implementation:

```sh
cargo test --locked -p aw-cli --lib shipping_session_http_sse_publication_measurement -- --ignored --nocapture
cargo test --locked -p aw-cli --lib shipping_presence_http_publication_measurement -- --ignored --nocapture
```

Each test creates its own numeric-loopback listener and *synthetic* owner,
with real HTTP framing/JSON, pairing-cookie authentication, unchanged
same-origin/Host policy, bounded concurrency and current permission checks.
The Session test exercises initial SSE high-water snapshots and repeated
`/api/session` reads alongside synthetic **in-memory** per-delta writes; its
4 MiB payload is a stress input, not a claim that normal output is that size.
The presence test uses the real invite-only team router, SQLite-backed draft
grant/epoch validation, the maximum 128-row memory cap, 30-second TTL, fresh
authorization and actual cursor POST/list endpoints. Fake pending display
and synthetic presence rows cannot call a paid provider, use tools, activate
a Session or grant a file or cross-machine capability.

Three completed runs in the pinned shell, medians of **eight-reader, 1:16
write/read read-p95** (one worker per authenticated HTTP client):

| Actual handler, current Mutex | Median read-p95 | Additional measured boundary |
| --- | ---: | --- |
| Session JSON, 32 KiB pending | 7.76 ms | initial SSE 2.12 ms |
| Session JSON, 256 KiB pending | 43.18 ms | initial SSE 12.22 ms |
| Session JSON, 4 MiB pending | 1,027.42 ms | initial SSE 374.13 ms |
| Presence list, 8 rows | 6.94 ms | actual Store grant + TTL + JSON |
| Presence list, 128 rows | 19.18 ms | same Store/grant path, full cap |

For 128-row presence with eight readers and **4:1 writes/read**, measured
read-p95 was **19.01 ms** and total sampled wall time was **694.35 ms**:
writes still matter. Real read latencies include serialization, IPC/Store
work, scheduler and loopback transport; CPU ticks, allocation/retention and
cold-registration detail remain in the earlier isolated benchmark. These
are synthetic offered loads with limited concurrent client connections,
not observed field traffic, production p95 SLOs, or a direct HTTP comparison
against a migrated implementation.

**Conclusion:** The measurable publication microbenchmark wins under
read-heavy contention do not justify cloning the growing full Session
snapshot for each delta, or introducing a second presence authority with
extra allocation/TTL/grant synchronization. Keep both original locks.
Revisit only if production-like request/refresh frequency and a bounded
candidate read projection show a repeatable end-to-end win **and** fresh
authorization and source epochs are rechecked at admission.

The review also uncovered and fixed a privacy/correctness regression.
The Store's `set_draft_mode` and `set_draft_grant` **each abandon** the
prior live draft generation after an audience/policy revision. The original
presence cache was allowed to retain old participant/cursor rows until
the 30-second TTL. The new scoped cleanup removes every cursor for the
affected Session on successful policy/audience change, without touching a
different Session. A checked cache admission epoch rejects an in-flight
POST that completed Store authorization before revocation but arrives at
the map afterwards. Both publish and list now refuse to present an
abandoned draft as live presence; a successful re-grant cannot resurrect
the previous generation. Focused tests cover revoke, re-grant, mode Off,
old-generation POST/list, an actual held authorized POST and cross-Session
isolation. Existing presence POSTs now return an explicit `409` for an abandoned or stale generation; route names, JSONL/SQLite versions, active owner and model effects are unchanged.

### Native acceptance added during the same screening

- The real shipping `Ui::draw` 90-frame TestBackend gallery passed on the
  starting HEAD with the pinned Nix developer environment: Dark/Light/System,
  80×24/100×30/120×40/160×45/200×60 and six scenes including split,
  palette, tabs overflow and stale terminal. The fixtures guarantee zero
  provider/PTY attachment; they are **not** real-terminal proof.
- A separate real PTY drove an existing immutable `aw 0.15.0` binary with a
  temporary fake `openai` endpoint at `127.0.0.1:9`, explicit `--tools-mode
  off`, synthetic private State/root/layout and isolated HOME/XDG dirs.
  Native startup, F1 palette, 80×24 → 120×30 resize, `quit-discard` clean
  exit code 0, byte-equal termios and alternate-screen restoration passed.
- A second isolated real PTY exercised explicit `terminal-start`, a live
  child shell receiving its own Ctrl+C, then `quit-all` with child
  termination. The owner exited 0 and restored terminal modes and alternate
  screen; all six SQLite Session/run/effect counters were zero. This was
  **not** a test of remote agents, paid providers, Windows, compositor or
  actual user data.
- A third real PTY drove 80×24 → **65×20**, Ctrl+K Z maximize and restore
  (both redrew, Sessions header repainted on restore), F1 modal close/reopen,
  then 120×30 resize and explicit clean quit; it exited 0 with restored
  terminal state and six zero State counters. Real split/modal-focus detail,
  broader terminal capability combinations and docking are still open;
  do not close the combined item from these bounded smoke tests.

## Docking — bounded same-window reparenting

First implemented slice: signed source `e06ffa7007997310bac2c5e5c14938fde92dc320`.
It is an actual user-facing **same-browser-window** resource move, not a
cross-process docking transport.

- `web/src/workspace-views.js` changes only `document.containerId` under
  the existing `createWorkspaceViews` owner. The document's exact
  `source_id/scope_id`, File path or immutable Git Review ID and retained
  original inventory/item, DOM identity, cached read snapshot and in-flight
  tokens stay unchanged. No fetch, grant, new Document ID or new execution
  path is created by the move.
- A destination must be an existing, currently available navigator of the
  matching kind (Files→Files or Diff→Git). An existing document of the same
  exact resource at the destination is an explicit conflict; the controller
  never silently merges or closes either view. Git documents may move to
  another Git navigator inspecting a *different* worktree, but still refresh
  through their original immutable Review and original checkout scope.
- Container closure now follows the document's current owning container.
  Moving before a pending read finishes retains that read when the original
  container closes. Closing the destination cancels/scrubs its children and
  rejects late replies. An unavailable or replaced source cannot be revived
  by moving a display descriptor.
- The shipping Solid File/Diff source bar offers a compact, accessible
  destination selector **only when at least two compatible views exist**.
  Changing the owner updates the matching Explorer selector but preserves
  active document focus, unsent composer and CodeMirror instance. It adds
  no duplicate global toolbar/header; 390 px and theme rules are retained.
- Three shipping-controller Node regressions cover pending reparenting,
  duplicate/mismatched/closed denial, and a Git Diff moved across dissimilar
  checkout contexts. Actual Chrome 155 via DevTools Protocol exercised
  two private Files containers at **390×844 Dark**, a real rooted File,
  move, original container closure and selector disappearance: CodeMirror
  instance, draft/caret, original read count and 390 px document width
  were preserved; zero uncaught JS exceptions and six zero SQLite
  Session/run/effect counters. Chrome used a fake endpoint and tools Off;
  these are bounded browser acceptance, not native/cross-window proof.

### Cross-window docking contract — File and immutable Review baseline delivered

Signed File-only browser implementation: `7de865852df747d0c3280c96180ef5dacbc6c8f2`.
The original authenticated Rust web owner now retains only a bounded,
**ephemeral presentation correlation** in `src/web/dock.rs` (24 windows,
24 tickets, 180-second window expiry and 90-second File ticket). A sender
creates a fresh cryptographic window proof and File request; a second browser
window separately registers, inspects the ticket and **independently** reads
the exact retained File via the existing scoped `SurfaceHost` before
acknowledging its own renderer. The origin closes its File tab **only after**
the owner verifies acceptance and authorizes Release. The ticket and browser
fragment contain no document bytes, provider state, tool receipt, root grant,
or execution authority; the receiver erases its fragment before disclosure.

The owner checks same-origin/browser auth and the exact Session instance,
source, scope, and original physical root on preparation, inspection,
acceptance and final release. It checks receiver identity, competing
acceptances, cancellation, expiry, a known receiver decline and late
stale/duplicate requests; acceptance is not replayed. The root/contents
are revalidated before an origin tab may be released. An accepted receiver
window whose **registration** expired is not considered live, but a
time-bounded registration is *not* a guarantee its OS/browser process
remains open at every instant; crash ambiguity must stay explicit. An
unknown acceptance response retains an authorized read-only receiver
for inspection without claiming success; confirmed denial scrubs it.
The native Agent, unsent draft, PTY, Git state, SQLite/JSONL and all effects
remain unaffected.

Evidence: seven authenticated owner-route tests; 155 browser-model
regressions; actual two-page Chrome 155 / private LocalWeb where receiver
reloads the File **after the original page closes**, 390×844 Dark geometry
holds, and all six SQLite effect counters remain zero. The source-HEAD
product gates and byte-identical eight-asset Vite rebuild are recorded in
`TODO.md`. At the original File-only checkpoint this proved a bounded
**File presentation move**; the later `41e66c4` checkpoint below adds
bounded CodeMirror continuity. Remote-device authentication and native
terminal docking remain separate.

#### Immutable Git Review extension — signed `00559eb070cc07bd96956f8d6b23fe9394202ca5`

The same owner registry now accepts a separate, strictly validated
`/api/dock/prepare-review` request for an **already retained** immutable
Review. Its shared typed resource is File(path) or Review(review_id); the
original strict File request schema remains unchanged. A Review's source
and **exact verified Git worktree scope** are rechecked through
`SurfaceHost::execute_if` on preparation, receiver inspection/acceptance
and origin release, including after an independent physical checkout
replacement. A stale or evicted Review ID is denied; the owner never
replays a Git inventory selection or grants a new Files root.

The browser receiver opens one scoped read-only Review document via
`openRetainedReview`, using the original `source_id/scope_id/review_id`.
It does **not** register the linked Git scope in its navigator scope
catalog. Its ordinary navigator remains launch-root based, while the
Diff labels and later refresh continue to use the original immutable
Review and its source checkout. The server ticket discloses only the
bounded Review identifier, never diff contents, Git credentials or a
new worktree grant. Original Agent/composer, saved State, PTYs, Git
checkout/index, and all model/tool effect paths are unchanged.

Verified after the signed source commit: 11 authenticated Owner docking
tests (including non-root linked scopes, evictions, stale/mismatched IDs,
root/worktree replacement, File regression and single-acceptance races),
161 Node model tests, 132 web-only Rust, 1,285 other Rust, 201 Store
library and 156 Store integration tests. Strict formatting/Clippy,
Workspace-only/minimal feature builds and all eight byte-identical
embedded frontend assets passed. Actual Chrome 155 with two distinct
pages independently rendered and refreshed both the new Git Diff and
existing File after closing their original pages; 390×844 Dark
display remained within viewport, fixture HEAD/index stayed identical,
and six SQLite execution-effect counters remained zero. Live two-page
Chrome covered the launch-root Git Review; non-root scope was
verified with actual HTTP and frontend model tests, not claimed as
a separate live non-root screenshot.

#### Bounded CodeMirror presentation continuity — signed `41e66c4ae115c48b450166fc5feec7c57c1fcec1`

The read-only File and immutable Review handoff now includes an **optional,
strictly validated version-1 presentation** in the same short-lived,
owner-held ticket: SHA-256 fingerprint, Find query (maximum 512 UTF-16
units/1024 UTF-8 bytes), selected match, up to two panes' selection and
viewport offsets, and an explicit Find/editor/none focus hint. No
document plaintext, DOM object, serialized editor, provider secret,
Git write capability, PTY or active Agent is carried in this metadata.
A Find query is user-entered text, so it is transported only within
the existing authenticated ticket, never in the URL fragment, console,
persisted layout, browser storage or a cross-window message.

The origin captures the active read-only CodeMirror presentation during
the direct popout gesture (or keyboard activation), with bounded gesture
lifetime and cancellation cleanup. The receiver authenticates and performs
its **own** exact File/Review read, verifies the SHA-256 checksum locally,
mounts CodeMirror, restores Find/selection/viewport/focus, and only then
acknowledges admission. For Files, the Rust owner independently verifies
the original authorized content fingerprint at prepare, inspect,
accept and release; changed content prevents the origin from releasing.
For Reviews, immutable retained Review ID and exact Git scope remain
authoritative, with independent receiver content comparison and existing
owner reauthorization. A checksum is a consistency guard, **not** an
access grant or replacement for resource identity.

A read-only CodeMirror editor is now keyboard focusable without becoming
editable. When a new viewport differs, pixel scroll may clamp; the
Review's paired panes may synchronize their vertical offsets. Exact
selection/text positions are checked against the independently loaded
document before restoration. An unavailable/different owner, stale
document or failed browser restore does not authorize a new window
to acquire execution ownership.

**Evidence:** On the exact signed source HEAD, 14 authenticated docking
tests, 166 Node tests (nine suites), 132 web-only Rust tests, 1,288 other
Rust tests, 201 Store library tests and 156 Store integration tests all
passed. Formatting, strict Clippy, Workspace-only/minimal feature builds
and a byte-identical rebuild of all eight browser assets passed in the
pinned Nix shell. Four real Chrome 155 two-page transfers (File/Review ×
Find/editor focus) passed at 390×844 Dark, including query/current match,
selection, focus, viewport, origin-close/receiver-refresh and zero JS/
execution effects with unchanged synthetic Git HEAD/index. These were
private synthetic fixtures, not live personal data or paid requests.
The complete Rust test coverage was executed as **split packages**, not
claimed as one uninterrupted monolithic Cargo command.

**Still open:** broader multi-window crash/restart, same-ID ABA,
paired/remote-browser authorization, ambiguous ACK and full lifecycle
acceptance; native second-terminal/PTY control and restoration.
An accepted ticket cannot prove the receiver process is continuously
live. No SQL/JSONL/native-layout migration, provider permission or Git
write operation is introduced.

#### Short-lived receiver-confirmed release and tab retirement — signed `e5589196e88ada25250b97d5fca3d693f23c2415`

A later lifecycle review found that a **180-second registered window** is not
sufficient evidence the browser is still present when the original window
releases its view. On the same short-lived, authenticated File/Review
ticket, the recipient now first Accepts an independently read/rendered
resource. The *origin* then initiates a fresh `Confirm` observation,
and only that exact accepted receiver may acknowledge via
`/api/dock/heartbeat` using its original window identity and proof.
The Rust owner admits source Release only when this post-confirmation
acknowledgment is **at most three seconds old**, the original window
and receiver are still within the owner's expiry bounds, the exact
original File/Review remains readable, and all existing grant, source,
scope, owner-epoch and single-receiver checks still pass.
Pre-confirmation pings and other registered windows cannot supply the
acknowledgment. Acknowledge/confirm does **not** grant access to another
File, Session, Git checkout, tool, PTY or provider.

The receiving page also issues a *best-effort* same-origin, JSON,
`keepalive` retirement on `pagehide`. The owner invalidates that
window's proof and prevents further heartbeat/release; when the browser
omits the retirement packet, missing fresh confirmation still blocks
origin release. A receiver reload never restores the in-memory
window proof or URL fragment and cannot automatically resurrect an old
ticket. A lost Release response is **unknown**: the sender keeps its
read-only original view rather than claiming success; any independently
authorized receiving view remains inspectable, never promoted to a
second Runtime or execution owner.

**Verification:** On signed `e558919`, 20 real authenticated Axum
docking tests and 169 Node tests cover accepted-but-unconfirmed receipt,
wrong/fresh-window proofs, expiry and renewed proof, concurrent retire
and Accept, original-owner restart, capacity limits, and unknown HTTP
outcomes. Rust validation passed as 1,294 non-Store tests (four ignored),
201 Store library tests and 156 Store integrations, with web-only 132,
strict library Clippy, formatting, Workspace-only/minimal features, and
an eight-asset byte-for-byte frontend rebuild. Real Chrome 155 File
and Git Review transfers, subsequent receiver reload and early File
popup closure passed with 390px Dark geometry, unmodified synthetic
Git HEAD/index and zero State/run/effect counters. The browser fixture
does not simulate a receiver crashing at every submillisecond point of
the post-ACK/Release race.

**Limits:** A three-second acknowledgment is a bounded *recent
observation*, not a durable lease or guarantee that a browser cannot
crash immediately afterwards. Full independent multi-profile/paired
account handling is not supplied by the existing one-time pairing
model. Network partitions, crashes immediately after ACK, and native
PTY/Runtime handoff still require separate acceptance and design.
No persistent handoff schema or authority was added.

A second browser/native window has a **different presentation lifetime**;
a string ID, URL, browser message or persisted layout may only request a view,
never grant Store/File/Git/Session/PTY or model ownership. Do not use
`window.open` + `postMessage` as a substitute for authority transfer.
Do not send source contents, credentials, old Tool receipts, or PTY handles
in a browser message.

1. Define an owner-approved, single-use **Prepared → Accepted → Released**
   handoff with source-window lifetime, destination-window identity,
   bounded expiry, exact Store/source epoch, original resource and distinct
   logical view identity. Receiver must authenticate to the same owner and
   independently reauthorize the source/scope before exposing content. A
   handoff token is a correlation key, not a root/tool/provider grant.
2. Keep the original view visible/owned until a trusted receiver acceptance
   is committed. An old origin that closes early cannot revoke another
   participant's legitimate view or silently authorize an unconfirmed move;
   ambiguity remains inspect-only or asks the human to reopen. No replay
   of active effects or implicit Agent activation is ever allowed.
3. Transfer bounded presentation metadata (Find/viewport/selection/tab
   placement), **not** serialized plaintext snapshots or a second
   cache/Provider/PTY owner. Across processes the receiver creates a fresh
   local renderer only after read authorization; source removes its old
   logical view only on an accepted handoff. Closing either window cancels
   its own pending work, never an unrelated source.
4. Measure and verify simultaneous moves, same-ID ABA, lost/delayed/duplicate
   ACKs, wrong/missing/revoked owner, source-epoch replacement, stale
   branch/Review/scope, origin/receiver crash, re-open and limits. Native
   cross-terminal/PTY attach must have its **own** explicit original Runtime
   handle and terminal restoration proof, not inferred from browser moves.

The **read-only File and immutable Git Review** ticket/API, browser popup
and bounded Find/scroll/selection/focus continuity are implemented.
Native cross-terminal docking and **automatically reopening** a transferred
view after receiver restart are not implemented. The fresh receiver-confirmation
gate and origin-preserving crash/reload denials are verified, but independent
paired-account browser admission, arbitrary partitions, crash exactly after
ACK and ambiguous transport outcomes need further acceptance. No serialized
layout migration was introduced: version **0.15.0**, SQLite **33**, JSONL
**1**, native saved layout **v4**, and all original runtime and resource
authority boundaries remain unchanged.

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
