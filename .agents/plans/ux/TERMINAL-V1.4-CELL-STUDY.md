# V1.4 terminal-cell visual study — first rendered pass

State: VISUAL STUDY ONLY, after the UX archive commit. **Not** aw runtime integration, not ratatui/PTY implementation or an implementation-plan breakdown.

The goal is to inspect how the canonical V1.4 mock actually fits in a fixed terminal grid, with real foreground/background RGB terminal color codes and Unicode/ASCII cell fallbacks. The source is [terminal-v1.4.py](terminal-v1.4.py), a standalone stdlib-only painter. It creates one char/color/background cell per row/column and exports three equivalent views:

- **.ansi**: actual ANSI SGR 24-bit fg/bg escapes, matching the cell colors. Preview in a spare terminal (don't paste ANSI into the native aw transcript).
- **.txt**: plain text, exact row/column layout without ANSI, easy to diff and inspect.
- **.svg / .png**: cell-by-cell placement of the SAME grid for offline design review. PNG is rasterized by a headless browser but does not use browser flex/grid/CSS pane layout. It is *not* a snapshot from a real ratatui/PTY process.

## Screen inventory

| Preset | Size | Palette | Proof |
|---|---:|---|---|
| Review with Activity, nested projects | 160×45 | dark truecolor | [PNG](terminal-previews/v1.4-review-160x45.png) · [ANSI](terminal-previews/v1.4-review-160x45.ansi) · [text](terminal-previews/v1.4-review-160x45.txt) |
| Review on wide terminal | 200×60 | dark truecolor | [PNG](terminal-previews/v1.4-review-200x60.png) |
| Mini Git + Review split | 120×40 | dark truecolor | [PNG](terminal-previews/v1.4-mini-git-120x40.png) |
| Agents and Recents | 100×30 | dark truecolor | [PNG](terminal-previews/v1.4-agents-100x30.png) |
| Pi-like minimal; no sidebar, no boxed Agent | 80×24 | dark truecolor + ASCII fallback | [PNG](terminal-previews/v1.4-minimal-ascii-80x24.png) · [text](terminal-previews/v1.4-minimal-ascii-80x24.txt) |
| Full Git / diff | 160×45 | light truecolor | [PNG](terminal-previews/v1.4-full-git-light-160x45.png) |
| Terminal grid | 160×45 | dark truecolor | [PNG](terminal-previews/v1.4-terminals-160x45.png) |
| Narrow Workspace | 100×30 | dark truecolor | [PNG](terminal-previews/v1.4-review-narrow-100x30.png) |
| Monochrome/ASCII degradation | 120×40 | monochrome | [PNG](terminal-previews/v1.4-monochrome-120x40.png) |

Every preset also has .ansi, .txt and .svg companions in terminal-previews/.

## Reproduce

    python3 .agents/plans/ux/terminal-v1.4.py --width 160 --height 45 --mode workspace --dock activity
    python3 .agents/plans/ux/terminal-v1.4.py --width 80 --height 24 --mode minimal --glyphs ascii
    bash .agents/plans/ux/render-terminal-previews.sh
    python3 .agents/plans/ux/validate-terminal-v1.4.py

The direct Python commands **print genuine ANSI to stdout**. Use a spare PTY with matching dimensions if displaying, because cursor positioning and colors affect terminal output. For a safe non-rendering check, read a generated .txt companion.

## First visual observations for iteration

- On 160×45 the proposed 60/40 sidebar split and Review Agent+Diff+Test PTY are readable without a permanent inspector; the content is constrained in terminal cells, not CSS pixels.
- On 120×40 Mini Git can show a compact changed-file list while preserving the main Review panes; small icons conserve space.
- On 100×30 a primary-focused Workspace/Agents view is much more legible than squeezing three tiles into 70 remaining columns.
- On 80×24 Minimal must be *unboxed*, with the thin Pi-like input, compact tool strips and bottom composer. Sidebar hidden.
- Semantic colors stay subtle, with symbols/text making success, activity and attention understandable in monochrome.
- Unicode line-drawing/selection works only with compatible terminal font/cell widths; a complete ASCII fallback and ncurses/rust ratatui mouse/resize proof still need validation.
- Current text content is sample fixture data, not a recorded Agent call. No real subprocess, Live PTY, network access, Git inspection or command input occurs.

## Open before implementation planning

This pilot **does not prove** actual rust ratatui rendering, VT escape interpretation, true terminal screen resizing, alternate-screen ownership, PTY cursor input, mouse, Ctrl-P collision handling, real multi-client ownership or native Windows/macOS. Those remain subjects for visual iteration and a dedicated ratatui/PTY fixture once the user signs off the shapes and spacing. Do not convert this into a runtime feature plan prematurely.

V1.4 remains the canonical target. The HTML prototypes still contain the richer interactive tab/scope switcher; this terminal study is the first precise cell/ANSI visual layer over that target.
