# Phase 56 — non-blocking native UI work graph and fast viewport scrolling

Status: authorized 2026-10-06 by the user after Phase 55 review.

## Boundary

Phase 56 changes **native human-UI scheduling, caching and viewport presentation only**. SQLite remains **v32**.

It must not change provider/account/tool/routing/subagent/planner/guardian authority, durable Session semantics, model context, billing/retry behavior, filesystem grants or browser authority.

The existing serialized Agent/owner worker remains authoritative for model/durable operations. Phase 56 does not route model/tool work through the UI background pool.

## Goal

The terminal UI thread becomes a small event/render reducer:

`input/event -> mutate local intent -> queue typed background work -> receive bounded result/progress -> update cache -> render`

No file/Git/process/layout/syntax operation may block input or paint.

New human-UI modules should become asynchronous by default by submitting work through one reusable bounded background executor rather than inventing another bespoke thread/task field.

## Shared background executor

Add a native Workspace background executor with:

- bounded queue;
- small fixed blocking worker set;
- cheap cloneable submit handle;
- named job classes for diagnostics;
- no unbounded thread-per-click behavior;
- panic isolation;
- shutdown by sender disconnect;
- explicit queue-full error rather than blocking the UI thread.

Use the executor for blocking/CPU-heavy **human UI** work:

- rooted Files list/read/search;
- Git inventory/review/compare/worktree selection;
- file/diff syntax highlighting;
- layout load/save;
- local terminal/process launch;
- future human preview/index operations.

The existing Agent owner worker, provider calls, durable write coordination and tool execution remain separate typed authorities.

## Typed async services

Each subsystem owns typed request/result data but receives the shared executor at construction.

A subsystem request must:

1. allocate a monotonically increasing generation/ticket;
2. update pending UI state immediately;
3. `try_submit` one bounded background closure;
4. return to the event loop without waiting;
5. deliver result/progress through async channels;
6. ignore stale generations/results after newer intent.

No subsystem may call filesystem/Git/process/highlighter work from `Ui::draw`, `Ui::key`, `Ui::mouse`, or synchronous response application.

## Progressive syntax highlighting

File content becomes visible immediately after bounded file read.

Highlighting is a second asynchronous progressive job:

- plain sanitized rows are renderable immediately;
- syntax worker retains parser/highlighter state across lines;
- emits bounded line chunks progressively;
- UI applies chunks by generation/path/theme;
- visible rows repaint as chunks arrive;
- stale theme/file generations are ignored;
- theme change invalidates/schedules progressive rehighlight without blocking input;
- unknown syntax remains plain;
- highlighting never performs filesystem/network/model work.

Diff highlighting follows the same progressive/cache path and must not recompute Syntect work inside `draw`.

## Viewport/scroll architecture

Replace ad-hoc integer scroll offsets with a reusable viewport state.

Input is immediate and local:

- mouse wheel and navigation keys only update desired viewport intent;
- no I/O or highlighting is awaited;
- wheel events coalesce naturally;
- render consumes cached rows;
- background cache misses may fill later without moving the viewport unexpectedly.

Scrolling should feel fast:

- wheel default step > historical 3 rows;
- PageUp/PageDown use most of the visible viewport;
- repeated scroll input may coalesce into one target;
- animation/ticks must never make input feel delayed;
- scroll state is bounded using saturating arithmetic.

All scrollable native regions use the same viewport semantics where compatible:

- Agent/read-only transcript;
- Files/Sessions/Git navigator lists;
- File/Diff/Help/Activity/main text;
- command palette;
- terminal scrollback where its PTY contract permits.

## File/Diff overscroll + scrolloff

Read-only File and Diff views use Neovim-like breathing room:

- allow bounded visual overscroll after EOF so the last real line need not be pinned to the bottom edge;
- optionally allow small head padding when navigation/search targets the first lines;
- target/jump/find rows maintain a configurable internal scrolloff (default 3 rows, reduced on tiny panes);
- max tail overscroll is bounded by a fraction of viewport height;
- overscroll rows are presentation-only blanks, never copied as source text;
- search/copy/source row identity remains tied to real lines.

Ordinary Explorer lists do **not** invent fake list items; they use fast/coalesced scrolling without source-row overscroll.

## Rendering/caches

Paint must be side-effect free with respect to external work.

`draw` may:

- read cached data;
- derive cheap visible geometry;
- render placeholders/progress;
- update hit rectangles.

`draw` must not:

- read files;
- run Git;
- spawn processes;
- save/load layout;
- execute Syntect over uncached source;
- wait for background work;
- mutate durable/model authority.

Expensive row projections should be cached by revision/theme/width or generated progressively when practical.

## Event-loop fairness

The async Workspace event loop must remain responsive under:

- large file highlight;
- Git status/review;
- file search;
- terminal launch;
- layout save/load;
- resize storms;
- rapid wheel input.

Background-result storms are bounded/coalesced so terminal input gets serviced between progress batches. Work completion sets dirty state; it never draws directly.

## Validation

At minimum:

- one shared background executor exists and Files/Git/syntax/layout/terminal launch use it;
- no `thread::spawn`, `spawn_blocking`, filesystem/Git/process/highlighting call remains in Workspace UI event/render code except centralized workers;
- bounded queue never blocks UI input;
- queue-full path is visible/retryable;
- stale file/Git/highlight generations are ignored;
- file read displays plain text before syntax highlight completion;
- progressive syntax emits more than one chunk for a large Rust fixture;
- theme change schedules new generation without synchronous highlight;
- diff draw performs no syntax work and uses progressive cache;
- rapid scroll input performs no background/external call;
- wheel scrolling is faster than historical 3-row step;
- File/Diff permit bounded EOF overscroll and preserve real-line search/copy identity;
- scrolloff keeps a jumped/search row away from viewport edges when possible;
- Explorer/list scrolling remains real-row-only;
- terminal/layout launch/save/load complete through the shared background executor;
- UI remains interactive while a deliberately slow background fixture runs;
- existing Agent/Workspace terminal lifecycle and Phase-55 shared chrome remain green;
- Phase-55/54/53/52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts and actual PTY smokes remain green.

## Deferred

- async rewrite of model/durable owner authority (already isolated and intentionally serialized);
- browser UI worker parity;
- virtualized multi-gigabyte file editing;
- semantic LSP indexing;
- filesystem watchers/background recursive indexing;
- automatic Git fetch;
- scheduling/autonomy/execution authority changes.
