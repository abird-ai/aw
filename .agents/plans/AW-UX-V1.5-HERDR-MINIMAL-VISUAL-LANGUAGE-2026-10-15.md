# AW UX v1.5 — Herdr-inspired minimal visual language

**Status:** Design specification, user-requested revision. **No implementation in this revision.**
**Supersession:** Additive visual refinement to AW UX **v1.4**; **v1.4 is retained unchanged**. Where specific border, tab, surface or semantic-color rules conflict, v1.5 takes precedence *for those visual details only*. All v1.4 information architecture, interaction specifications, keyboard commands, views, agent hierarchy, features and permissions stay in force.
**Reference:** User-provided Herdr screenshot on 2026-10-14 (reference image in conversation); Pi coding Agent transcript semantics. Reproduce the design principles, not Herdr's code or pixel-identical styling.

**Repository coordination:** This file intentionally lives directly in `.agents/plans/` and **not** `.agents/plans/ux/` because another agent exclusively owns the UX subtree and its Git staging. Merge/integrate into the canonical UX plan only through that agent's normal ownership process; do not overwrite/rewrite v1.4. This document is the independently reviewable v1.5 design handoff.

## 1. Visual thesis

AW should feel like a spacious, highly capable terminal workspace that mostly **does not draw boxes**. Herdr's screenshot illustrates the right grammar: nearly black shared canvas, a restrained shaded left navigation column, thin structural separation, a slim top tab strip, an active tab expressed by a compact filled selection, quiet metadata, and broad unornamented work area. Keep every AW capability but make the hierarchy visible through **placement, spacing, contrast and active state** rather than frames and permanent labels.

Principle: **surfaces are established by content; separators are exceptions**. The visual language is restrained, cohesive, composable and keyboard-first, with clear mouse hit targets.

## 2. Reference breakdown: what to borrow

- **Window:** dark neutral almost-black work canvas; sidebar a subtly distinct charcoal; no full-width rectangular panel frames.
- **Sidebar:** a minimal single structural vertical divider separating spaces/agents/navigation from the main surface. Text and selected-row wash establish structure.
- **Vertical rhythm:** concise panel titles in muted color, lightweight rows, substantial empty canvas; do not try to fill all space with borders.
- **Tabs:** a thin horizontal strip at the top of the work surface. Inactive tabs recede into strip background; active tab is a small, muted-accent filled rectangular tab spanning only its label/allocated width, not a bordered card. '+' is quiet and aligned with the tabs.
- **Selection:** one muted background band for the active sidebar row; no double outline. Small badges/dots only when they communicate real state.
- **Typography:** terminal-native monospace, subtle weight/contrast hierarchy, muted operational metadata. Colored text communicates facts, not decoration.
- **Terminal/process:** plain continuous canvas with working content at the upper edge. The shell's own prompt and cursor are not wrapped in another AW border.
- **Panel furniture:** subdued resize affordance on hover/focus only; avoid redundant header within a freshly opened or maximized file/terminal/diff/tab.

These are design observations and intentionally not exact sampled screenshot colors.

## 3. Shared semantic design tokens

Implement one semantic token layer used by native ratatui, browser/SolidJS and any other presented views; adapt to terminal capability (truecolor/256/16-color, contrast, light/dark). **Tokens express roles, not hard-coded RGB requirements.**

| Role | Intended treatment |
| --- | --- |
| `canvas` | near-black neutral in dark mode; natural uninterrupted work surface |
| `sidebar` | slightly distinguishable charcoal wash |
| `tab_strip` | dark neutral, very low emphasis |
| `tab_active` | compact muted-blue/accent fill, readable contrasting label |
| `tab_inactive` | transparent/strip-colored, secondary-contrast text |
| `selection` | understated row-width tinted wash without outline |
| `divider` | single hairline/one terminal cell, subdued contrast; only where essential |
| `text_primary` | crisp readable foreground |
| `text_secondary` | muted foreground for labels, secondary columns and timing |
| `text_disabled` | lower emphasis but distinguishable from readable metadata |
| `focus` | visibly distinct accessible keyboard focus, independent of selection |
| `prompt_user_bg` | **muted blue background across transcript width** |
| `prompt_user_fg` | high-contrast foreground over user band |
| `assistant_text` | ordinary unboxed foreground on canvas |
| `tool_running` | muted gray/neutral state |
| `tool_success` | **muted green status text / subtle wash** |
| `tool_failure` | **muted red status text / subtle wash**, including denied/unknown-error |
| `tool_warning` | muted amber only when distinct from failure and supported by semantics |
| `notice` | neutral to muted accent, not alert-red unless actionable failure |

Color is not the sole status cue: use compact semantic icons/words and appropriate terminal-safe styles. Achieve adequate contrast in all supported themes. Prefer dimmer tints over saturated full-surface green/red.

## 4. Tab specification: minimal, legible and consistent

**Unified tab component** for Editor files, Terminals, Diffs, Sessions, Agents, Workspaces and secondary panes wherever tab navigation is appropriate. It should borrow the screenshot's narrow, filled active tab.

1. A **single tab row**, no tab-outline boxes, double top borders, underline-plus-background duplication, or per-tab heavy separators.
2. Active tab: one muted accent/blue fill; tight horizontal padding; label contrasted; optional dirty/attention marker inside the same footprint.
3. Inactive tab: just text on tab-strip background; subtle hover/focus states.
4. New tab `+` and close actions are compact, aligned and visible on hover/focus or when discoverability requires. No large bordered buttons.
5. Tab order stable, keyboard cycling/wrap-around, pointer hit rects correct for varying label widths and scrolling. Hover/focus must not make inactive tab indistinguishable from active.
6. Overflow: horizontal scrolling or compact overflow affordance; never let tab labels wrap into a second header row. Preserve keyboard access and tab order.
7. Current tab's content has **one title source**. Fresh file, maximized file, terminal and diff must never display an AW tab title plus a redundant internal large filename/title header.
8. Pinned/unsaved/running/failed state indicators remain factual and low-noise. Narrow terminal layouts may hide explanatory text but not essential status/focus.
9. Divider under strip only if visual continuity makes it necessary. Prefer surface tone differential.

## 5. Borderless panel and chrome rules

- **Default:** panels have no perimeter borders. No nested boxes.
- **Allowed structural separators:** one quiet sidebar/workspace boundary, necessary split divider(s), and occasional header/content rule where semantic separation otherwise fails.
- **No duplicates:** don't stack sidebar divider + child-pane left border, or tab row + content title + breadcrumb with identical context.
- **Focus:** use local accent/selection wash, a focus indicator or cursor, not four-sided heavy rectangles; maintain keyboard focus accessibility.
- **Resize:** pointer cursor and subtle divider highlight on hover or drag; keyboard resize retains clear feedback.
- **Sidebars:** Spaces and Agents as understated headings, with content rows; preserve hierarchical grouping and expandable sub-agents. No forced cards for each agent.
- **Maximization:** expanded view removes surrounding pane chrome and unnecessary header while retaining one context-bearing tab strip or minimal title.
- **Empty states:** intentionally quiet canvas; short hint only when needed; never decorative boxes simply to occupy space.
- **Light theme:** same relative hierarchy, not a dark screenshot copied wholesale; borders remain rare and understated.

## 6. Agent transcript: Pi-like semantic backgrounds

The shared rendering model is **one Agent transcript**, consistent across minimal/inline, scrollback, fullscreen Agent, Workspace Agent pane and web projection. This refines the existing Phase 51 and post-Phase58 direction and should reuse the current shared renderer rather than adding per-surface variants.

**User prompts:** full available transcript width (within existing content gutters), **muted blue background**; one-cell-ish horizontal inset, sensible vertical rhythm; grapheme-aware wrapping; no separate `>` marker, no outer border, no chat bubble or rounded panel. Distinct visual group for each turn, with meaningful contrast in both themes. The blue band should not overwhelm adjacent content.

**Assistant prose:** ordinary unboxed canvas text. Paragraph rhythm does the grouping. Thinking/elapsed/safe activity metadata is muted, not given a large background card.

**Tools:** a **single compact semantic block**; running neutral gray, success muted green, failure/denied/interrupted muted red, and warnings only if factually distinct. Apply a coherent state tone to title/status and visible bounded output; prefer thin left-state marker or soft strip over an outlined box if structure is needed. Show success/failure by word/icon as well as color. Collapsed tool = one compact state row, not an empty frame. Use true execution state; UNKNOWN/unsettled must never be shown as successful. Tool output still sanitized, truncated and policy-governed.

**Composer:** Pi-like compact two-rule editor, with existing activity animation in the top rule, no additional surrounding box. Keep multiline input behavior, keyboard shortcuts, Ctrl+C semantics and current slash/palette positioning.

**Status:** preserve two compact status rows below composer; text hierarchy over chrome; factual model/provider/effort, tool/session and measured performance fields only. Avoid a third decorative toolbar.

## 7. The whole AW product, not just the screenshot view

Apply tokens and furniture rules to **minimal Agent**, hierarchical multi-Agent, Workspace, Spaces/projects/machines navigation, Explorer, Files/editor, Diff/Git, Terminal, Sessions, Queue/Activity, Settings, command palette, dialogs and browser UI. Do **not** remove commands, keyboard accessibility, process or security status, panes, source provenance or window management to achieve visual simplicity.

- Palette remains the existing **unified AW palette** for actions `>`, files `@`, terminals `$`, sessions `#`, windows `%`, commands `/`; add the previously designed Agents source in the same palette rather than a separate UI.
- Hover and active state follow shared tokens. Overlays should have only a subtle surface contrast/shadow where supported; no heavy border stacks.
- Critical destructive confirmations and denied permissions may need more contrast and clear textual warnings; minimalism must not hide them.
- Preserve existing source/Agent/WriterEpoch/Process identity and immutable history; this plan changes display only.

## 8. Acceptance scenarios and visual QA

1. **Screenshot parity check:** with a basic Workspace, main canvas has no perimeter box; sidebar has only the single quiet structural divider; tab strip shows one muted filled active tab and quiet inactive tabs.
2. **File-open/maximized:** exactly one file title/tab context; no double header or unnecessary breadcrumb copy.
3. **Agent turns:** multiline user prompt has consistent muted-blue width/padding; assistant unboxed; successful tool muted green, failed tool muted red, running muted neutral; UNKNOWN not success.
4. **Multiple tabs:** active/hover/focus/dirty markers remain distinguishable; labels don't wrap into duplicate headers; mouse and keyboard focus hit testing agree.
5. **View parity:** native inline, fullscreen Agent, Workspace and Solid web share semantic state treatment and no nested chrome.
6. **Constrained terminals:** 80x24, narrow widths, Unicode/graphemes, 16/256-color fallback, light/dark themes, zoom and terminal resizing are usable.
7. **State transitions:** split, maximize, detach/reopen, scrollback, agent hierarchy expand/collapse, modal/palette open, mouse resize and terminal restore do not leave border artifacts.
8. **Accessibility:** visible keyboard focus, labels/icons in addition to color, adequate foreground contrast, no essential status conveyed by blue/green/red alone.
9. **No authority regression:** presentation tests only; no changed Tool, provider billing, Queue, Actor, Process, SourceKind or restored-State behavior.

## 9. Implementation roadmap (future; not authorized by this document alone)

**V1.5-A — Token and visual inventory.** Audit native/web duplicated backgrounds/borders/tab presentation. Establish shared token roles, render-story fixtures, light/dark and fallbacks. No execution code changes.

**V1.5-B — Shell/panels/tabs.** Apply borderless composition, single separators, minimal filled active tabs, correct active/focus/hover, overflow and no-double-header policy. Test geometry and mouse hit rects.

**V1.5-C — Agent transcript.** Consolidate muted-blue prompt band and green/red/neutral Tool blocks through the current shared transcript renderer; no second status pipeline. Test all outcome states and Unicode wrapping.

**V1.5-D — Product-wide convergence.** Extend tab/chrome rules across Explorer/Git/Diff/Terminals/Agent hierarchy/Sessions/Settings/palette/web; narrow widths, resize, split/maximize and accessibility regression screenshots.

**V1.5-E — Native visual review.** Capture before/after from representative real terminal sizes and browser viewport, compare to screenshot principles, resolve remaining noisy boxes, document any deliberate exceptions. Keep all UX functionality and old v1.4 archived.

## 10. Non-goals / preserved decisions

- Do not overwrite or delete **v1.4**. This spec is **v1.5 additive**.
- No re-architecture of AW, new runtime primitive, new Hook, new renderer authority or independent Agent UI state machine.
- No new Phase69, migrations, indexing/LSP, E6 work, billing, retries, permissions, or restored-State activation.
- No copying Herdr copyrighted source/assets or introducing its dependency solely to mimic its screenshot.
- No automatic implementation changes or Git changes in the other UX agent's exclusive subtree without explicit coordination.
