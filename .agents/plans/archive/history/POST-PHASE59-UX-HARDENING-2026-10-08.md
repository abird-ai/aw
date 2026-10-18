# Post-Phase59 UX hardening — TUI mode grammar, semantic block padding, browser file visibility

**Status:** implemented and validated
**Implementation checkpoint:** `4821c06` (`Harden TUI mode UX and browser file rendering`)
**Numbered phase:** unchanged — `phase-59 -> 8b7d307af32fe0a70c3a3f334a1cc9fb0bb2ab05`
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## User-reported problems

This unnumbered hardening follows direct product review after Phase 59:

1. Pi-style semantic bands had better vertical breathing room than aw's prompt/tool strips.
2. aw's user-prompt renderer still inserted an implicit one-cell horizontal text inset even when configured output padding was zero.
3. presentation switching was exposed as several parallel slash commands (`/scrollback`, `/inline`, `/fullscreen`, `/agent`) instead of one composable `/tui` command.
4. Workspace should remain a distinct fullscreen application while still being a valid `/tui` destination.
5. Workspace should expose one simple `/minimal` return path to Agent-only presentation.
6. the current Solid browser Files view could show only CodeMirror line numbers while source text was effectively absent.

The work deliberately stays below provider/tool/storage/routing/governance authority.

## Current native mode grammar

Agent presentation and Workspace application ownership remain distinct, but the interactive command grammar is now unified:

```text
/tui
/tui scrollback
/tui inline
/tui fullscreen
/tui workspace

/workspace     # direct shortcut for /tui workspace
```

`/tui` without a value reports the current Agent TUI mode plus the canonical usage.

The old standalone presentation commands are removed from the shared slash registry:

```text
/scrollback
/inline
/fullscreen
/agent
```

They are no longer parallel command authorities or autocomplete entries.

### Why Workspace is a `/tui` target but not a persisted Agent `TuiMode`

`TuiTarget` is the shared interactive navigation target:

```text
Scrollback | Inline | Fullscreen | Workspace
```

Persisted `UiConfig.tui_mode` remains the three physical Agent presentation modes only:

```text
Scrollback | Inline | Fullscreen
```

This is intentional. Workspace owns a separate full-screen alternate-screen application with Explorer, Agent, Git, Files, terminals and layout state. Treating it as `TuiMode::Fullscreen` would collapse an application boundary into a renderer setting.

`tui::commands::tui_request()` is the single parser for `/tui` and the direct `/workspace` shortcut. Full and minimal Agent loops execute that typed request instead of independently parsing presentation strings.

A build without the Workspace feature still understands `workspace` as a canonical `/tui` target and fails explicitly when that application is unavailable.

## Workspace return contract

Inside Workspace, the presentation-switch surface is intentionally minimal:

```text
/minimal
```

`/minimal` exits the fullscreen Workspace application and returns to Agent-only presentation while preserving the currently configured Agent `tui_mode`.

Workspace no longer exposes:

```text
/agent
/inline
/scrollback
```

and it does not mutate Agent `UiConfig.tui_mode` during return.

The return is still blocked while the rejected-input recovery buffer requires explicit recover/discard handling.

## Semantic prompt/tool block padding

Shared Agent chrome now uses one `filled_block` primitive for colored semantic bands.

The user prompt band and success/failure/neutral tool bands have one filled blank row above and one below their content, producing the Pi-like vertical breathing room requested by the user.

Horizontal content padding remains controlled only by the existing settings:

- standalone Agent: `ui.output_padding`;
- Workspace Agent transcript: `ui.workspace_output_padding`.

The prompt renderer no longer manufactures a one-cell text inset or subtracts two hidden columns. Therefore:

```text
output_padding = 0
```

means zero horizontal text inset.

Nonzero configured values remain symmetric. Scrollback's separately committed prompt and final-turn paths now also apply `output_padding`, so Scrollback cannot silently diverge from Inline/Fullscreen.

The semantic band itself still spans the available transcript width; configured output padding controls the content inset.

## Solid browser Files visibility

The browser file API was inspected and already returned the complete UTF-8 source text unchanged. The failure was in the active SolidJS/CodeMirror presentation path, not the rooted file-read service.

The current `CodeFile` hardening does four things:

1. **Visible plain-text fallback**
   - `.cm-content` explicitly inherits `var(--fg)`.
   - old Shiki decorations are cleared before asynchronous syntax work.
   - syntax/highlighter failure leaves readable plain text instead of an invisible/stale styled layer.

2. **Generation fencing**
   - every async highlight request receives a per-`EditorView` generation.
   - stale results are rejected if another highlight request superseded them.

3. **Exact document/theme fencing**
   - the result applies only if the current CodeMirror document still exactly matches the captured text;
   - the result applies only if the effective theme still exactly matches the captured theme;
   - a theme change starts a fresh highlight pass rather than applying stale colors.

4. **File-identity viewport reset**
   - when the same CodeMirror component switches from one file to another, it scrolls to document start;
   - `scrollLeft` is explicitly reset to zero.

That last item directly prevents a retained horizontal scroll offset from leaving only the fixed line-number gutter visible after switching files.

The rebuilt tracked `web/dist/chunk-code-surface.js` contains the same behavior.

## Browser validation limitation

The production Vite/Solid bundle builds successfully and the source/built-chunk regression checker verifies the exact fallback/fencing/scroll-reset contract.

The connector environment does not currently have Python Playwright or a Chromium binary/runtime installed, so this closeout does **not** claim a fresh real-Chromium reproduction of the Solid CodeMirror pane. Existing JavaScript model/workspace suites remain green, but the current CodeMirror proof in this environment is source/bundle/build based.

## Validation

Fresh implementation evidence at `4821c06`:

- focused post-Phase59 UX/browser checker: **62/62**;
- full native `aw`: **245/245**;
- minimal/no-default warning-denied native `aw`: **73/73**, plus inherited library/provider tests;
- Phase 51 source checker: **83/83**;
- Phase 54 source checker: **138/138**;
- Phase 53 source checker: **56/56**;
- Phase 55 source checker: **117/117**;
- Phase 57 source checker: **54/54**;
- Phase 58 source checker: **38/38**;
- post-Phase58 review: **49/49**;
- post-Phase58 UX hardening: **163/163**;
- Workspace interaction/browser checker: **266/266**;
- browser JavaScript model/context/multiplayer/workspace tests: **102/102**;
- production Vite build: green; known lazy CodeMirror/Shiki chunk-size warning only;
- whole-workspace/all-target compilation: green;
- whole-workspace/all-target Clippy `-D warnings`: green;
- doctests and rustdoc warnings-as-errors: green;
- actual compiled Workspace PTY: **11/11**;
- Agent presentation PTY: **3/3**;
- terminal lifecycle PTY: **8/8**;
- natural-flow PTY: **6/6**;
- Scrollback saturation PTY: **4/4**;
- post-Phase58 resize/exit PTY: **4/4**;
- Phase-59 loaded-resource PTY: **3/3**;
- implementation-tree static validation: **1012 active files / 993 authored Rust test attributes / 737 local links / 0 errors / 0 warnings**.

The final source-bound reference profile is rerun after current documentation and handoff self-hash fields are frozen.

## Explicit non-goals

This hardening does not add:

- Phase 60;
- schema v33;
- a new provider/tool capability;
- a new Workspace execution mode;
- a second slash parser;
- a new UI settings persistence model;
- a new browser file API;
- browser write/edit authority;
- live-browser security certification;
- a skill runtime;
- any historical tag movement.

## Next gate

User review / explicit next task.

Do not infer Phase 60 automatically.
