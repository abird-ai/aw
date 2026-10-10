# Visual gallery — design concept screenshots

These are optional screenshot artifacts rendered from the standalone HTML prototypes. They represent **browser renderings of a TUI look**, not genuine terminal cell rendering. Open [index.html](index.html) for fully interactive all-version comparisons.

| Historical point | Screenshot | Intended reading |
|---|---|---|
| V0 / Minimal | [preview](previews/v0-minimal.png) | Pi-like native conversation, little chrome |
| V0 / Workspace | [preview](previews/v0-workspace.png) | Original permanent right inspector |
| V1 / Workspace Review | [preview](previews/v1-review.png) | Herdr-inspired native Agent/Diff/PTY panes |
| V1.1 / Workspace | [preview](previews/v1.1-workspace.png) | Top Recents and quiet Activity |
| V1.2 / Space picker | [preview](previews/v1.2-space-switcher.png) | Global Space / Machine quick switching |
| V1.3 / Workspace | [preview](previews/v1.3-workspace.png) | Projects/Recents textual sidebar + unified Activity |
| V1.4 / Review | [preview](previews/v1.4-review.png) | Canonical two-tier icon-only sidebar |
| V1.4 / Mini Git | [preview](previews/v1.4-mini-git.png) | Sidebar Git independently of main Agent |
| V1.4 / Mini Files | [preview](previews/v1.4-mini-files.png) | Lazy file tree beside real Workspace |
| V1.4 / Full Git | [preview](previews/v1.4-full-git.png) | Dedicated rich Git and diff view |
| V1.4 / Full Files | [preview](previews/v1.4-full-files.png) | Full lazy Files tree and highlighted source |
| V1.4 / Full Recents | [preview](previews/v1.4-recents.png) | Flat navigation history; compact duplicate suppressed |
| V1.4 / Activity/Automations | [preview](previews/v1.4-automations.png) | Schedule definitions and run activity |
| V1.4 / Local Machine Space | [preview](previews/v1.4-local-x2.png) | Local Home working context is not a fake Git repo |
| V1.4 / Light palette | [preview](previews/v1.4-light.png) | Full work area with semantic light theme |
| V1.4 / Focus preset | [preview](previews/v1.4-focus.png) | One native Agent pane without secondary tiles |
| V1.4 / Split preset | [preview](previews/v1.4-split.png) | Agent and Diff, no bottom terminal |
| V1.4 / Terminals | [preview](previews/v1.4-terminals.png) | Four real-PTY-shaped panes (mock contents) |
| V1.4 / Agents | [preview](previews/v1.4-agents.png) | Native conversation and compact Agent hierarchy |

If a PNG file is missing on a target checkout or a screenshot capture was blocked, the linked HTML remains the canonical visual and this gallery row is an **intended screenshot slot**, not proof that capture passed. Validate with the screenshot checker before claiming all assets present.

## Post-archive terminal-cell derived gallery

Unlike the HTML concept screenshots above, these are generated from the same fixed character-cell matrix as the actual .ansi truecolor streams. Each PNG also has .ansi, .txt and .svg companions.

| V1.4 terminal-cell scenario | Screenshot |
| --- | --- |
| Review, 160×45 dark | [PNG](terminal-previews/v1.4-review-160x45.png) |
| Review, 200×60 wide | [PNG](terminal-previews/v1.4-review-200x60.png) |
| Mini Git, 120×40 | [PNG](terminal-previews/v1.4-mini-git-120x40.png) |
| Agents, 100×30 | [PNG](terminal-previews/v1.4-agents-100x30.png) |
| Minimal, 80×24 ASCII | [PNG](terminal-previews/v1.4-minimal-ascii-80x24.png) |
| Full Git, 160×45 light | [PNG](terminal-previews/v1.4-full-git-light-160x45.png) |
| Terminal grid, 160×45 | [PNG](terminal-previews/v1.4-terminals-160x45.png) |
| Narrow Review, 100×30 | [PNG](terminal-previews/v1.4-review-narrow-100x30.png) |
| Monochrome ASCII, 120×40 | [PNG](terminal-previews/v1.4-monochrome-120x40.png) |

These are not screenshots of live ratatui or PTY execution. See [TERMINAL-V1.4-CELL-STUDY.md](TERMINAL-V1.4-CELL-STUDY.md) for renderer and limitations.

## V1.5 — current canonical design (full interactive HTML)

Unlike historical V1.4, **V1.5 Minimal has no top header**, and the former Agents view is Workspace with its Agent pane maximized. Each File/Diff can be contained, opened as standalone tab, or maximized.

| Design screen | Live HTML-derived preview |
| --- | --- |
| Headerless Minimal | [v1.5-minimal.png](previews/v1.5-minimal.png) |
| Complete V1.5 Workspace Review | [v1.5-review.png](previews/v1.5-review.png) |
| Native Agent maximized, still Workspace | [v1.5-agent-max.png](previews/v1.5-agent-max.png) |
| Contained Files tab: explorer + viewer | [v1.5-files-contained.png](previews/v1.5-files-contained.png) |
| Files contained viewer maximized | [v1.5-files-editor-max.png](previews/v1.5-files-editor-max.png) |
| Independent standalone File tab | [v1.5-file-tab.png](previews/v1.5-file-tab.png) |
| Immersive standalone File maximized | [v1.5-file-tab-max.png](previews/v1.5-file-tab-max.png) |
| Contained Git tab: list + Diff | [v1.5-git-contained.png](previews/v1.5-git-contained.png) |
| Git's contained Diff maximized | [v1.5-git-diff-max.png](previews/v1.5-git-diff-max.png) |
| Independent standalone Diff tab | [v1.5-diff-tab.png](previews/v1.5-diff-tab.png) |
| Immersive standalone Diff maximized | [v1.5-diff-tab-max.png](previews/v1.5-diff-tab-max.png) |
| Full terminal grid | [v1.5-terminals.png](previews/v1.5-terminals.png) |
| Individual PTY maximized | [v1.5-terminal-max.png](previews/v1.5-terminal-max.png) |

These HTML previews are simulations, not a live Agent, real Git file, or PTY. The V1.5 switcher is [v1.5.html](v1.5.html); design authority is [V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md).

## V1.5 — terminal-cell / ANSI visual study

Real fixed character-cell paints, ANSI 24-bit colors, and matching .txt/.svg/.png from the same grid—not ratatui or an active PTY.

| Fixed-cell scenario | Preview |
| --- | --- |
| Minimal headerless, ASCII, 80×24 | [PNG](terminal-previews/v1.5-minimal-headerless-80x24.png) |
| Minimal headerless, Unicode, 120×40 | [PNG](terminal-previews/v1.5-minimal-headerless-120x40.png) |
| Full Workspace Review, 160×45 | [PNG](terminal-previews/v1.5-workspace-review-160x45.png) |
| Native Agent maximized, 120×40 | [PNG](terminal-previews/v1.5-agent-max-120x40.png) |
| Files contained, 160×45 | [PNG](terminal-previews/v1.5-files-contained-160x45.png) |
| Files editor maximized, 120×40 | [PNG](terminal-previews/v1.5-files-editor-max-120x40.png) |
| Standalone File, 160×45 | [PNG](terminal-previews/v1.5-file-tab-160x45.png) |
| Immersive File max, 120×40 | [PNG](terminal-previews/v1.5-file-tab-max-120x40.png) |
| Git contained, 160×45 | [PNG](terminal-previews/v1.5-git-contained-160x45.png) |
| Git diff maximized, 120×40 | [PNG](terminal-previews/v1.5-git-diff-max-120x40.png) |
| Standalone Diff, 160×45 | [PNG](terminal-previews/v1.5-diff-tab-160x45.png) |
| Immersive Diff max, 120×40 | [PNG](terminal-previews/v1.5-diff-tab-max-120x40.png) |
| Standalone PTY max, 120×40 | [PNG](terminal-previews/v1.5-terminal-max-120x40.png) |
| Light and monochrome | [Light](terminal-previews/v1.5-review-light-160x45.png) · [Monochrome](terminal-previews/v1.5-review-mono-100x30.png) |

Full inventory, generator and limitations: [TERMINAL-V1.5-CELL-STUDY.md](TERMINAL-V1.5-CELL-STUDY.md).

## V1.5 polished one-title / unified-palette gallery

The current V1.5 HTML screenshots above have been regenerated without a duplicate view-level title row. The standalone File/Diff tab labels are the sole document headings; contained viewers have one title and retained zoom controls.

| New interactive palette screenshot | Preview |
| --- | --- |
| One centered aw Command Palette in Workspace | [v1.5-palette.png](previews/v1.5-palette.png) |
| Filtered & Agents search | [v1.5-palette-agents.png](previews/v1.5-palette-agents.png) |
| Filtered @ Files search (File + Diff results) | [v1.5-palette-files.png](previews/v1.5-palette-files.png) |
| Same command palette over headerless Minimal | [v1.5-palette-minimal.png](previews/v1.5-palette-minimal.png) |

| New terminal-cell palette screenshot | Preview |
| --- | --- |
| Workspace palette 120×40 | [PNG](terminal-previews/v1.5-palette-workspace-120x40.png) |
| Agent-filtered palette 100×30 | [PNG](terminal-previews/v1.5-palette-agents-100x30.png) |
| File-filtered palette 120×40 | [PNG](terminal-previews/v1.5-palette-files-120x40.png) |
| Headerless Minimal with palette 80×24 | [PNG](terminal-previews/v1.5-palette-minimal-80x24.png) |

The browser palette and ANSI palette render one coherent aw command palette, not two overlapping quick-switch tools. All item actions in these screenshots are simulated.
