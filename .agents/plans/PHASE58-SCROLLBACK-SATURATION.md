# Phase 58 — dynamic Scrollback scratch viewport and natural saturation

Status: authorized by the user after Phase 57 screenshots showed a remaining tall-terminal Scrollback gap.

## Boundary

Phase 58 changes Scrollback presentation only. SQLite remains v32. No model/tool/provider/Session/routing/governance authority changes.

## Root problem

Phase 57 correctly top-aligned visible Scrollback chrome, but Scrollback still used a fixed Ratatui `Viewport::Inline(height)` reserving up to 18 rows. On tall terminals this left an invisible/blank scratch reservation under the composer; after enough committed output, that fixed reservation made the active composer appear anchored in the middle even though terminal history was correct.

Moving those unused rows above the composer would only move the gap. The correct fix is to remove unused reservation rows entirely.

## Dynamic scratch viewport

Scrollback now sizes its Ratatui inline scratch viewport to the rows currently required by active UI state:

- idle default: compact AboveComposer status + composer;
- slash completion: add only visible popup rows;
- live Thinking/tool projection: add only visible live rows;
- notice/cancel state: add one notice row when present;
- Settings/keymap: temporarily expand to their actual requested rows;
- custom Above/BelowComposer status lines remain counted exactly.

The initial viewport is compact (at most four rows on a normal terminal), not 18 rows.

Before every Scrollback prompt/live draw, `scratch_required_rows(...)` computes the required height and `ensure_scratch_height(...)` recreates the inline viewport only when the requested height changes.

## Natural saturation and bottom stickiness

Ratatui inline viewports are anchored to the current cursor and reserve exactly their configured height. Scrollback observes the actual viewport rectangle and records when its bottom reaches the physical terminal bottom.

- Before natural saturation, resizing the scratch viewport stays anchored immediately after committed terminal history.
- Once the viewport has naturally reached the terminal bottom, saturation becomes sticky for that presenter.
- Subsequent scratch resizes (popup/live/settings collapse or terminal resize) re-anchor the new compact viewport against the physical bottom.
- No fake history rows are rendered and no fixed blank reservation remains between committed output and active chrome.

Thus Scrollback behaves like Inline: content advances naturally until the terminal fills, then the composer remains at the bottom.

## Terminal resize

On terminal resize, Scrollback keeps its current dynamic scratch height. If the presenter has already saturated, it force-reanchors that viewport to the new physical bottom. Before saturation, it retains normal cursor/history anchoring.

## Exit

Phase-57 clean exit semantics remain unchanged:

- `/quit` commits the final `› /quit` prompt;
- transient scratch content is cleared/collapsed;
- cursor returns to the actual viewport origin;
- terminal modes restore through the shared Phase-55 lifecycle guard;
- shell resumes immediately after committed aw output.

## Validation

- unit checks pin compact idle scratch sizing and transient expansion behavior;
- 50-row tmux smoke proves Scrollback starts unsaturated and naturally advances with real committed turns;
- saturation occurs only when real content reaches the physical bottom, not because of an 18-row reservation;
- subsequent short/error turn remains bottom-pinned after saturation;
- saturated `/quit` still returns a clean shell;
- Phase-57/56/55/54/53/52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts and native PTY smokes remain green.
