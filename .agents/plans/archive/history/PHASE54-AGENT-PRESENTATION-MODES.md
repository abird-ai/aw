# Phase 54 — Agent presentation ownership and mode separation

Status: authorized 2026-10-06 by the user after Phase 53 review.

## Boundary

Phase 54 is **native application/presentation architecture only**. SQLite remains **v32**.

It must not change provider/account/tool/routing/subagent/planner/guardian authority, Session persistence semantics, model context, billing/retry behavior, filesystem grants, AwNet authority, or browser ownership.

The phase fixes a class of primary-terminal corruption caused by mixing terminal-owned scrollback with short-lived Ratatui inline viewports.

## Product modes

The top-level native product has two explicit application modes:

- `agent` — the ordinary Agent product; default.
- `workspace` — the richer Explorer/Agent/Files/Git/terminal workspace.

CLI:

- `aw` defaults to `--mode=agent`.
- `aw --mode=agent` is explicit ordinary Agent mode.
- `aw --mode=workspace` opens the full native Workspace.
- `aw workspace` opens the full native Workspace.
- `aw w` is a compatibility alias for `aw workspace`.
- existing typed `aw workspace <resource-subcommand>` operations remain available.
- `aw workspace open NAME` remains the explicit named-Workspace form.

Application mode and Agent TUI mode are separate concepts. `--tui-mode` applies only to Agent mode.

## Agent TUI modes

`TuiMode` becomes:

- `scrollback`
- `inline`
- `fullscreen`

CLI:

`--tui-mode=scrollback|inline|fullscreen`

Default:

- application mode: `agent`
- Agent TUI mode: `inline`

Persisted `[ui].tui_mode` uses the same three names. Existing `inline` and `fullscreen` configuration values remain valid; no migration is required.

### Scrollback

Scrollback is append-oriented terminal-owned history:

- settled turns are inserted above one persistent bounded Agent composer viewport;
- shell/terminal scrollback remains authoritative for prior Agent output;
- one Ratatui inline terminal exists for the entire Agent presentation lifetime rather than being recreated for prompt, popup, live turn, and final settlement;
- popup growth, live tool rows, and ordinary redraws may redraw only that reserved viewport;
- slash completion must never render at terminal row 0 merely because a later viewport was re-anchored there;
- final output is committed once to scrollback and never repainted afterward.

This is the closest mode to historical Pi-style terminal scrollback.

### Inline

Inline is the default Agent mode.

It is **not** Ratatui `Viewport::Inline`.

aw owns the complete visible primary terminal screen while the Agent UI is active:

- no alternate screen;
- one persistent full-terminal Ratatui renderer;
- transcript, live activity, composer, slash popup and status rows are all aw-owned bytes;
- resize redraws the whole owned primary screen from current Agent state;
- transient menus cannot overwrite unrelated shell/previous viewport rows;
- output before aw was launched is preserved in terminal scrollback by reserving a fresh primary-screen region before ownership begins;
- leaving aw does not erase terminal scrollback.

This mode is intended to make resize storms and repeated turns deterministic while retaining primary-screen behavior.

### Fullscreen

Fullscreen is the same Agent renderer/state model as Inline, but on the alternate screen:

- same Agent transcript;
- same composer;
- same slash menu;
- same status lines;
- same prompt/tool/Markdown/syntax semantics;
- same Session/provider/tool authority;
- only the physical screen ownership changes.

Fullscreen is **not** the full Workspace product.

### Workspace

Workspace keeps the existing richer multi-pane application:

- Explorer;
- Agent;
- Activity;
- Files/Git/Diff;
- terminals;
- command palette;
- durable Agent/session navigation.

Workspace is selected by application mode, not by `TuiMode::Fullscreen`.

`/workspace` from Agent mode may enter the full Workspace, but `/fullscreen` must only switch the same Agent presentation to Agent fullscreen.

## Persistent Agent presenter

The ordinary Agent interactive loop owns exactly one native Agent presenter at a time.

The presenter survives across:

- prompt editing;
- slash popup open/close;
- model streaming;
- tool streaming;
- turn settlement;
- subsequent prompt;
- terminal resize.

Do not create/drop one Ratatui terminal for `read_prompt` and a second terminal for `stream_turn`.

Changing Agent TUI mode replaces the physical presenter only after current activity is settled, carrying:

- same Host;
- same Agent/Session;
- same draft;
- same UI config;
- same provider/account/tool grants;
- same history.

No model/tool replay is allowed.

## Owned transcript

Inline/fullscreen redraw settled Agent history from authoritative current Agent turns plus the live in-flight projection.

- no out-of-band `println!/eprintln!` may write into an owned Agent screen while the presenter is active;
- recoverable notices/errors are rendered inside the Agent UI;
- settled failed/incomplete/cancelled turns use the same safe transcript/error projection as successful turns;
- command/status output that cannot be represented inline must suspend/release presentation ownership before physical terminal output, then restore the same presenter deterministically.

The ordinary provider-error class must not duplicate prompts or move the composer to stale coordinates.

## Resize

### Owned Inline / Fullscreen

A resize event:

- updates/recreates only the current full-screen Ratatui buffers as required;
- redraws the entire aw-owned frame;
- never relies on stale pre-resize cursor coordinates;
- never emits a second independent prompt viewport.

### Scrollback

A resize may recompute the one persistent bounded scrollback viewport, but it must not create a fresh viewport anchored from an unrelated cursor row.

## Slash popup

The slash popup is part of the persistent presenter state.

- scrollback: confined to the reserved Agent viewport;
- inline/fullscreen: drawn from the current full-frame layout;
- never painted at physical row 0 unless row 0 is actually part of the aw-owned frame;
- popup close restores the underlying Agent frame from state rather than terminal archaeology.

## Slash/application switching

Agent commands:

- `/scrollback` → Agent scrollback
- `/inline` → Agent inline
- `/fullscreen` → Agent fullscreen
- `/workspace` → full Workspace application
- `/minimal` → compatibility alias for `/scrollback`

Workspace:

- `/agent` returns to Agent application using the persisted/current Agent TUI mode;
- existing `/inline` compatibility may continue to return to Agent inline, but must no longer define Workspace as “fullscreen Agent”.

## Scrollbar semantics

All native scroll indicators use one semantic rule:

- **active/thumb/current viewport = brighter color**
- **inactive/track/outside viewport = dimmer/muted gray**

No pane may invert those roles.

The shared palette should expose semantic scrollbar styles/colors so individual call sites do not choose track/thumb ordering manually.

The glyph distinction may remain:

- track `│`
- thumb `┃`

but the brighter color always belongs to `┃`.

## Settings

Settings cycles Agent TUI mode in this order:

`scrollback → inline → fullscreen → scrollback`

Descriptions must explain physical ownership, not call Workspace fullscreen.

## Validation

At minimum:

- config parses/serializes all three `TuiMode` values and defaults to Inline;
- `--tui-mode=scrollback|inline|fullscreen` works in Agent mode;
- `--mode=agent|workspace` exists and defaults to Agent;
- `aw workspace` and `aw w` enter Workspace without requiring `open`;
- `aw workspace open NAME` and Workspace resource commands remain valid;
- Agent fullscreen no longer invokes the Workspace UI;
- `/fullscreen` stays in Agent application;
- `/workspace` explicitly enters Workspace;
- `/scrollback`, `/inline`, `/fullscreen` preserve same Agent/Session/draft/grants;
- one persistent scrollback terminal is used across prompt → live turn → next prompt;
- scrollback slash popup never overwrites unrelated terminal rows;
- default Inline uses full primary-screen ownership without `EnterAlternateScreen`;
- Fullscreen uses `EnterAlternateScreen`;
- Workspace uses its existing alternate-screen owner;
- terminal resize in default Inline redraws one clean owned frame;
- resize during slash popup/live tool activity remains geometrically correct;
- provider failure/cancel/incomplete settlement does not duplicate prompt/composer;
- old out-of-band `report` errors cannot write through an active owned screen;
- prior shell output remains in scrollback after entering/exiting owned Inline;
- all native scrollbar thumbs use the brighter color and tracks use the muted/faint color;
- Phase-53/52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts and actual PTY smokes remain green.

## Deferred

- browser presentation-mode parity;
- terminal emulator private protocols for perfect primary-screen snapshot restore;
- arbitrary retained scroll position/navigation inside the simple Agent transcript;
- workspace layout redesign;
- images/OSC-8/CommonMark extensions;
- execution/orchestration authority changes.
