# NEXT: real terminal-cell UX prototype before any implementation plan

State: FIRST ANSI TERMINAL-CELL VISUAL STUDY COMPLETED (nine checked fixtures; see [TERMINAL-V1.4-CELL-STUDY.md](TERMINAL-V1.4-CELL-STUDY.md)). The full ratatui, true PTY, interactive input/focus/resize, native terminal feature proof below remains OPEN. Product baseline V1.4. No implementation plan authorized.

## Goal

Take the approved V1.4 HTML/spec/archive and render **actual terminal geometry and terminal color semantics**, not browser-styled rectangles. Iterate the visual quality and behavior before turning any of these into Rust feature plans.

## Scope boundaries

- Visual-only terminal harness in its own approved UX area, not a replacement for the product Agent, journaling, PTY, Git, filesystem or terminal runtime.
- Begin by rendering a fixed set of Unicode/ANSI cell grids to a real terminal/PTY or using a thin standalone ratatui visual-only harness (subject to user approval of writing code).
- Fixture names/tool counts/example data must remain labeled illustrative; never leak real keys or inspect real sessions for visual samples.
- No production wiring until user approves implementation planning separately.

## Exact screens to render as proof

1. V1.4 Minimal Pi-like Agent at 80x24, 100x30, 120x40, 160x45: unboxed composer, tool strips and long scrollback.
2. Agents: nested depth-3 tree; one working, one completed, one approval waiting, one disconnected/stale, terminal machine tags.
3. Workspace Review: sidebar Projects + Activity; Agent and Diff split with terminal below, at 160x45 and 200x60.
4. Upper navigator Recents (flat, full) while lower dock Activity/Now expanded; verify Recents not duplicated.
5. Mini Git staged/unstaged with no-repo Local Home empty state; Mini Files changed highlights and lazy tree.
6. Full Git: file list, focused hunk, split/unified diff and zero accidental staging from row selection.
7. Full Files: tree, syntax-highlighted file, line numbers and horizontal scroll.
8. Full Terminals: four true PTY viewports and focused ownership badge; remote disconnected/unknown state.
9. Spaces and Fleet full manager screens; Space/Machine quick switch overlays at both narrow and wide widths.
10. Sidebar collapse and keyboard resize; inspector optional; zoom/unzoom; focus ring and branch switching without state loss.
11. Light/dark/System and ANSI truecolor/256/16/monochrome comparisons, ASCII fallback.
12. Long Agent tool log streaming while sidebar receives background events: transcript scroll anchor remains stable.

## Terminal-specific constraints

- width measured in cells, never CSS pixels; Unicode wcwidth and border glyph fallback; double-wide emoji prohibited in structural controls unless measured.
- Input and alternate-screen semantics are per PTY; ESC can belong to Vim; multiplexer leader must not intercept regular terminal input.
- Cursor and popup position deterministic after resize; composer bottom pinned to focused native Agent pane.
- Distinguish status color from connection health; don't mix source machine status and Agent state.
- Draw order: background → pane borders → app cells → overlay popup → cursor, respecting full terminal clipping and repaint frequency.
- No global clear in inline mode; native fullscreen may use alternate screen deliberately.
- Verify actual screenshots using PTY/tmux/wezterm/kitty terminals as available, and emulator-independent ANSI snapshots.

## Deliverables for terminal iteration

- Named screen fixtures, fixed test sizes and theme values.
- Rendered ANSI recordings and PNG snapshots for each fixture with exact terminal rows/cols.
- Diffable cell snapshot representation for regression; notes from visual review.
- Keyboard/focus/scroll/resize acceptance test plan, **then** implementation breakdown only after explicit UX sign-off.

The browser HTML archive remains a design reference but is not accepted as actual terminal rendering proof.

## V1.5 visual refinement status

A subsequent V1.5 standalone terminal-cell study now reproduces the **headerless Minimal**, merged two-mode Workspace, maximized native Agent/File/Diff/PTY, and contained-versus-standalone File/Diff distinctions at fixed terminal widths. Read [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md). The earlier V1.4 source and captures remain unchanged. Real ratatui interactive rendering, PTY owner/cursor tests and implementation planning remain OPEN; HTML/browser/ANSI cell visual proof is not a runtime integration certificate.

## Final V1.5 visual pass before future ratatui prototype

Terminal-cell and offline browser designs now demonstrate (a) one per-document title (standalone/contained/immersive) and (b) one real-aw-shaped command palette with Actions, Windows, Files, Sessions, Terminals, Commands and proposed Agents. The remaining native proof must use the EXISTING aw palette renderer and introduce just an Agents category rather than a new command-picker module. Test terminal width, ANSI layering/clear, keyboard ownership, portable Ctrl+Shift+P/F1, and arrow/Enter/Esc semantics in an actual PTY/ratatui harness. Do NOT treat these visual mockups as implementation plans or claim runtime integration.
