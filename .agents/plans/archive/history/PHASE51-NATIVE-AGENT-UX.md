# Phase 51 — native Agent transcript and fullscreen UX convergence

Status: authorized 2026-10-06 by the user immediately after Phase 50.

## Boundary

Phase 51 is **presentation/input only**.

SQLite remains **v32**. No migration, provider/account/tool/routing/subagent/pipeline/team/guardian authority, billing behavior, Session provenance, filesystem grant, scheduler, or replay semantics change.

The goal is to make native `aw` feel like one coherent product across its two presentations:

- inline/regular mode keeps the Pi-like terminal-scrollback model;
- fullscreen keeps the richer workspace/navigation model;
- transcript semantics, colors, slash completion, thinking/tool visibility, composer hierarchy, and transition behavior are shared rather than separately approximated.

The supplied Pi/Codex screenshots are visual/interaction references, not dependencies or compatibility claims.

## Product direction

### Inline remains Pi-like

Keep:

- true terminal-owned scrollback;
- unboxed user prompts;
- plain assistant prose;
- muted italic safe Thinking labels;
- compact tool execution blocks;
- muted green successful tools;
- muted red failed/denied/interrupted tools;
- muted gray running/neutral tools;
- compact two-rule composer;
- no terminal-wide clear;
- slash completion below the composer;
- existing Codex-like status rows.

Inline must never promote itself into fullscreen after an ordinary turn. Entering fullscreen requires an explicit presentation action (`/fullscreen`, `/workspace`, Settings TUI-mode selection, or an explicit fullscreen startup preference).

### Fullscreen is the same Agent, not a second visual language

Fullscreen keeps:

- Explorer + Agent/Activity workspace;
- existing status line;
- durable Session/Agent navigation;
- command palette;
- file/diff/terminal/settings surfaces.

But the Agent transcript must use the **same semantic transcript palette** as inline:

- user prompt marker/text;
- assistant answer;
- Thinking activity/timing;
- tool success/failure/neutral strips;
- safe error/status text;
- after-turn performance rows.

No per-surface duplicate tool-color rules.

### Slash completion takes the best Codex behavior

Fullscreen slash autocomplete should visually match the compact Codex reference:

- directly attached above the composer;
- distinct neutral overlay background;
- command usage emphasized;
- descriptions visibly muted;
- selected row uses the accent background with deterministic high-contrast foreground;
- keyboard/mouse selection refers to exact rendered rows;
- no generic terminal reverse-video fallback when semantic colors are available.

Inline uses the same menu typography/selection semantics while retaining its required placement below the composer.

The workspace command palette should use the same selected-row semantic accent where appropriate.

### Tool execution presentation takes the best Pi behavior

A tool is one semantic block:

- command/tool title and its visible bounded output use the same status tone;
- success → muted green;
- failure/denied/interrupted/error output → muted red;
- running/missing/other → muted gray;
- collapsed mode keeps one compact status label and no detailed output;
- output remains sanitized/bounded;
- model/file text cannot inject terminal control sequences.

Do not add decorative cards, heavy borders, or permanent labels.

### Thinking

Thinking remains a safe activity/timing projection only.

- muted + italic;
- no raw/private provider reasoning;
- F6 changes current/future projection immediately;
- inline cannot erase already-written scrollback;
- fullscreen can repaint current projection.

### Composer and status hierarchy

- Inline: stable per-Agent accent top/bottom rules.
- Fullscreen: compact one-row composer with only the top separator; pane border remains the bottom boundary.
- Existing Codex-like status projection remains below/around the composer according to `status_lines` placement.
- No duplicate help/status sentence when the placeholder already communicates the action.
- Empty placeholder remains readable under System/Light/Dark.

## Presentation switching

Presentation switching must be explicit and lossless.

### Inline -> fullscreen

- same live Host/Agent/Session;
- same draft;
- same account/model/root/tools;
- retained PTYs remain retained;
- no model/tool replay;
- no history reset;
- no explicit physical-screen clear before first fullscreen draw;
- alternate-screen entry is allowed for the fullscreen workspace, but aw must avoid an extra visible clear/flicker cycle.

### Fullscreen -> inline

- `/inline` and `/minimal` set the persisted/live TUI mode to Inline **before** returning;
- returning inline from a startup-fullscreen session must not immediately reopen fullscreen on the next prompt/turn;
- same draft/history/resources survive;
- no terminal-wide clear/erase sequence;
- primary terminal scrollback remains intact because fullscreen uses the alternate screen only while active.

### Ordinary inline turn

An ordinary completed/cancelled/failed inline turn cannot transition to fullscreen merely because fullscreen had previously been active in the same process.

## Shared semantic styling

`src/tui/theme.rs` is the native semantic palette authority.

Phase 51 should centralize:

- user marker style;
- assistant style;
- thinking style;
- error style;
- tool-tone style;
- menu base/usage/description style;
- menu selected-row style.

Inline and fullscreen call these helpers rather than each owning independent literal style rules.

System mode may keep terminal fg/bg authoritative for ordinary content while still using explicit bounded semantic colors for tool strips and menus.

## Fullscreen transcript representation

The current private marker-based fullscreen text representation may remain as an internal layout device, but marker decoding must map to the shared semantic styles above.

Do not leak private marker codepoints into copied/readable transcript text.

## Screen-clearing rule

- Inline: never send terminal-wide clear/erase for prompt/turn/menu updates.
- Fullscreen: do not call an extra `terminal.clear()` after entering the alternate screen; the first Ratatui draw owns the complete frame.
- Leaving fullscreen restores the original terminal screen/scrollback through the existing guard.
- Onboarding may continue to manage its own bounded inline viewport; this phase does not redesign onboarding.

## Validation requirements

At minimum:

- fullscreen `/inline` mutates `ui_config.tui_mode` to Inline before exit;
- startup/fullscreen -> inline -> next prompt does not reopen fullscreen;
- ordinary inline turn with Inline mode stays Inline;
- fullscreen entry no longer calls `terminal.clear()`;
- inline mode still never uses `EnterAlternateScreen`;
- shared semantic tool style is used by inline and fullscreen;
- tool success/failure/neutral colors remain distinct in System/Light/Dark;
- fullscreen slash menu uses semantic background/usage/description/selected styles;
- inline slash menu uses the same menu styles;
- selected slash row is accent-backed, not unstyled/reverse-only;
- command palette selected row uses the semantic menu selection style;
- user prompt remains unboxed in both modes;
- assistant text remains normal foreground, no card background;
- Thinking remains muted italic and raw reasoning remains absent;
- collapsed tool display hides output in both modes;
- existing status-line projection remains unchanged;
- fullscreen composer remains one editable row plus top separator;
- exact current schema remains v32;
- Phase 50 checker and authority tests remain green;
- Phase 49–47 interaction/presentation checkers remain green;
- whole-workspace/all-targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts remain green;
- inline PTY and fullscreen workspace actual-binary smokes remain green.

## Deferred

Phase 51 does not add:

- markdown-rich block rendering beyond existing safe text projection;
- image rendering;
- terminal hyperlinks;
- arbitrary ANSI from model/tool output;
- new browser styling work;
- new workspace layout concepts;
- new keybinding families;
- new execution/scheduling/orchestration authority;
- schema changes.
