# Phase 47 — native interaction polish and precise mouse feedback

**Status:** authorized 2026-10-05 by the user after Phase 46 review.

## Boundary

Phase 47 is a native fullscreen workspace/TUI presentation and input-correctness slice.

It changes no provider, account, routing, subagent, pipeline, team, Session-authority, filesystem-authority or billing semantics.

SQLite remains **v30**. No migration is added.

Phase 47 addresses six concrete user-facing regressions/requests:

1. command-palette category filtering and natural typed prefixes;
2. clean one-line Agent composer placeholder rendering;
3. responsive vertical folding for narrow Explorer/main tab strips;
4. exact rendered mouse hit geometry, including the Files/Git misclick;
5. minimal visual scroll-position feedback;
6. mouse text selection + clipboard copy for read-only main content.

## Command palette categories

Visible categories use regular/title case:

- Actions
- Files
- Sessions
- Windows
- Terminals

Natural query prefixes are:

- `>` — Actions
- `@` — Files
- `#` — Sessions
- `%` — Windows
- `$` — Terminals

No prefix means search all categories.

The query parser is the single source of truth for:

- typed prefix filtering;
- fuzzy-search body;
- command execution;
- command Tab completion;
- workspace file-search dispatch;
- category click state.

Clicking a category applies the same prefix as typing it.
Clicking the already-active category returns to all-category search.

A category hit region must be derived from the exact rendered label rectangle, never an approximate/equal-width partition.

The model/provider never sees command-palette query/category state.

## Explorer and main-tab geometry

The Explorer's Sessions / Files / Git selector and main content tabs use **rendered geometry as hit geometry**.

The old Explorer behavior where variable-width labels were rendered but mouse hit regions were divided into equal thirds is forbidden.

When available width is insufficient:

- Explorer Sessions / Files / Git fold vertically one-per-line;
- existing Sessions User / Autonomous control continues using the shared responsive segmented-control helper;
- existing Git Changes / Worktrees / Branches control continues using the same helper;
- main content tabs fold vertically one-per-line rather than clipping over one row.

Content begins after the actual number of tab rows consumed.

Narrow-layout presentation must not mutate the durable Workspace layout.

## Composer placeholder

The fullscreen Agent composer remains exactly one editable row plus its top separator.

When empty:

- narrow: `Ask anything…`;
- medium: `Ask anything…  / commands`;
- wide: normal contextual send/command/navigation hint.

The placeholder is bounded/truncated to the one-line editor width and must not create a second row or duplicate transcript hint.

No placeholder text is persisted as Agent content.

## Scroll-position feedback

Scrollable native workspace content gets a minimal one-column vertical indicator only when content exceeds the viewport.

The indicator uses:

- a faint track;
- a compact proportional thumb;
- no extra border/chrome;
- no persistent state.

Apply to:

- main read-only content/transcript/file/diff views;
- Files/Sessions Explorer lists;
- Git Changes/Worktrees/Branches lists;
- command-palette results.

The scrollbar position must use the same actual list/text offset as rendering.

Explorer list row mouse hit indices must likewise use the ListState visible offset; clicking a visible scrolled row must select that actual item, not row 0 + screen offset.

## Read-only text selection and clipboard

Mouse text selection is supported in read-only main content:

- Agent transcript/output;
- file previews;
- diff previews;
- Activity/Help/Context/Session text surfaces rendered in the main group.

It intentionally does **not** take over:

- embedded terminal input;
- Agent composer editing;
- command palette;
- Settings controls;
- Explorer action rows.

Selection is based on rendered terminal cells, including Unicode display widths and horizontal scroll offset.

Left-drag creates/updates a selection.
Mouse release preserves the visible selection and copies it to the terminal clipboard using bounded **OSC 52**.

Ctrl+C with an active read-only selection copies that selection first.
Without an active selection, existing Ctrl+C semantics remain unchanged:

- cancel active Agent work;
- or clear the editable draft when appropriate.

Clipboard payload is bounded to 256 KiB and base64-encoded locally.
No shell process, clipboard helper executable, provider call, filesystem authority or new secret-bearing integration is involved.

Phase 47 does not claim clipboard support in terminals that explicitly disable OSC 52; failure is surfaced as UI notice.

Selection is viewport-bounded in this phase; dragging does not auto-scroll.

## Security and authority

Phase 47 is presentation/input only.

It must not:

- execute model/provider work;
- mutate Session authority;
- add browser/native provider fields;
- add filesystem/shell grants;
- change routing/team/subagent budgets;
- persist palette/selection/scrollbar presentation state as authority;
- execute terminal OSC received from model/file text.

Incoming text continues through existing display sanitization. OSC 52 is emitted only from an explicit local user copy gesture.

## Validation requirements

At minimum:

- palette category labels are regular case;
- parser recognizes exactly `> @ # % $` and no-prefix all;
- click category hit regions exist and share the same parsed filter state;
- prefix-filtered results contain only the requested category;
- Explorer category hit regions derive from rendered rectangles;
- narrow Explorer tabs occupy separate rows;
- Files and Git rendered hit rectangles do not overlap;
- narrow main content tabs occupy multiple rows and content starts below them;
- empty composer remains two physical rows total (separator + one editor row) and retains meaningful narrow placeholder;
- scroll indicator is absent without overflow and renders track/thumb with overflow;
- Git/Explorer hit indices account for ListState scroll offset;
- main content exposes TextView hit surfaces;
- Unicode/multiline selection extraction is deterministic;
- OSC-52 base64 encoding has known-vector tests;
- Ctrl+C selection path precedes cancel/draft-clear only when a selection exists;
- tiny/no-default and optional-feature cuts remain green;
- exact whole-workspace/all-targets, doctests, rustdoc and Clippy remain green;
- existing fullscreen workspace and inline PTY smokes remain green;
- no SQLite schema/version change from v30.
