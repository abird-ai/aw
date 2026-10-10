# V1 — Native Agent + real terminal multiplexer and tiled Workspace

Status: historical design evolution. Preview: [v1.html](v1.html).

## Inherited (V0)

Preserve Minimal, Agents, Workspace, exact status semantics, git/repo fidelity, compact colors, nested subagent tree and Pi-like chat. **Do not replace native aw with a terminal-only renderer**.

## New concept from Herdr-inspired observations

All visual content belongs to one composable pane/layout system:

- Native Agent transcript: structured, owned Agent result, tools and approvals.
- Real OS PTY: terminal cells, alternate screen, input/resize and copy semantics intact; can host Pi, Claude Code, Codex, OpenCode, editor, shell or server.
- Git changes, Diff, File tree, File viewer, Terminals aggregate, Machines/Fleet state and Activity: all independent pane content types.
- Tabs name saved **arrangements**, not Sessions. A tab may mix Agent, diff and real PTY across machines. Closing/zooming a pane does not terminate or reauthorize its process.
- Standard presets: Focus (one Agent), Coordinate (Agent+tree), Review (Agent/Diff/Test PTY), Build (four real PTYs).
- Split horizontal/vertical, resize, focus ring, temporary zoom, restore layout, optional inspector instead of permanent V0 inspector.
- Detach/reattach UI and machine connection are presentation concerns, not permission to replay paid calls or external tool effects.

### Visual example

    ┌──────────┬───────────────┬────────────────┐
    │ project/ │ Agent root    │ Diff file.rs   │
    │ Agents   │ ● working     │ - old policy   │
    │ Machines │               │ + exact grant  │
    │          ├───────────────┴────────────────┤
    │          │ Test PTY · remote              │
    └──────────┴────────────────────────────────┘

## Interaction contracts

- One visual PaneRef can observe same Agent from multiple clients; process execution remains single-owner/custody and tool grants never come from a pane reference.
- Client viewer vs PTY input owner/resize owner must be explicit; remote disconnect shows last-known stale content, not active green.
- Native Agent keyboard editing wins in minimal. For PTY, ordinary keys go to application except configurable multiplexer prefix.
- Terminals are full-color + ANSI-aware; don't counterfeit Agent transcripts by screen scraping. External Agent status may be heuristic/presentation-only unless trusted integration.
- Responsive under terminal width pressure: inspector hides first, secondary panes collapse to a tab, focus never lost.
- Default space: side panes only where useful. More work area, less permanent chrome.

## What V1 did not yet solve

Sidebar still had separate Spaces/Machines/Projects/Agents/Terminals groups, which can become visually crowded; Recents and autonomous work are not yet elegantly organized. Those are addressed in V1.1–V1.4.
