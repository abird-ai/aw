# References and visual inventory

Archive date 2026-10-11. All included previews use **invented example data**; they do not expose real local Git status, machines, credentials or Agent work.

## Inspiration and interpretation

1. **Pi / pi-inspired Agent TUI** — minimal fast conversation, unobtrusive user composer, stream/tool progress, no card chrome in focused mode. Root V0 visual baseline.
2. **boadij/pi-herdsman** — https://github.com/boadij/pi-herdsman — nested Agent/subagent supervision in a legible tree, branch connectors, small colored state lights, direct child navigation, and compact progress visibility.
3. **Herdr** — https://herdr.dev/ — true PTY multiplexer, mixed Claude/Codex/OpenCode/editor/dev-server tiles, saved layout tabs, mouse focus/resize, attention/unread status, persistent hosted client detach, local/remote connection management, per-machine owner/viewer distinction.
4. **User supplied screenshot: pi-herdsman** — the initial dark terminal view with nested Agent children and green status feedback, source of tree connector/semantic-light direction.
5. **User supplied Herdr screenshots (three)** — one Agent + development server, multiple mixed-pane code/edit sessions, and focused single Agent. Main takeaway: use almost all screen space for applications, no permanent inspector unless requested, make authentic PTYs fully functional.
6. **Iterative user-approved aw design discussions** — preserved V0, V1, V1.1, V1.2, V1.3, V1.4 documents and HTML live mockups. These are the actual design decision history rather than replacing V0's strengths with V1.4 fashion.

No third-party screenshot or copied HTML/CSS is included in this archive. Visual assets and fixtures are original reinterpretations of approved concepts. This avoids license, privacy, screenshot redistribution and outdated-demo confusion.

## HTML inventory

Each HTML file is a **full switchable UX app**, built on shared local ux.css/ux.js; offline via file:// and no external requests.

| HTML | Expected default | Full previews |
|---|---|---|
| index.html | V1.4 Workspace Review | All versions via version switcher |
| v0.html | V0 Minimal Agent | Original Minimal/Agents/Workspace with inspector |
| v1.html | V1 Review tiling | Minimal/Agents/Workspace; Review/Agent/Git/Files/Build/Fleet |
| v1.1.html | V1.1 quiet Recents | Full V1 panes + compact recents & Activity |
| v1.2.html | V1.2 global header switchers | named fleet + per-machine Local; full management tabs |
| v1.3.html | V1.3 textual Projects/Recents & Activity | same full Workspace, activity tree and machine switch |
| v1.4.html | V1.4 tiny-icon upper tabs, mini Activity/Git/Files dock | all three modes, main Git/Files/Diff/PTY, narrow/focus/split/grid |

The universal switcher offers: version, mode, scope, theme, nav, Activity group, lower dock, named Workspace view, review layout, inspector/compact/zoom, current mock selection, popovers and keyboard shortcuts. Disabled or hidden options are version-correct; no decorative "working" buttons.

## Screenshot gallery

See [VISUAL-GALLERY.md](VISUAL-GALLERY.md). PNGs in previews/ are generated from the actual HTML using a local headless browser when supported, for quick human side-by-side comparison. They are explicitly **browser concept snapshots**, not ratatui/ANSI screenshots. The next iteration will create actual terminal-renderer proof.

## Nonclaims

The archive neither changes any aw source nor asserts code exists for Fleet Spaces, runtime-updating terminal tiles, approvals, Background Agents, remote navigation or per-client notifications. Prior V0/V1 visuals are retained for comparison even where their sidebar structure was superseded by V1.4.

## Post-archive native-color terminal-cell study

Nine additional V1.4 fixture sets are now under terminal-previews/, with .ansi, .txt, .svg and .png for each. The independent terminal-v1.4.py painter generates an exact character grid and real ANSI foreground/background color sequences; it neither invokes aw nor uses browser CSS for layout. See [TERMINAL-V1.4-CELL-STUDY.md](TERMINAL-V1.4-CELL-STUDY.md). This supplements but does not overwrite the historical V0–V1.4 HTML archive or the canonical V1.4 design.

## V1.5 design iteration and screenshots

The newest user refinement merges Agents into a maximized native Agent pane within the regular Workspace and leaves Minimal **headerless**, with only bottom status. All File/Diff panes can be maximized; Files/Git containers retain internal preview behavior, while mini sidebar, quick palette and explicit ↗ break-out create independent file/diff tabs. Original V0–V1.4 source inspiration and screenshot inventory remain historical and intact.

V1.5 adds [HTML](v1.5.html), [canonical spec](V1.5-UNIFIED-FOCUS-AND-TABS.md), an additional V1.5 browser click test, 13 HTML screenshots and 16 ANSI/SVG/PNG fixture sets. See [VISUAL-GALLERY.md](VISUAL-GALLERY.md), [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md). No third-party source images, bundled fonts, or other agent worktree changes.

## Read-only verified current aw palette contract for V1.5 UX

At this design checkpoint, read-only inspection of src/workspace/palette.rs confirmed Actions >, Windows !, Files @, Sessions #, Terminals $, and Commands /. src/workspace/ui/interaction.rs confirms Ctrl+Shift+P/F1 opens the full palette; a collapsed Ctrl+P fallback applies only when navigating without a focused Agent input or PTY. The planned & Agents extension is deliberately **new** and design-only. Avoid independently reinventing the existing palette.

The V1.5 visual archive now includes one centered palette in Workspace and Minimal, filtered Agents and Files variants, a single document title, and refreshed ANSI/cell views. Historical V0–V1.4 visual fixtures and source references remain intact; no changes are made to the live palette Rust implementation as part of this UX iteration.
