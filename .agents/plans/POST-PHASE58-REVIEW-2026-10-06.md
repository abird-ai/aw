# Post-Phase-58 fresh architecture/correctness review — 2026-10-06

Status: unnumbered hardening review after immutable `phase-58`. This does **not** define Phase 59 and does not move any historical phase tag.

## Scope

Fresh review of bugs, regressions, correctness, responsiveness, terminal safety, async state machines, cache lifetimes, and maintainability after Phases 54–58.

SQLite remains v32. No provider/tool/routing/subagent/planner/guardian authority changes.

## Fixed findings

### 1. Recoverable background panic could dismantle the live terminal — high

Phase-56 UI background jobs intentionally use panic isolation, but the process panic hook ran before `catch_unwind` and restored terminal state for **every thread panic**. A caught syntax/file/Git/layout panic could therefore disable raw/paste/alternate-screen state while the UI continued running.

Fix:

- terminal lifecycle records the actual owner thread;
- panic restore is owner-thread scoped;
- intentionally isolated UI jobs use a thread-local `catch_isolated_panic` seam;
- only the wrapped job/on-panic closure suppresses the process-level panic diagnostic;
- worker-infrastructure panics outside that seam still reach the normal hook;
- restore ownership uses an internal `RESTORING_OWNER` state so a newer presenter cannot enter during old-generation cleanup.

### 2. Initial layout pending lifecycle was incomplete — medium

`schedule_initial_layout_load()` checked `layout_pending` but never set it after successful enqueue and `InitialLayout` did not clear it. Duplicate startup loads were therefore possible if the method was called again.

Fix: one real pending lifecycle, set only after successful submit and cleared on every result before stale-generation handling.

### 3. Manual async layout load could overwrite newer local intent — medium

Manual `layout-load` had no generation fence. A late loaded layout could replace a newer local layout.

Fix: manual layout results carry `base_generation`; stale loads are ignored with an explicit notice. Local split/tab/pane operations remain responsive while layout I/O runs; only another layout I/O command is serialized.

### 4. Rapid Files/palette search kept obsolete recursive traversal alive — medium

Stale search replies were ignored, but old rooted searches (up to 2,048 directories / 50,000 entries) kept running and consuming the 16-job preview budget/shared worker pool.

Fix:

- cancellable rooted file search;
- latest-query replacement cancels and removes prior search jobs immediately;
- palette close and Files→Git/Sessions navigator changes cancel obsolete search;
- closing a navigator pane cancels its search;
- cancelled search workers stop traversal cooperatively and skip stale reply publication.

### 5. Progressive syntax chunks could monopolize shared workers under result backpressure — medium

Every 128-line progress chunk used blocking send into a bounded UI result channel. Under a result storm, all shared workers could block trying to publish nonessential intermediate progress.

Fix: intermediate progress publication is nonblocking/best-effort; each job accumulates the complete styled output and emits one guaranteed final snapshot. Final correctness is unchanged while bulk progress cannot occupy all workers merely because the UI result lane is full.

### 6. Workspace Agent rich transcript projections could accumulate stale expensive jobs — medium

Resize/theme/turn churn could enqueue multiple rich shared-Agent transcript projections while an older projection was still running. Late results were ignored, but CPU work accumulated.

Fix: one rich transcript projection in flight at a time. Plain shared-Agent geometry remains immediately visible; when an old job settles, the next dirty frame schedules the newest key.

### 7. Closed tabs/panes leaked auxiliary state/work — low/medium

Closed tabs did not prune find state/generation/cancellation or selection state. Evicted Git reviews could leave diff-highlight caches, and closed navigator panes could leave recursive file search work alive.

Fix: centralized reconciliation prunes/cancels these states; diff-highlight cache is bounded to currently cached Git reviews.

### 8. Workspace taught the wrong mode command — low

Visible hint said `/inline for scrollback mode` even though `/inline` selects owned-primary Inline and `/scrollback` selects Scrollback.

Fix: Workspace now says `/agent return to Agent`; slash-command registry already contains correct specific modes.

### 9. Current docs contradicted current code — low but high maintenance risk

Current `WORKSPACE.md` still said legacy `status_line` normalized BelowComposer, current Inline slash popup was below composer, and Scrollback used one fixed persistent viewport.

Fix: current-authority docs now reflect AboveComposer + popup-above-composer + one persistent aw presenter with dynamic Ratatui scratch viewport. Historical Phase-51/54 text is explicitly marked/specified as superseded where appropriate.

### 10. Duplicate Workspace UI persistence call — low

Workspace return path called `host.apply_ui()` twice under the same condition.

Fix: removed the dead duplicate.

## Reviewed and intentionally retained

- Crossterm 0.28.1 saves the exact pre-raw termios and restores it in `disable_raw_mode`; no extra termios snapshot layer is needed.
- Git stage/unstage remains on the serialized owner worker because it mutates repository state; read-only Git views use the human UI executor.
- Ongoing PTY input/resize/terminate are bounded driver messages; terminal launch remains backgrounded.
- File/diff overscroll remains presentation-only and real-row identity is unchanged.
- Phase-58 dynamic Scrollback scratch reconstruction may issue CPR when required row count changes because Ratatui 0.30 exposes no in-place Inline-height setter; stable-height navigation does not reanchor.

## Remaining design debt / recommended next explicit work

### A. Decompose `src/workspace/ui.rs` — high maintenance priority

The controller is ~9.5k lines and still combines interaction reduction, command dispatch, async coordination, caches, hit testing, selection, and rendering. Supporting modules are good, but this file is the largest future-regression surface.

Recommended direction without changing behavior:

- `WorkspaceState` / resource caches;
- `InteractionReducer` (key/mouse/actions only);
- `WorkspaceCommands` / command-palette execution;
- `AsyncCoordinator` (jobs/results/generations only);
- `WorkspaceRenderer` + hit-map production;
- narrow typed view models between them.

Keep a single top-level `Ui` façade if useful, but move subsystem logic behind these seams.

### B. Agent-only config persistence is still synchronous — medium responsiveness priority

Phase 56 makes Workspace human I/O async by default, but Agent-only `Host::apply_ui` and profile persistence still call config save synchronously. Do not solve this with ad-hoc threads.

Recommended: move the generic bounded executor/service above `workspace` into a shared native TUI/application service, then use a coalescing typed config-persistence actor with explicit flush-on-exit semantics in both minimal/full Agent builds and Workspace.

### C. Shared Agent header lacks typed loaded-resource provenance — medium clarity priority

`AgentHeader::standard()` can truthfully say system prompt + no registered skills, but aw supports explicit instruction files and does not currently carry their source names into the UI projection.

Recommended: introduce a typed `LoadedResourceSummary` at runtime setup (context source display names + registered skills), carry it through Agent/Workspace projection, and let shared Agent chrome render exactly that. Do **not** scan the filesystem from render code or infer “loaded skills” from project files.

### D. Background executor class fairness could become explicit later — low/medium scalability priority

The current fixed pool is bounded and now avoids blocking on progressive chunks. If future indexing/LSP/watch workloads are added, introduce explicit latency/bulk classes or reserved capacity rather than growing queues or adding private pools.

## Review rule

Do not convert these remaining design debts into opportunistic broad refactors without an explicit numbered phase. The post-Phase-58 hardening fixes above are behavior-preserving correctness/responsiveness repairs; the deferred items change architecture/data plumbing and deserve isolated review boundaries.

## Validation evidence

Validated on the exact unnumbered hardening bytes before commit:

- dedicated post-Phase-58 hardening checker: **49/49**;
- Phase-58 checker: **38/38**;
- Phase-57 checker: **53/53**;
- Phase-56 checker: **98/98**;
- Phase-55 checker: **115/115**;
- Phase-54 checker: **135/135**;
- Workspace/Agent interaction checker: **254/254**;
- complete reference profile: **63/63**;
- full `aw` binary unit suite: **204/204**;
- exact whole workspace/all targets: green with trusted `AW_GIT`/`AW_BASH`;
- whole-workspace/all-targets Clippy with `-D warnings`: green;
- doctests and rustdoc with warnings-as-errors: green;
- warning-denied no-default / pipelines / routing / combined feature cuts: green;
- Phase-58 tall-terminal Scrollback tmux: **4/4**;
- Phase-57 natural-flow/clean-exit tmux: **6/6**;
- terminal lifecycle smoke: **8/8**;
- visible resize/provider tmux: **7/7**;
- Workspace PTY: **11/11**;
- Agent mode low-level PTY: **3/3**;
- live F6/F7 PTY: green;
- static source validation: **982 active files / 937 authored Rust test attributes / 727 local links / 0 errors / 0 warnings**;
- validation plan sizes after adding this review gate: **63 reference / 154 Rust / 157 full**.

No live-provider/billing/security audit claim is added. Pinned/writable-Nix proof remains environment-blocked as before.
