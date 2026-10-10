# Visual/TUI rendering specification (canonical V1.4)

Scope: appearance and geometric behavior. Not current aw implementation. See V1.4-CANONICAL.md and HTML index.html.

## Responsive grid: real terminal-cell units

Viewport topology:

    Row 0           header (Space/project + machines), one terminal row
    Rows 1..H-3     body = optional left sidebar + selected main UI/pane tree
    Row H-2         composer belongs INSIDE native Agent pane, not global Workspace
    Row H-1         compact global status when shown; may be 2 lines per original aw product constraints

Target starting geometry by terminal cols x rows:
| Size | Layout choice | Left sidebar | Right inspector |
|---|---|---:|---|
| 80x24 | primary Agent/file/terminal pane; sidebar as overlay or collapsible | hidden default | hidden |
| 100x30 | Agents/Workspace two panes at most | 24 columns | hidden |
| 120x40 | sidebar + one main pane / narrow split if requested | 27–29 columns | hidden |
| 160x45 | full Review Agent+Diff+test PTY | 29–33 columns | hidden |
| 200x60 | Review Grid; optionally inspector toggled | 32–36 columns | user-selected only |

Pane width/height ratio stored independent of terminal actual geometry; when constrained collapse secondary panes but preserve requested ratio for expansion. Sidebar split default 60% upper navigator, 40% lower dock of usable sidebar rows after 2 mini headers. Mouse drag plus keyboard resize; dock collapsed becomes one header row. Never let a resize cause an Agent output scroll anchor or cursor loss.

ASCII-aware tree connectors: Unicode '│ ├─ └─ ▾ ▸ ◆ ◉ ▤ ◷' when supported; fallback '| +-- `-- v > * @ # ~'. Glyph display widths MUST be measured by terminal cell width; symbols can't make columns drift. A colorless/mono terminal remains understandable.

## Color semantics

Use semantic terminal tokens; RGB samples are HTML mockup values, NOT hardcoded device assumptions.

| Token | Dark truecolor example | Light example | Meaning |
|---|---|---|---|
| canvas | #14151c | #fbfbfd | base |
| sidebar/panel | #1b1d27 | #f0f1f6 | chrome |
| raised | #252734 | #e9eaf2 | selected input/context |
| border | #363846 | #d6d9e4 | understated |
| text | #dadde8 | #303347 | foreground |
| muted | #9a9cae | #6f7386 | secondary labels |
| faint | #6e7285 | #a1a5b3 | guides only |
| purple | #b9a0e8 | #7557ad | selection/branch/context |
| blue | #90b5f2 | #3f78c3 | working/progress/unread |
| green | #87c99c | #337d4d | confirmed success/healthy |
| amber | #e6bc76 | #9b6c1d | attention/warning/modified |
| red | #eb929c | #bd5260 | failure/blocked |
| neutral | #9a9cae | #6f7386 | idle/unknown |

Implement separate truecolor, 256-color, 16-color and monochrome palettes. For System mode resolve terminal background using supported query/fallback, not assuming an RGB browser value or leaking terminal state.

Do not signal semantics by color alone: pair blue with '● working', amber with '◐ waiting' or 'M modified', red with '! blocked', green with '✓ complete' or '● connected', gray with '○ idle'/'? stale'. Tool result surfaces are **muted strips**: soft green/red/neutral tint or simple left border; never saturated filled cards. Selection is one faint row highlight, not a giant boxed control. User message minimal unboxed/thin colored left rule.

## Chrome hierarchy

1. Global 1-line header: 'aw / {Space} ▾ / {project} ◆ {branch}' left; machine health '● x2 ● a1 ▾' right. Status and machine switchers separate click targets. Truncate breadcrumbs from middle without losing selected Scope.
2. Upper nav mini header: title 'projects'/'recents' left; [▦][◷] right. Lower mini header: title 'activity'/'git'/'files' with meaningful single badge, [●][±][▤] right. Selected tab = subtle purple underline, tooltip label, accessible name, focus ring.
3. Main tab strip in Workspace is always the saved layout tab name; native Agent Minimal intentionally omits it.
4. Main pane header: type glyph, human name, location/machine and owner/viewer/read-only state; emphasized only for focused pane.
5. Footer: current exact worktree, dirty status, activity attention; native Agent may additionally show provider/context/tool budget. No made-up financial usage.

## Activity/recents tree conventions

Activity is a projection, not ownership. Now shows actual Agent hierarchy with nested connectors (owner->child), machine/worktree shown after dot in muted color; automation origin symbol ↻ may appear next to a live Agent, but no duplicate entry under separate Background category. Recents stable by deliberate visit, not last emitted event. Attention badge rolls up only true user-action-needed events. Unknown link states remain stale until actual provider says otherwise.

## Mobile/narrow external panels

This is a desktop terminal-first design. HTML responsive render is illustrative only. Actual narrow TUI must use cell-width calculations and only one application focus at a time. Tab bars horizontally scroll or shorten with stable IDs. Do not pop file/terminal overlays unsolicited; internal modal required for irreversible actions.

## Rendering correctness

- Zero width ambiguity: every cell painted exactly once; clipping bounded to terminal dimensions.
- No layout oscillation when scrollbar appears; overlays should not shift pane widths.
- Mouse focus and keyboard focus indicate same focused pane; selecting a tree node is distinct from PTY write ownership.
- Stable scrollback on background updates; bottom composer anchored to owned Agent pane, not the terminal middle.
- Palette color state consistent across all panes; soft focus, no gray slab behind user message.
- Idle Agent status must never visually imply disconnected machine or vice versa.

## V1.5 current rendering override (historic V1.4 above remains intact)

**New canonical semantics:** The app offers only Minimal and Workspace modes, not separate Agents mode. In Minimal, render zero top-header rows and no persistent tab/sidebar/inspector; leave the native Agent transcript, composer and exactly one compact bottom status. Workspace retains its one-row Space/Machine header and icon-only mini docks. A maximized native Agent fills the Workspace main pane, with outer navigation still available; this replaces the previous Agents view.

Every pane can display an unobtrusive keyboard-focusable ⛶ maximize / ❐ restore icon on the right of its thin pane header. Contained File/Diff viewers additionally show ↗ break-out; standalone tabs show ×. These controls should not force another full text toolbar. For a standalone File/Diff already occupying the tab, immersive maximize may hide Workspace sidebar/header/tab strip while retaining the pane and visible Restore.

On constrained terminals preserve actual cell-width accounting, color palette fallback, compositor focus, and per-tab scroll/ratio state. The two-mode switching action must not reflow a PTY during an active external effect merely because its pane was hidden. Opening a file/diff from mini dock or global palette opens a single full tab without creating the Files/Git container. Selecting a file inside the full Files/Git tab previews inside that contained tab. The same exact source ref should appear consistently across these projections without duplicate scanners or authority.

Terminal-cell visual fixtures and HTML gallery: [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md) and [V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md).

## V1.5 no-double-chrome / current palette overlay visual tokens

Top-level Workspace Space/Machine header appears once; main tab strip contains document/layout identity. Suppress the generic extra title/context bar completely. Split-pane headers name Agent/Diff/Editor/PTY ONLY if needed to disambiguate siblings; no repeated basename on a viewer's first line. Standalone single-document tabs use no inner pane title at all: one selected tab label and a tiny floating maximize control. In immersive focus the tab strip is hidden and a small filename + Restore micro-control provides the necessary context without a full-width bar.

Centered Command Palette follows the existing aw ratatui visual outline: subtle neutral border, Command Palette title, one editable › line, seven small category labels using > ! @ # $ / & (first six from actual aw; & Agents proposed), selected result highlight and short footer for ↑↓/Enter/Esc. Its backdrop must actually clear text behind the popup in the terminal-cell painter; merely repainting colors must not leave unrelated Agent/diff glyphs bleeding through. This popup is ONE source for actions, windows, files, sessions, terminals, commands and agents, available in Minimal and Workspace.

Avoid a second permanent search bar, a duplicate title, or another row of chrome. Keep result source/machine/scope information as faint metadata and only when distinguishing destinations. Dark/light/mono and Unicode/ASCII fallbacks retain category names and selectable row labels.
