# Post-Phase-58 Workspace UI ownership decomposition — 2026-10-07

Status: implemented and validated as **unnumbered hardening above Phase 58**.

Implementation checkpoint:

```text
65905ad  Decompose Workspace UI ownership seams
```

Numbered product checkpoint remains:

```text
phase-58 -> 5092c3fd7d41773acca0a87f1abc0c889bd2d20b
```

SQLite schema remains **v32**. This work is **not Phase 59**, does not move a historical tag, and does not add provider/tool/account/routing/governance authority.

## Why this hardening exists

The fresh review correctly identified that `src/workspace/ui.rs` still concentrated too many unrelated responsibilities even after the first typed Workspace seams were introduced.

Before this repair it was about 9,978 lines and mixed:

- core Workspace layout/state façade;
- human command execution;
- Agent composer/input reduction;
- mouse/search/palette interaction;
- file/search/highlight background coordination;
- conversation/file/diff presentation;
- renderer and hit geometry;
- large behavior regression suites.

The problem was not line count by itself. The risk was that unrelated responsibilities could drift together, recreate parallel behavior, or make future changes harder to review.

A broad rewrite would have been the wrong response. The existing `Ui` state, bounded background executor, owner thread, shared Agent composer, typed palette/session/tab seams and rendering behavior were already correct and heavily tested.

## Adopted design

Keep **one Workspace `Ui` state machine** and partition its implementation by responsibility.

```text
workspace::ui::Ui
├── ui.rs
│   ├── data/view-model types
│   ├── construction and application façade
│   ├── core layout/navigation state
│   ├── palette/session projections
│   └── shared helpers + regression tests
├── ui/commands.rs
│   └── resolved human command execution
├── ui/interaction.rs
│   └── keyboard/mouse/search/palette/composer/viewport interaction
├── ui/async_work.rs
│   └── bounded Files/search/highlight/background request + result coordination
└── ui/render.rs
    └── presentation projection, drawing and hit geometry
```

These files are implementation partitions of the **same Rust type**, not independent controllers.

They intentionally use the parent `Ui` state and existing typed services. There is no second Workspace state store, no second Agent composer, no private worker pool, no second command registry, and no new authority boundary.

## Command seam

`src/workspace/ui/commands.rs` owns the effectful human-command reducer, including:

- `send_draft`;
- `command`;
- `extra_command`;
- `execute_palette_item`;
- mode handoff requests and the existing typed command dispatch.

The slash vocabulary remains sourced from `src/tui/commands.rs`.

The Workspace command seam does not create a competing parser or command registry. Aliases, slash semantics, owner commands, Git actions, terminal actions and layout actions retain their existing typed paths.

## Interaction seam

`src/workspace/ui/interaction.rs` owns human input state transitions, including:

- `key`;
- `mouse`;
- palette open/close/key interaction;
- incremental find/search interaction;
- prompt-history projection;
- the shared Agent composer call site;
- viewport scrolling/jump/home/end behavior;
- navigator activation and paging.

The embedded Agent continues to call the shared:

```text
tui::composer::Interaction
```

for composer editing/history/slash behavior.

Focused child PTYs continue to own ordinary terminal keys. This partition does not become a terminal protocol or Agent execution engine.

## Async coordination seam

`src/workspace/ui/async_work.rs` owns bounded human-UI background coordination, including:

- rooted file list/read/search request plumbing;
- latest-intent search cancellation;
- file/diff progressive highlighting;
- theme-driven rehighlight scheduling;
- layout/terminal/highlight/find/conversation background result application;
- visible-file demand loading.

It continues to use the existing shared bounded Workspace background executor.

It does **not** own provider dispatch, durable Agent execution, tool authority, Session mutation authority, or a private thread pool.

## Render seam

`src/workspace/ui/render.rs` owns presentation/projection behavior, including:

- conversation/file/diff visible-row projection;
- shared Agent projection;
- render-cache interaction;
- read-only text selection paint/copy projection;
- `draw`;
- `draw_pane`;
- `draw_palette`;
- exact tab/hit geometry through the existing `tab_strip` seam;
- demand-driven presentation scheduling.

Rendering remains presentation-only. It does not grant filesystem/model/tool authority or mutate durable conversation semantics.

## Façade that remains in ui.rs

The parent `src/workspace/ui.rs` intentionally remains the cohesive application façade for:

- `Ui` and local view-model/data types;
- construction;
- terminal pool attachment;
- initial layout-load request;
- core pane/layout mutation;
- navigator/explorer state;
- runtime palette item projection;
- Session row/action target projection;
- shared rendering/input helpers;
- unit/render regression tests;
- `event_loop`.

This is deliberate. The goal is not to reduce every file below an arbitrary size; it is to keep responsibility boundaries clear while preserving one state machine.

After extraction, the parent file is about 4.8k lines including its large test modules, with production code ending around the 2k-line mark.

## Visibility and composability rule

The child files extend the parent `Ui` type rather than wrapping or mirroring it.

That keeps:

- state ownership singular;
- helper reuse direct;
- compiler-enforced method/type coupling;
- no synchronization between duplicate UI models;
- no abstraction layer whose only purpose is indirection.

The child-module `use super::*` is intentional inside this one logical implementation module: it lets the partitions consume the parent’s private view-model types without widening those types into crate-wide APIs.

If a partition later becomes independently reusable, extract a typed value/service at that time rather than prematurely publishing all current UI internals.

## Regression guard

New checker:

```text
the removed test_post_phase58_workspace_decomposition helper
```

It verifies ownership rather than raw file size:

- all four partitions exist and are registered by `ui.rs`;
- each partition extends the existing `Ui` and does not define a second `Ui`;
- command methods live in `commands.rs`;
- input methods live in `interaction.rs`;
- async methods live in `async_work.rs`;
- render methods live in `render.rs`;
- those heavy methods do not regress back into the façade;
- Workspace still consumes the shared Agent composer;
- commands still canonicalize through the shared slash model;
- rendering still delegates tab paint/hit geometry to the typed tab-strip seam;
- async work still uses typed bounded result publication.

The checker is part of the reference validation matrix as:

```text
post-phase58-workspace-decomposition-reference
```

## Historical checker adaptation

Historical Phase 37–56 and Workspace/post-Phase-58 source checkers previously treated `src/workspace/ui.rs` as the entire Workspace UI implementation.

Those checks were updated to inspect the logical Workspace UI source set:

```text
src/workspace/ui.rs
src/workspace/ui/async_work.rs
src/workspace/ui/commands.rs
src/workspace/ui/interaction.rs
src/workspace/ui/render.rs
```

Their behavioral assertions were retained.

This was important: the decomposition did not “fix” failing historical checks by deleting old requirements. It made those requirements location-independent.

## Validation at implementation checkpoint

Fresh validation for `65905ad`:

- Workspace decomposition checker: **55/55**;
- fresh-review hardening checker: **42/42**;
- post-Phase-58 UX checker: **163/163**;
- post-Phase-58 review checker: **49/49**;
- Phase 37 first-run checker: **53/53**;
- Phase 41: **111/111**;
- Phase 42: **129/129**;
- Phase 43: **161/161**;
- Phase 44: **106/106**;
- Phase 47: **85/85**;
- Phase 48: **55/55**;
- Phase 51: **82/82**;
- Phase 52: **89/89**;
- Phase 53: **56/56**;
- Phase 54: **136/136**;
- Phase 55: **117/117**;
- Phase 56: **99/99**;
- Workspace interaction/presentation checker: **264/264**;
- native full-feature `aw` suite: **240/240**;
- whole-workspace/all-target compilation: green;
- whole-workspace/all-target Clippy with `-D warnings`: green;
- warning-denied Workspace feature cut: **210/210** native binary tests plus its inherited library/provider checks;
- workspace actual-binary PTY: **11/11**;
- doctests with warnings denied: green;
- source/static validation at the implementation tree: **1006 active files / 988 authored Rust test attributes / 734 local links / 0 errors / 0 warnings**;
- complete reference profile: green, including the new decomposition step.

Reference-profile receipt used during implementation validation:

```text
validation/host/20261007T075234Z-a3343d2ebdf9/report.json
```

As with prior reference evidence, that receipt does not claim Rust execution merely because separate Rust commands in this session were green.

## What did not change

- schema v32;
- Phase 58 tag/checkpoint;
- provider selection or billing behavior;
- tool grants or mutation authority;
- Session/branch durability semantics;
- no-replay recovery rules;
- semantic-sync cursor semantics from `49f3be8`;
- per-user atomic archive semantics from `49f3be8`;
- shared Agent composer semantics;
- config persistence semantics;
- terminal lifecycle/key ownership;
- execution routing/governance;
- release claims;
- remote/network authority.

## Maintenance guidance

Do not merge these partitions back into one large reducer merely because cross-partition methods share `Ui`.

Do not create duplicate state or event engines to make the files “more independent.”

Prefer the current pattern:

1. one authoritative state;
2. typed services for real authority boundaries;
3. implementation partitions for UI responsibility boundaries;
4. shared Agent/editor/palette services for cross-surface behavior;
5. focused regression tests at each seam.

Further decomposition should be driven by an actual ownership problem, not by line count alone.

## Next gate

User review or an explicitly requested next task.

Phase 59 remains undefined until explicitly requested.
