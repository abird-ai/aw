# V1.5 terminal-cell visual study — unified focus and tabs

State: **VISUAL PROOF ONLY**, not ratatui/real PTY or a production implementation. Follows V1.4 terminal study, which remains separately preserved.

The independent [terminal-v1.5.py](terminal-v1.5.py) renderer builds on V1.4's pure character-cell painter and palette. It generates genuine 24-bit ANSI SGR streams, exact rows/columns of plain-text cells, and cell-derived SVG/PNG. These are **not** screenshots of an executing aw Agent, an OS PTY or any production File/Git backend.

## Exact terminal states and sizes

| Concept | Grid | Preview |
|---|---:|---|
| Headerless Minimal (ASCII) | 80×24 | [PNG](terminal-previews/v1.5-minimal-headerless-80x24.png) · [Text](terminal-previews/v1.5-minimal-headerless-80x24.txt) |
| Headerless Minimal (Unicode) | 120×40 | [PNG](terminal-previews/v1.5-minimal-headerless-120x40.png) |
| Full Workspace Review | 160×45 | [PNG](terminal-previews/v1.5-workspace-review-160x45.png) · [ANSI](terminal-previews/v1.5-workspace-review-160x45.ansi) |
| Full Workspace Review wide | 200×60 | [PNG](terminal-previews/v1.5-workspace-review-200x60.png) |
| **Agent maximized, still Workspace** | 120×40 | [PNG](terminal-previews/v1.5-agent-max-120x40.png) |
| Files container (tree + viewer) | 160×45 | [PNG](terminal-previews/v1.5-files-contained-160x45.png) |
| Files viewer maximized within container | 120×40 | [PNG](terminal-previews/v1.5-files-editor-max-120x40.png) |
| Standalone File tab | 160×45 | [PNG](terminal-previews/v1.5-file-tab-160x45.png) |
| Standalone File immersive max | 120×40 | [PNG](terminal-previews/v1.5-file-tab-max-120x40.png) |
| Git container (changes + diff) | 160×45 | [PNG](terminal-previews/v1.5-git-contained-160x45.png) |
| Git contained diff maximized | 120×40 | [PNG](terminal-previews/v1.5-git-diff-max-120x40.png) |
| Standalone Diff tab | 160×45 | [PNG](terminal-previews/v1.5-diff-tab-160x45.png) |
| Standalone Diff immersive max | 120×40 | [PNG](terminal-previews/v1.5-diff-tab-max-120x40.png) |
| Single terminal pane maximized | 120×40 | [PNG](terminal-previews/v1.5-terminal-max-120x40.png) |
| Workspace Review light | 160×45 | [PNG](terminal-previews/v1.5-review-light-160x45.png) |
| Workspace Review monochrome/ASCII | 100×30 | [PNG](terminal-previews/v1.5-review-mono-100x30.png) |

Every scenario also has a companion .ansi, .txt, and .svg under terminal-previews/. The images are derived from the same grid as ANSI and do not use CSS Flexbox to position TUI panes.

## Reproduce

    python3 .agents/plans/ux/terminal-v1.5.py --width 80 --height 24 --mode minimal --glyphs ascii
    python3 .agents/plans/ux/terminal-v1.5.py --width 120 --height 40 --mode workspace --tab agent-max
    bash .agents/plans/ux/render-terminal-v1.5.sh
    python3 .agents/plans/ux/validate-terminal-v1.5.py

Standard output contains actual ANSI cursor/color codes. Display it in a **spare matching-sized terminal**, not inside a running Agent scrollback.

## UX interpretation

- Minimal has **zero global topbar**. Essential context, model, dirty state and attention are condensed in one bottom status line.
- Workspace Agent maximization shows the native transcript as the only main pane; the Space/Machine header, sidebar and tabs remain available. That replaces the former "Agents" mode.
- Files/Git "contained" are structurally different from File/Diff standalone tabs, even when showing the same file. A contained editor/diff can maximize without opening a new tab.
- An immersive standalone File/Diff zoom hides Workspace chrome while retaining its own pane title and restore hint. It still cannot write Git/files or change execution-site rights.
- Monochrome/ASCII fallback replaces Unicode structural glyphs and preserves the position of borders, labels, headers and composer.
- The source is deliberately static and untrusted fixture data; no terminals were actually spawned, no file trees scanned and no Git changes read.

## Remaining visual/behavior proof before Rust implementation plans

The next iteration should use **real ratatui rectangle layout**, terminal emulator/PTY color-cell captures and interactive mouse/keyboard/focus/resize tests. Measure Unicode width (including ambiguous and double-wide glyphs), terminal scrollback, alt-screen, terminal owner permissions and actual narrow-terminal fallback. Do not call these PNGs executable TUI integration evidence. [V1.5 canonical spec](V1.5-UNIFIED-FOCUS-AND-TABS.md) is the UX target, with earlier visual designs preserved for context.

## Clean-header and single-palette extension — four more fixed-cell screens

The terminal-cell fixture generator was polished: **no generic second Workspace title row**, no repeated inner File/Diff title for standalone tabs, and editor/diff context text carries only new worktree/site/scope information. Immersive views retain the compact filename/restore affordance. The ANSI painter now clears underlying glyphs when placing the centered Command Palette; changing background color alone left ghost text.

| New fixture | Fixed character grid | PNG |
| --- | --- | --- |
| Unified palette over Workspace Review | 120×40 | [PNG](terminal-previews/v1.5-palette-workspace-120x40.png) |
| Agents (&) category | 100×30 | [PNG](terminal-previews/v1.5-palette-agents-100x30.png) |
| Files (@) category over standalone file | 120×40 | [PNG](terminal-previews/v1.5-palette-files-120x40.png) |
| Palette usable in headerless Minimal | 80×24 ASCII | [PNG](terminal-previews/v1.5-palette-minimal-80x24.png) |

Every fixture includes ANSI truecolor SGR, text cells, SVG and a cell-derived PNG. The 20 V1.5 scenarios are generated by render-terminal-v1.5.sh and checked by validate-terminal-v1.5.py. This is NOT proof of live ratatui, actual PTY input, palette Rust integration or provider/command execution.
