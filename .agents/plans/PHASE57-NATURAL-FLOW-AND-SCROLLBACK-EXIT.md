# Phase 57 — natural-flow Agent layout and Scrollback exit correctness

Status: authorized 2026-10-06 by the user after Phase 56 review.

## Boundary

Phase 57 changes native Agent presentation/layout/terminal handoff only. SQLite remains v32.

It must not change provider/tool/model authority, Session persistence, routing, subagents, planner/guardian governance, filesystem grants or browser authority.

## Goals

1. Agent Inline and Scrollback grow downward naturally with visible Agent content instead of pinning the composer to the physical bottom immediately.
2. Once natural content reaches the terminal bottom, the composer stays at the bottom and older transcript scrolls above it.
3. Scrollback exit commits the final visible Agent state and releases its reserved inline viewport before the shell resumes.
4. Default Agent status/notice/Thinking chrome lives above the composer; no persistent/default rows are rendered below it.
5. Fullscreen may continue to own the full alternate screen and keep bottom-anchored Agent chrome.

## Shared flow geometry

Introduce one pure flow-layout helper used by owned Inline and Scrollback drawing.

For a physical frame height and actual transcript/chrome row counts it computes:

- transcript visible rows;
- transient status/notice rows;
- popup rows;
- composer rows;
- trailing unused rows.

Inline natural-flow rule:

- when `transcript + chrome < terminal height`, allocate exactly those rows from the top and leave unused rows below;
- when `transcript + chrome >= terminal height`, allocate the remaining height to transcript and keep composer as the final active row group;
- transcript bottom-scroll is based on wrapped visual rows;
- popup/status changes may move the composer only by their actual transient row count, never by an arbitrary reserved screen height.

Fullscreen keeps its full-screen-owned geometry.

## Scrollback mode

Scrollback remains terminal-owned history with one Ratatui inline scratch viewport.

- Settled prompts/turns are inserted above the scratch viewport once.
- Scratch rendering is top-aligned: status/notice/popup/composer start immediately at the viewport origin. Unused scratch rows stay below the composer rather than between committed output and the composer.
- The scratch viewport may remain conservatively sized for settings/keymap/popup support, but its reserved height must never determine where visible active rows begin.
- On `/quit`, commit the typed `/quit` prompt above the viewport, clear the transient scratch viewport, move the cursor to the current viewport origin obtained from the last completed frame, then restore terminal modes.
- On double-Ctrl+C quit, clear/collapse the transient viewport and release from its current origin without inventing a prompt.
- Shell output after aw exits begins immediately after committed aw output; no stale status/composer rows remain beneath the shell prompt.

## Status hierarchy

Default `[ui].status_lines` changes from three `below-composer` rows to one compact `above-composer` row plus the existing `fullscreen-status` row.

Default above-composer row:

`model · directory · branch · profile · tools · state`

Session ID is omitted from the default compact row but remains available through Settings/custom `status_lines` and status commands.

Legacy exact Phase-54/55 default layouts (three below-composer rows plus the known fullscreen row) are automatically migrated in deserialization to the new default. Genuinely custom status-line layouts are preserved unchanged.

Legacy singular `status_line=[...]` maps to AboveComposer, not BelowComposer.

Settings toggles insert Agent status items into AboveComposer + FullscreenStatus by default.

Notices such as `Input cleared · Ctrl+C again to quit`, cancellation status and F6/F7 visibility state render above the composer in Inline/Scrollback. Live Thinking/tool activity remains in transcript/live projection above the composer. Default active layout has nothing persistent below the composer.

## Validation

At minimum:

- pure flow geometry tests prove short content leaves trailing blank rows below the composer, not above it;
- once content exceeds terminal height, composer is at the physical bottom;
- Inline short-history PTY/tmux proof shows composer immediately follows header/transcript rather than jumping to terminal bottom;
- Inline long-history/resize proof keeps composer bottom-pinned after natural saturation;
- Scrollback startup shows header then status/composer contiguously with no middle-screen jump;
- Scrollback settled output pushes the same active viewport downward naturally;
- `/quit` in Scrollback commits `/quit`, clears the scratch viewport and returns a shell immediately after aw output with no stale aw status below it;
- double-Ctrl+C Scrollback exit also leaves a clean shell;
- default UiConfig contains no BelowComposer rows;
- exact old default persisted status-lines migrate automatically while custom layouts do not;
- Settings status toggles target AboveComposer by default;
- default notice/status rows are above composer in Inline and Scrollback;
- Fullscreen existing alternate-screen ownership remains green;
- Phase-56/55/54/53/52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy -D warnings, doctests/rustdoc, feature cuts and native PTY smokes remain green.

## Deferred

- changing fullscreen to natural-flow geometry;
- arbitrary scrollback viewport resizing APIs inside Ratatui;
- browser Agent natural-flow parity;
- terminal private protocols beyond standard cursor/clear/scrollback behavior;
- execution/governance changes.
