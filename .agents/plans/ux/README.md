# aw TUI UX design archive — V0 through V1.5

Status: DESIGN ONLY. Canonical target: **V1.5** (two modes, native File/Diff tabs and pane maximize). V1.4 is the preserved immediately preceding baseline. Archive date: 2026-10-11.
This directory contains visual reference artifacts and prospective behavior only; none of its TUI, remote-machine, permission, durable-queue, or PTY interactions is asserted to be implemented in live aw.

## Open the interactive prototype

1. Open **index.html** in any current Chromium/Firefox browser, including from file://; no web service, dependencies or build are needed.
2. Use the version control to select V0, V1, V1.1, V1.2, V1.3, V1.4 or V1.5. Change interface mode (V1.5: Minimal / Workspace; earlier versions also show historical Agents mode), tab, navigator, activity group, context, appearance, tiles and sidebar dock.
3. Each version also has a directly openable HTML file: v0.html, v1.html, v1.1.html, v1.2.html, v1.3.html, v1.4.html, v1.5.html. The switcher is present on every page.
4. Offline PNG preview gallery is in [VISUAL-GALLERY.md](VISUAL-GALLERY.md), with artifacts in previews/. These pictures are frozen snapshots of the authored mockups, not real ratatui/PTY output.
5. All interactions and data in HTML are **simulations**. There are no network calls, filesystem access, real provider invocations, commits or process starts.

## Reading order — preserve every previous design

| Version | Authoritative design | Core addition |
| --- | --- | --- |
| **V0** | [V0-BASELINE.md](V0-BASELINE.md) | Pi-like minimal Agent; nested Agent sidebar; first-class Workspace Git/files/diff/terminals; gentle status lights |
| **V1** | [V1-TILING.md](V1-TILING.md) | Herdr-inspired tabs, tiled native Agent/PTY/Git/files; zoom; space/machine visibility |
| **V1.1** | [V1.1-RECENTS-ACTIVITY.md](V1.1-RECENTS-ACTIVITY.md) | Recent destinations, quiet background work, automation definitions and attention |
| **V1.2** | [V1.2-SPACE-MACHINE-HEADER.md](V1.2-SPACE-MACHINE-HEADER.md) | Move Space + machine navigation to top header; Local Home and fleet Space distinction |
| **V1.3** | [V1.3-INTEGRATED-NAV.md](V1.3-INTEGRATED-NAV.md) | Projects/Recents upper navigator and Activity (Recents/Now/Automations); full tiled Workspace preserved |
| **V1.4** | [V1.4-CANONICAL.md](V1.4-CANONICAL.md) | Preserved V1.4 baseline: upper Projects/Recents + lower Activity/Git/Files icon dock |
| **V1.5** | **[V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md)** | **CURRENT: Minimal headerless; only Minimal/Workspace modes; every pane maximizable; contained vs standalone File/Diff tabs** |

## Cross-cutting specifications

- [VISUAL-SPEC.md](VISUAL-SPEC.md): layout dimensions, palettes, status lights, connectors, terminal limits, accessibility, visual invariants.
- [NAVIGATION-AND-STATE.md](NAVIGATION-AND-STATE.md): Space/Machine/Project/Worktree/Session/Actor definitions; state ownership and recent/attention semantics.
- [INTERACTIONS-AND-ACCEPTANCE.md](INTERACTIONS-AND-ACCEPTANCE.md): keyboard/mouse/pane focus, rapid navigation, per-view actions, acceptance scenarios, observability rules.
- [REFERENCES-AND-VISUAL-INVENTORY.md](REFERENCES-AND-VISUAL-INVENTORY.md): source inspiration, interpretation of supplied screenshots, screenshots and HTML coverage matrix.
- [TERMINAL-PROTOTYPE-NEXT.md](TERMINAL-PROTOTYPE-NEXT.md): **the next step** is an actual terminal-cell/ANSI/ratatui visual study before writing feature implementation plans. Do not confuse HTML screenshots with terminal screenshots.

## Reproducing and validating the archive

From the aw checkout, only UX-directory files are touched by these commands:

    python3 .agents/plans/ux/validate-ux.py
    python3 .agents/plans/ux/validate-ux.py --browser
    bash .agents/plans/ux/render-previews.sh

The validator checks all documentation links, versioned HTML wrappers, offline assets, JavaScript syntax, all 36 browser concept screenshots and actual headless-browser rendering. The browser suite also executes historical V1.4 and expanded V1.5 scripted UI interaction assertions, including unified-palette keyboard and category filtering on the separate test pages. The optional screenshot generator uses an installed local font via temporary fontconfig (never commits font files), and regenerates 36 rendered concept captures in previews/. These are mock HTML graphics, not real TUI terminal frames.

## Current V1.5 — only two modes

**Minimal:** no top header, no sidebar, no tabs—just native Pi-like conversation, composer, and bottom status. **Workspace:** full V1.4 tiled UI, with Agent pane max replacing the old Agents mode; contained Files/Git previews, `↗` breakout to independent standalone File/Diff tabs, and `⛶`/`❐` pane maximize/restore. V1.5 captures in [gallery](VISUAL-GALLERY.md) and exact ANSI character-cell study in [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md).

    bash .agents/plans/ux/render-terminal-v1.5.sh
    python3 .agents/plans/ux/validate-terminal-v1.5.py

## Preserved V1.4 structure

    aw / abird ▾ / aw ◆ main                  ● x2 ● a1 ▾
    ┌─────────────────────┬─────────────────────────────────┐
    │ projects     [▦][◷] │ [Review] [Agent] [Git] [Files]… │
    │ ▾ aw           M 4  ├────────────────┬────────────────┤
    │   ◆ main · x2       │ native Agent   │ Diff · tool.rs │
    │   ◆ feat/hooks · a1 │                │                │
    │ ▸ dotlink           │                │                │
    │ ▸ abird-health      │                │                │
    │ ─────── ↕ ───────── ├────────────────┴────────────────┤
    │ activity   [●][±][▤]│ Terminal · tests · pvl-a1       │
    │ ▾ Recents        3  │ $ cargo test                    │
    │   ▤ Review         │ ✓ 47 passed                     │
    │ ▸ Now            3 │ $ ▍                            │
    │ ▸ Automations    2 │                                │
    └─────────────────────┴────────────────────────────────┘

## Implementation boundary and collaboration

Only UX specification files are in scope in this UX archive. This history intentionally **does not edit runtime crates, source code, the frozen Oct-11 handoff, schema, tool grants, old Phase plans, tags or implementation roadmaps**. Other agents can continue their own aw implementation without importing these mock controls as authority. Git is the source of truth for the current implementation; V1.5 is the current product-design target, not an implementation claim.

Future changes: new UX ideas should use a versioned successor (V1.6 etc.) or an explicitly marked amendment; do not silently overwrite the V0–V1.3 narrative. V1.4 remains the preserved historical baseline; V1.5 is the user-requested current successor.

## V1.4 terminal-cell visual study (post-archive)

Following the V0–V1.4 archive commit, the first standalone **terminal-cell/ANSI V1.4 study** has been rendered. See [TERMINAL-V1.4-CELL-STUDY.md](TERMINAL-V1.4-CELL-STUDY.md) and [160×45 Review screenshot](terminal-previews/v1.4-review-160x45.png).

Nine original V1.4 fixed-size scenarios cover 80×24–200×60, dark/light/monochrome palettes, Unicode/ASCII, Minimal/Agents/Workspace and Git/PTY views. Each has genuine ANSI SGR truecolor escape sequences (readable in a spare terminal), plain text cells, cell-derived SVG and PNG. **This is visual proof of layout and color, not a ratatui/real-PTY integration or implementation plan.**

    bash .agents/plans/ux/render-terminal-previews.sh
    python3 .agents/plans/ux/validate-terminal-v1.4.py

Iterate the terminal visuals first; native ratatui/mouse/PTY/live resize proof is the next design stage.

## V1.5 polish: no duplicate document headers; one current aw palette

V1.5 now uses a **single contextual title**: the Workspace tab names an independent File/Diff; a split pane has a short title only when it distinguishes siblings. A maximized contained Files/Git viewer has one name, not a second view-level breadcrumb. Standalone File/Diff content starts directly under the selected tab with only a small worktree/site/scope line and a floating maximize/restore button. Immersive focus hides outer chrome but preserves Restore.

**One existing aw command palette, not an additional Quick Switch:** Ctrl+Shift+P or F1 opens its centered searchable modal; native category prefixes stay Actions (>), Windows (!), Files (@), Sessions (#), Terminals ($), Commands (/), and the proposed new Agents (&). Ordinary Ctrl+P remains composer/PTY-owned except the existing supported navigation fallback. The palette filters typed File/Diff/Agent/Session/Window/Terminal results and performs only preview/UI actions in the offline HTML.

- Canonical details: [V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md)
- Interactive page: [index.html](index.html) (F1 opens palette)
- Browser gallery: [VISUAL-GALLERY.md](VISUAL-GALLERY.md) with Palette, Agents-filtered, Files-filtered and Minimal palette captures.
- Terminal colors/cells: [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md), now 20 fixed-cell examples including four centered palette overlays.
- Reproduce: run the UX-only render/validation scripts already listed above; no runtime implementation or deployment.

All live aw source, palette implementation, current runtime worktree and tool grants remain **outside the scope of these UX mockups**. Historical V0–V1.4 preview files are preserved.
