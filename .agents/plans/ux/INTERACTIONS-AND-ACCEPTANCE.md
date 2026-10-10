# V1.4 interaction, focus and test specification

Design-only acceptance matrix for future terminal-native study, NOT a passing test report.

## Focus model

Three independent notions:
1. Selection (highlighted Agent/file/project/Worktree/machine).
2. Pane focus (who owns navigation keys and cursor/scroll).
3. Effectful input owner (trusted Agent composer, PTY input/resize owner, Git staging capability).

Never fuse them. Viewing a machine's terminal is not authorization to write to it. Double-click/Enter on project/file selection opens/focuses view without running commands or staging Git.

## Basic flows

1. Launch bare aw -> Minimal Pi-like conversation, header and compact footer. No hidden daemon, remote sockets, external discovery or background Agent as prerequisite.
2. Toggle Agents -> show sidebar with Activity / Now preferred, actual nested children, machine tags; conversation scroll and composer stay anchored.
3. Toggle Workspace -> restore named layout; Review default Agent + Diff + Test PTY; optional inspector off.
4. Upper tab icon ▦ -> Projects; ◷ -> Recents. Same Space context persists. Recents includes recent visited Tabs, Agents, File, Repo, Terminal, not just projects.
5. Lower dock ● -> Activity (Recents/Now/Automations), ± -> Mini Git, ▤ -> Mini Files; independent of upper mode and main tab; mini files and git exact worktree only.
6. Upper Recents active -> Activity Recents auto-collapses (remember previous state). Return to Projects -> previous Activity expansion restored.
7. Click Activity attention -> exact actionable request; no separate duplicate Needs You tree. Background idle/completion may update quiet indicator but never steals focus.
8. Click header Space name -> Spaces tab; chevron -> quick switch menu. Click machine indicators -> Fleet tab; chevron -> Local Space picker.
9. Select mini Git entry -> full diff focused; NO staging by selection. Select mini Files entry -> File viewer focused; no write.
10. PTY tile focus -> send regular keys to actual terminal; multiplexer prefix is configurable and escape/focus-out discoverable. Explicit viewer -> owner takeover flow; remote disconnect disables input and marks stale.
11. Zoom -> enlarge selected pane; zoom out restores original layout, ratios, scroll and focus. Close pane means hide/detach, not kill.
12. Narrow terminal -> secondary panels collapse presentation but not underlying focus/layout state; when re-expanded restore ratios without scroll-anchor jumps.

## Keymap proposals (not final bindings)

- Ctrl+P where supported: global quick switch across all typed destinations. Fall back to configured prefix/command palette in terminals that intercept Ctrl+P.
- Ctrl+Shift+P: existing aw command palette behavior should remain.
- F2 or remappable action: cycle Minimal→Agents→Workspace, preserving selection. Audit against existing shortcuts before locking.
- F6/Shift+F6 or remappable: focus next/previous pane.
- Enter/Left/Right on focused tree: open/collapse/expand; keyboard mirrors mouse.
- Multiplexer prefix: pane split, focus, zoom/unzoom, move, resize and detach. Choose after PTY/terminal conflict audit.
- Tab navigation: selected/hovered/keyboard-focused icon exposes tooltip/help label; never icon-only without accessibility name.
- ESC inside PTY normally goes to the PTY; alternate escape via configured leader, not stealing Vim Esc.
- Click/drag sidebar split handle: resize lower dock with minimum 4 rows; single action to collapse; no implicit auto-focus.

## Visual acceptance

| Case | Correct behavior |
|---|---|
| Single repo/single Agent | Workspace still has useful Git/Files/Terminal and Minimal stays focused |
| Many projects | Projects collapse except selected; Recents flat and stable |
| Many Agents | Now tree shows parents/children and counts; detailed view search/filter opens on demand |
| Automation fires | Its actual live Agent appears once in Now; definition remains in Automations |
| One blocked Agent | Amber attention count and precise owner, without duplicate alert rows |
| Background success | Green completion and quiet unread indicator; no popover or pane theft |
| Git dirty on a1 | Git mini preview pinned to a1 Worktree, never to x2 merely because space is fleet |
| Local Home with no repo | Git says 'No repository selected'; Files lists normal Home contents |
| Machine offline | Stale timestamp and read-only pane; don't claim green running or auto-retry |
| 80x24 | Focused one pane, header/footer and on-demand nav accessible |
| 160x45 | Sidebar, Agent+Diff, Terminal beneath and no compulsory right inspector |
| Truecolor unavailable | 256/16/mono palette with shapes/text and non-overlapping borders |
| Tool output grows | Scroll anchor and composer stable; no giant colored box |
| File tree moves/scrolls | Independent focus and scroll persist across lower-dock tab switch |
| Detached client returns | Restore UI arrangement, not made-up OS process or charged model result |

## Mechanical check targets for next phase

Terminal-native snapshots should be generated at fixed widths, resized, visually inspected in dark/light/System truecolor and degraded 256/16/mono. Use recorded unicode widths to detect overflow, broken border segments or pointer focus regressions. Terminal PTY fixtures should validate alt-screen/backscroll, color strips and independent app input. Test semantics without changing live runtime until user explicitly approves implementation.

## V1.5 current acceptance override

- F2 or configured mode-switch action toggles exactly Minimal ↔ Workspace; never cycles through a third Agents mode.
- Minimal has no top Space/Machine header, no sidebar or layout tabs; only native Pi-like transcript/composer and bottom compact status.
- Selecting an Agent from Activity Now or palette selects Workspace Agent tab, optionally maximized, keeping the same Agent Session and child lineage. Zoom is view-only.
- Any pane has small header ⛶/❐ focus toggle; restoring a zoom reconstructs previous sibling panes, split ratios, scroll and focus. Double-click on pane **header only** may be an alternate, but double-clicking text/editor content must not steal selection.
- Files/Git container selection changes only its own contained preview; ↗ breaks the displayed File/Diff out as standalone tab, leaving the original contained tab intact.
- Mini Git, mini Files, Recents and Ctrl+P typed palette can directly open independent File/Diff tabs with exact source/worktree/scope identity. Reopening deduplicates, closing returns to originating tab.
- Standalone single-pane maximize is optionally immersive: hide Workspace sidebar/header/tab strip, retain clear restore action, retain underlying process/journal.
- Maximize PTY read-only view never grants terminal input/resize ownership; same exact host/effect permissions apply.
- Maintain historical V0–V1.4 test cases; V1.5 has its own browser regression covering all new transitions and per-pane zoom.
- Next genuine terminal/rust UX proof must reproduce headerless 80×24 Minimal, maximized Agent Workspace, contained Files/Git, standalone File/Diff and immersive maximizations. No production implementation plan yet.

See [V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md) and [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md).

## Current V1.5 refinement: one title layer and reused command palette

This section supersedes the prior V1.5 quick-switch suggestion: **F1 / Ctrl+Shift+P** invokes the already-existing aw command palette. Ordinary Ctrl+P belongs to input/history or focused PTY; a navigation-only fallback may be retained where the real aw input router already supports it.

Six live palette categories retain exact prefixes > Actions, ! Windows, @ Files, # Sessions, $ Terminals, / Commands; proposed & Agents adds typed Agent results. Use one popup, one input, category chips, ArrowUp/ArrowDown selection wrap, Enter activate, Esc close. Session search remains distinct from Agents; command result previews in HTML never execute real slash commands.

- Browser tests must reject a second quick-switch overlay and exercise all exact prefixes (including Windows !, not %).
- Palette selection of an Agent focuses its existing native Agent tab within Workspace; no third Agents mode or runtime spawn.
- File or Diff palette result opens a standalone typed tab directly; doesn't first open Files/Git container or grant write access.
- Standalone File/Diff tab displays filename exactly once in its selected tab; body holds only new worktree/site/scope. Immersive zoom retains floating identity/Restore when tabs hide.
- Contained Files/Git zoom retains one pane title, no second generic global context bar or duplicate filename row.
- Double-clicking source/editor text never maximizes; explicit button or focused pane-header interaction does.
- Actions, Windows, Sessions and Terminals preserve their existing semantic destinations; a Terminal viewer never gains input ownership via palette or zoom.
- The entire palette works in headerless Minimal without reintroducing a permanent header.

See [V1.5 canonical refinements](V1.5-UNIFIED-FOCUS-AND-TABS.md), [screenshots](VISUAL-GALLERY.md), and [fixed-cell palette proof](TERMINAL-V1.5-CELL-STUDY.md).
